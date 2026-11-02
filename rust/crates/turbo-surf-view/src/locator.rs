//! Locator resolvers (port of the resolver core of `src/locator.mjs`) — the
//! Playwright-style getByRole / getByText / getByLabel / getByAttr matching
//! engine, over the turbo-dom `Tree`, returning matched node handles. Pure DOM,
//! no JS. (The `Locator` wrapper class — chaining, accessors, actions — is
//! Playwright-shim glue and lands with the napi tier, not here.)

use crate::aria::{accessible_name, role_of, text_of};
use std::collections::HashSet;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;

const ELEMENT_NODE: u8 = 1;

/// Text match mode. `Exact` compares trimmed strings; `Substring` is a
/// case-insensitive `contains` (the Playwright default); `Regex` treats `want`
/// as a `/source/flags` literal (Playwright passes `RegExp` names this way).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TextMode {
    Exact,
    Substring,
    Regex,
}

/// Is `want` a `/source/flags` regex literal (how a JS `RegExp` stringifies)?
pub fn is_regex_literal(want: &str) -> bool {
    want.len() >= 2 && want.starts_with('/') && want[1..].rfind('/').is_some()
}

/// `/source/flags` → compiled `Regex` (`i` flag honored via inline `(?i)`).
fn compile_regex_literal(want: &str) -> Option<regex::Regex> {
    let body = want.strip_prefix('/')?;
    let last = body.rfind('/')?;
    let (src, flags) = (&body[..last], &body[last + 1..]);
    let pattern = if flags.contains('i') {
        format!("(?i){src}")
    } else {
        src.to_string()
    };
    regex::Regex::new(&pattern).ok()
}

/// Does `value` match `want` under `mode` (after trimming)?
pub fn text_match(value: &str, want: &str, mode: TextMode) -> bool {
    let v = value.trim();
    match mode {
        TextMode::Exact => v == want,
        TextMode::Substring => v.to_lowercase().contains(&want.to_lowercase()),
        TextMode::Regex => compile_regex_literal(want).is_some_and(|re| re.is_match(v)),
    }
}

fn all_elements(tree: &Tree) -> Vec<Handle> {
    tree.query_selector_all("*").iter().copied().collect()
}

fn push_unique(out: &mut Vec<Handle>, seen: &mut HashSet<Handle>, h: Handle) {
    if seen.insert(h) {
        out.push(h);
    }
}

/// `getByRole(role, name?)` — elements whose resolved ARIA role equals `role`,
/// optionally filtered by accessible-name match.
pub fn by_role(tree: &Tree, role: &str, name: Option<(&str, TextMode)>) -> Vec<Handle> {
    all_elements(tree)
        .into_iter()
        .filter(|&h| role_of(tree, h) == role)
        .filter(|&h| match name {
            Some((want, mode)) => text_match(&accessible_name(tree, h), want, mode),
            None => true,
        })
        .collect()
}

fn has_matching_child(tree: &Tree, h: Handle, want: &str, mode: TextMode) -> bool {
    tree.descendants(h)
        .into_iter()
        .filter(|&c| tree.node_type_id(c) == ELEMENT_NODE)
        .any(|c| text_match(&text_of(tree, c), want, mode))
}

/// `getByText(want)` — innermost elements whose text matches and which have no
/// matching descendant (so the deepest holder wins).
pub fn by_text(tree: &Tree, want: &str, mode: TextMode) -> Vec<Handle> {
    all_elements(tree)
        .into_iter()
        .filter(|&h| {
            text_match(&text_of(tree, h), want, mode) && !has_matching_child(tree, h, want, mode)
        })
        .collect()
}

/// `getByAttr(attr, want)` — elements with `attr` whose value matches.
pub fn by_attr_text(tree: &Tree, attr: &str, want: &str, mode: TextMode) -> Vec<Handle> {
    tree.query_selector_all(&format!("[{attr}]"))
        .iter()
        .copied()
        .filter(|&h| match tree.get_attribute(h, attr) {
            Some(v) => text_match(v, want, mode),
            None => unreachable!("the [attr] selector only matches elements with {attr}"),
        })
        .collect()
}

// Controls a <label> labels: for=/id, aria-labelledby back-refs, wrapped input.
fn collect_label_targets(
    tree: &Tree,
    label: Handle,
    out: &mut Vec<Handle>,
    seen: &mut HashSet<Handle>,
) {
    if let Some(for_id) = tree.get_attribute(label, "for") {
        if let Some(t) = tree.get_element_by_id(for_id) {
            push_unique(out, seen, t);
        }
    }
    if let Some(id) = tree.get_attribute(label, "id") {
        for h in tree
            .query_selector_all(&format!("[aria-labelledby~=\"{id}\"]"))
            .iter()
        {
            push_unique(out, seen, *h);
        }
    }
    if let Some(input) = first_descendant_control(tree, label) {
        push_unique(out, seen, input);
    }
}

fn first_descendant_control(tree: &Tree, h: Handle) -> Option<Handle> {
    tree.descendants(h).into_iter().find(|&c| {
        matches!(
            tree.tag_name(c).as_deref(),
            Some("INPUT") | Some("SELECT") | Some("TEXTAREA")
        )
    })
}

/// `getByLabel(want)` — controls labelled by a matching `<label>` (for=/wrapped/
/// aria-labelledby), plus controls whose own `aria-label` matches.
pub fn by_label(tree: &Tree, want: &str, mode: TextMode) -> Vec<Handle> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for label in tree.query_selector_all("label").iter() {
        if text_match(&tree.text_content(*label), want, mode) {
            collect_label_targets(tree, *label, &mut out, &mut seen);
        }
    }
    for el in tree.query_selector_all("[aria-label]").iter() {
        if let Some(v) = tree.get_attribute(*el, "aria-label") {
            if text_match(v, want, mode) {
                push_unique(&mut out, &mut seen, *el);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use TextMode::{Exact, Regex, Substring};

    #[test]
    fn text_match_modes() {
        assert!(text_match("  Hello  ", "Hello", Exact));
        assert!(!text_match("Hello!", "Hello", Exact));
        assert!(text_match("Hello World", "hello", Substring));
        assert!(!text_match("Hello", "bye", Substring));
    }

    #[test]
    fn by_role_with_name() {
        let tree = Tree::parse("<button>Save</button><button>Cancel</button><a href='/x'>Save</a>");
        assert_eq!(by_role(&tree, "button", None).len(), 2);
        assert_eq!(by_role(&tree, "button", Some(("Save", Exact))).len(), 1);
        assert_eq!(by_role(&tree, "link", None).len(), 1);
    }

    #[test]
    fn by_role_regex_name() {
        // Playwright `getByRole('button', { name: /add employee/i })` arrives as
        // the stringified RegExp "/add employee/i".
        let tree = Tree::parse("<button>Add employee</button><button>Cancel</button>");
        assert!(is_regex_literal("/add employee/i"));
        assert!(!is_regex_literal("Add employee"));
        assert_eq!(
            by_role(&tree, "button", Some(("/add employee/i", Regex))).len(),
            1
        );
        assert_eq!(by_role(&tree, "button", Some(("/ADD/", Regex))).len(), 0);
        assert_eq!(by_role(&tree, "button", Some(("/ADD/i", Regex))).len(), 1);
    }

    #[test]
    fn by_text_picks_innermost() {
        let tree = Tree::parse("<div>wrapper <span>target</span></div>");
        let hits = by_text(&tree, "target", Substring);
        assert_eq!(hits.len(), 1);
        assert_eq!(tree.tag_name(hits[0]).as_deref(), Some("SPAN"));
    }

    #[test]
    fn by_attr_text_matches() {
        let tree = Tree::parse("<div data-test='one'>a</div><div data-test='two'>b</div>");
        assert_eq!(by_attr_text(&tree, "data-test", "one", Exact).len(), 1);
    }

    #[test]
    fn by_label_for_attribute() {
        let tree = Tree::parse("<label for='e'>Email</label><input id='e'>");
        let hits = by_label(&tree, "Email", Substring);
        assert_eq!(hits.len(), 1);
        assert_eq!(tree.tag_name(hits[0]).as_deref(), Some("INPUT"));
    }

    #[test]
    fn by_label_wrapped_input() {
        let tree = Tree::parse("<label>Name <input></label>");
        let hits = by_label(&tree, "Name", Substring);
        assert_eq!(hits.len(), 1);
        assert_eq!(tree.tag_name(hits[0]).as_deref(), Some("INPUT"));
    }

    #[test]
    fn by_label_aria_labelledby_backref() {
        let tree = Tree::parse("<label id='l'>Phone</label><input aria-labelledby='l'>");
        let hits = by_label(&tree, "Phone", Substring);
        assert_eq!(hits.len(), 1);
        assert_eq!(tree.tag_name(hits[0]).as_deref(), Some("INPUT"));
    }

    #[test]
    fn by_label_aria_label_on_control() {
        let tree = Tree::parse("<input aria-label='Search'>");
        assert_eq!(by_label(&tree, "Search", Exact).len(), 1);
    }

    #[test]
    fn by_label_dedupes_multiple_sources() {
        // for= and aria-label both point at the same control → counted once.
        let tree = Tree::parse("<label for='e'>Email</label><input id='e' aria-label='Email'>");
        assert_eq!(by_label(&tree, "Email", Exact).len(), 1);
    }
}
