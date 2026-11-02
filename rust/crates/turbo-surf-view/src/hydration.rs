//! No-JS hydration-state mining (port of `src/hydration.mjs`): recover SPA data
//! that frameworks ship server-side inside inline `<script>` tags, with zero JS
//! execution. Next.js `__NEXT_DATA__`, JSON-LD, typed `application/json`
//! islands, and `window.X = <json>` global assignments (parsed without eval).

use serde_json::Value;
use std::collections::BTreeMap;
use turbo_dom_parser::rtdom::Tree;

const GLOBAL_KEYS: &[&str] = &[
    "__INITIAL_STATE__",
    "__APOLLO_STATE__",
    "__PRELOADED_STATE__",
    "__NUXT__",
    "__remixContext",
];

/// Mined hydration state. Maps are sorted (BTreeMap) for stable output.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct Hydration {
    pub next: Option<Value>,
    pub json_ld: Vec<Value>,
    pub json: BTreeMap<String, Value>,
    pub states: BTreeMap<String, Value>,
}

fn try_parse(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim()).ok()
}

fn parse_json_script(tree: &Tree, selector: &str) -> Option<Value> {
    let h = tree.query_selector(selector)?;
    try_parse(&tree.text_content(h))
}

fn parse_json_ld(tree: &Tree) -> Vec<Value> {
    tree.query_selector_all(r#"script[type="application/ld+json"]"#)
        .iter()
        .filter_map(|&h| try_parse(&tree.text_content(h)))
        .collect()
}

fn parse_typed_json(tree: &Tree) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for &h in tree
        .query_selector_all(r#"script[type="application/json"]"#)
        .iter()
    {
        let Some(id) = tree.get_attribute(h, "id") else {
            continue;
        };
        if id.is_empty() || id == "__NEXT_DATA__" {
            continue; // __NEXT_DATA__ is surfaced as `next`
        }
        if let Some(v) = try_parse(&tree.text_content(h)) {
            out.insert(id.to_string(), v);
        }
    }
    out
}

// Concatenated text of all inline (no src) scripts — where globals are assigned.
fn inline_script_text(tree: &Tree) -> String {
    let mut text = String::new();
    for &h in tree.query_selector_all("script").iter() {
        if tree.get_attribute(h, "src").is_none() {
            text.push('\n');
            text.push_str(&tree.text_content(h));
        }
    }
    text
}

// Byte index of the next '{' or '[' at/after `from`.
fn find_bracket(bytes: &[u8], from: usize) -> Option<usize> {
    (from..bytes.len()).find(|&i| bytes[i] == b'{' || bytes[i] == b'[')
}

// From the '{'/'[' at `start`, slice to the matching close (string/escape aware).
fn slice_balanced(text: &str, start: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &ch) in bytes.iter().enumerate().skip(start) {
        if esc {
            esc = false;
        } else if in_str {
            match ch {
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
        } else if ch == b'"' {
            in_str = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..=i]);
            }
        }
    }
    None
}

// Whether `text[i..]` begins with `key` followed (after optional whitespace) by '='.
fn assignment_at(bytes: &[u8], i: usize, key: &[u8]) -> Option<usize> {
    if !bytes[i..].starts_with(key) {
        unreachable!("callers locate `key` at `i` with find() first");
    }
    let mut j = i + key.len();
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    (j < bytes.len() && bytes[j] == b'=').then_some(j + 1)
}

// Extract `<key> = <json>` from inline script text.
fn parse_assignment(text: &str, key: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    let kb = key.as_bytes();
    let mut i = 0;
    while let Some(rel) = text[i..].find(key) {
        let at = i + rel;
        if let Some(after_eq) = assignment_at(bytes, at, kb) {
            if let Some(br) = find_bracket(bytes, after_eq) {
                if let Some(js) = slice_balanced(text, br) {
                    if let Some(v) = try_parse(js) {
                        return Some(v);
                    }
                }
            }
        }
        i = at + key.len();
    }
    None
}

fn parse_global_states(tree: &Tree) -> BTreeMap<String, Value> {
    let text = inline_script_text(tree);
    let mut out = BTreeMap::new();
    for key in GLOBAL_KEYS {
        if let Some(v) = parse_assignment(&text, key) {
            out.insert(key.to_string(), v);
        }
    }
    out
}

/// Mine server-embedded hydration state (no JS executed).
pub fn extract_hydration_state(tree: &Tree) -> Hydration {
    Hydration {
        next: parse_json_script(tree, "#__NEXT_DATA__"),
        json_ld: parse_json_ld(tree),
        json: parse_typed_json(tree),
        states: parse_global_states(tree),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn next_data_json() {
        let tree = Tree::parse(
            r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"a":1}}</script>"#,
        );
        let h = extract_hydration_state(&tree);
        assert_eq!(h.next, Some(json!({"props": {"a": 1}})));
        // __NEXT_DATA__ is NOT duplicated into `json`
        assert!(h.json.is_empty());
    }

    #[test]
    fn json_ld_blocks() {
        let tree = Tree::parse(
            r#"<script type="application/ld+json">{"@type":"Article"}</script>
               <script type="application/ld+json">[{"x":1}]</script>"#,
        );
        let h = extract_hydration_state(&tree);
        assert_eq!(h.json_ld.len(), 2);
        assert_eq!(h.json_ld[0], json!({"@type": "Article"}));
    }

    #[test]
    fn typed_json_islands_keyed_by_id() {
        let tree = Tree::parse(
            r#"<script type="application/json" id="data">{"k":"v"}</script>
               <script type="application/json">{"noid":1}</script>"#,
        );
        let h = extract_hydration_state(&tree);
        assert_eq!(h.json.get("data"), Some(&json!({"k": "v"})));
        assert_eq!(h.json.len(), 1); // the id-less one is dropped
    }

    #[test]
    fn global_assignments_parsed_without_eval() {
        let tree = Tree::parse(
            r#"<script>window.__INITIAL_STATE__ = {"user":{"id":7}};
               window.__NUXT__=[1,2,3];</script>"#,
        );
        let h = extract_hydration_state(&tree);
        assert_eq!(
            h.states.get("__INITIAL_STATE__"),
            Some(&json!({"user": {"id": 7}}))
        );
        assert_eq!(h.states.get("__NUXT__"), Some(&json!([1, 2, 3])));
    }

    #[test]
    fn nested_braces_and_strings_balance() {
        // Braces inside a string must not confuse the balancer.
        let tree = Tree::parse(
            r#"<script>__APOLLO_STATE__ = {"q":"a}b{c","n":{"m":1}}; var x=2;</script>"#,
        );
        let h = extract_hydration_state(&tree);
        assert_eq!(
            h.states.get("__APOLLO_STATE__"),
            Some(&json!({"q": "a}b{c", "n": {"m": 1}}))
        );
    }

    #[test]
    fn escaped_quotes_inside_string_balance() {
        // A backslash-escaped quote must not end the JSON string early.
        let tree = Tree::parse(r#"<script>__NUXT__ = {"s":"a\"b}c"};</script>"#);
        let h = extract_hydration_state(&tree);
        assert_eq!(h.states.get("__NUXT__"), Some(&json!({"s": "a\"b}c"})));
    }

    #[test]
    fn external_scripts_ignored_for_globals() {
        let tree = Tree::parse(r#"<script src="/app.js">window.__NUXT__={"x":1}</script>"#);
        // src present → not inline → assignment text not scanned.
        assert!(extract_hydration_state(&tree).states.is_empty());
    }

    #[test]
    fn bad_json_is_skipped() {
        let tree = Tree::parse(
            r#"<script id="__NEXT_DATA__" type="application/json">{not valid}</script>
               <script>window.__NUXT__ = {oops;</script>"#,
        );
        let h = extract_hydration_state(&tree);
        assert_eq!(h.next, None);
        assert!(h.states.is_empty());
    }

    #[test]
    fn empty_document_yields_empty_state() {
        let tree = Tree::parse("<p>plain</p>");
        assert_eq!(extract_hydration_state(&tree), Hydration::default());
    }
}
