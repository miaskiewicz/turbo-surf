//! Unified node-query (port of `src/query.mjs`): query by CSS selector OR XPath,
//! get back matches as `{ node, html, text }`. `Auto` treats a selector starting
//! with `/`, `.`, `./`, `..`, or `(` as XPath, else CSS; explicit `xpath=` /
//! `css=` prefixes choose their engine. Sizzle-only extensions (`:contains`)
//! are NOT supported — use the XPath `text()` predicate.

use crate::text::text;
use crate::xpath::{evaluate, XPath};
use turbo_dom_parser::rtdom::serialize::serialize_outer;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum QueryType {
    Auto,
    Css,
    Xpath,
}

/// One match. `node` is `None` for an XPath trailing-`@attr` value result, where
/// `value` carries the attribute string and `text` mirrors it.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct Match {
    #[serde(serialize_with = "serialize_handle_opt")]
    pub node: Option<Handle>,
    pub html: Option<String>,
    pub text: String,
    pub value: Option<String>,
}

// Opaque handle → raw u32 index on the JSON wire (preserves the pre-0.4 format).
fn serialize_handle_opt<S: serde::Serializer>(h: &Option<Handle>, s: S) -> Result<S::Ok, S::Error> {
    match h {
        Some(h) => s.serialize_some(&h.raw()),
        None => s.serialize_none(),
    }
}

fn looks_like_xpath(selector: &str) -> bool {
    let s = selector.trim_start();
    s.starts_with('/')
        || s.starts_with('(')
        || s.starts_with("./")
        || s == "."
        || s == ".."
        || s.starts_with("../")
}

fn selector_parts(selector: &str, ty: QueryType) -> (&str, QueryType) {
    let selector = selector.trim_start();
    if let Some(xpath) = selector.strip_prefix("xpath=") {
        return (xpath, QueryType::Xpath);
    }
    if let Some(css) = selector.strip_prefix("css=") {
        return (css, QueryType::Css);
    }
    (selector, resolve_type(selector, ty))
}

fn resolve_type(selector: &str, ty: QueryType) -> QueryType {
    match ty {
        QueryType::Css | QueryType::Xpath => ty,
        QueryType::Auto if looks_like_xpath(selector) => QueryType::Xpath,
        QueryType::Auto => QueryType::Css,
    }
}

fn describe(tree: &Tree, h: Handle) -> Match {
    Match {
        node: Some(h),
        html: Some(serialize_outer(tree, h)),
        text: text(tree, h),
        value: None,
    }
}

fn css_results(tree: &Tree, root: Handle, selector: &str) -> Vec<Match> {
    if root == tree.root() {
        return tree
            .query_selector_all(selector)
            .iter()
            .map(|&h| describe(tree, h))
            .collect();
    }
    turbo_dom_parser::rtdom::NodeRef::new(tree, root)
        .query_selector_all(selector)
        .iter()
        .map(|node| describe(tree, node.handle()))
        .collect()
}

fn xpath_results(tree: &Tree, root: Handle, selector: &str) -> Vec<Match> {
    match evaluate(tree, root, selector) {
        XPath::Values(values) => values
            .into_iter()
            .map(|value| Match {
                node: None,
                html: None,
                text: value.clone(),
                value: Some(value),
            })
            .collect(),
        XPath::Nodes(nodes) => nodes.into_iter().map(|h| describe(tree, h)).collect(),
    }
}

/// Query `tree` (rooted at `root`) by CSS or XPath.
pub fn query(tree: &Tree, root: Handle, selector: &str, ty: QueryType) -> Vec<Match> {
    let (selector, ty) = selector_parts(selector, ty);
    match ty {
        QueryType::Xpath => xpath_results(tree, root, selector),
        _ => css_results(tree, root, selector),
    }
}

/// Count matches without materializing HTML or text payloads.
pub fn query_count(tree: &Tree, root: Handle, selector: &str, ty: QueryType) -> usize {
    let (selector, ty) = selector_parts(selector, ty);
    match ty {
        QueryType::Xpath => match evaluate(tree, root, selector) {
            XPath::Values(values) => values.len(),
            XPath::Nodes(nodes) => nodes.len(),
        },
        _ => tree.query_selector_all(selector).len(),
    }
}

/// First match, or `None`.
pub fn query_first(tree: &Tree, root: Handle, selector: &str, ty: QueryType) -> Option<Match> {
    query(tree, root, selector, ty).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_engines_route_queries_and_scalar_counts() {
        let tree =
            Tree::parse("<section><button>inside</button></section><button>outside</button>");
        for selector in ["xpath=//button", "css=button"] {
            assert_eq!(
                query(&tree, tree.root(), selector, QueryType::Auto).len(),
                2
            );
            assert_eq!(
                query_count(&tree, tree.root(), selector, QueryType::Auto),
                2
            );
        }
        let section = tree.query_selector("section").unwrap();
        assert_eq!(
            query(&tree, section, "xpath=.//button", QueryType::Auto).len(),
            1
        );
    }

    #[test]
    fn count_matches_queries_without_describing_results() {
        let tree = Tree::parse("<a href='/x'>one</a><a href='/y'>two</a>");
        for (selector, ty) in [
            ("a", QueryType::Css),
            ("//a", QueryType::Auto),
            ("//a/@href", QueryType::Xpath),
            (".absent", QueryType::Auto),
        ] {
            assert_eq!(
                query_count(&tree, tree.root(), selector, ty),
                query(&tree, tree.root(), selector, ty).len()
            );
        }
    }

    #[test]
    fn auto_detects_css_vs_xpath() {
        assert_eq!(resolve_type("//a", QueryType::Auto), QueryType::Xpath);
        assert_eq!(resolve_type("(//a)", QueryType::Auto), QueryType::Xpath);
        assert_eq!(resolve_type("./a", QueryType::Auto), QueryType::Xpath);
        assert_eq!(resolve_type(".item", QueryType::Auto), QueryType::Css);
        // explicit override wins
        assert_eq!(resolve_type("//a", QueryType::Css), QueryType::Css);
    }

    #[test]
    fn css_query_returns_html_and_text() {
        let tree = Tree::parse("<ul><li class='x'>one</li><li class='x'>two</li></ul>");
        let r = query(&tree, tree.root(), ".x", QueryType::Auto);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].text, "one");
        assert!(r[0].html.as_deref().unwrap().contains("<li"));
        assert!(r[0].node.is_some());
    }

    #[test]
    fn css_scope_is_the_supplied_parent_and_keeps_global_handles() {
        let tree = Tree::parse("<section id='one'><button class='child'>first</button><div><button class='child'>nested</button></div></section><section><button class='child'>outside</button></section>");
        let parent = tree.query_selector("#one").unwrap();
        let direct = query(&tree, parent, ":scope > .child", QueryType::Css);
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].text, "first");
        assert_eq!(direct[0].node, Some(tree.query_selector("button").unwrap()));
        assert_eq!(query(&tree, parent, ".child", QueryType::Css).len(), 2);
    }

    #[test]
    fn xpath_nodes_and_first() {
        let tree = Tree::parse("<div><p>a</p><p>b</p></div>");
        let first = query_first(&tree, tree.root(), "//p", QueryType::Auto).unwrap();
        assert_eq!(first.text, "a");
    }

    #[test]
    fn xpath_attribute_values() {
        let tree = Tree::parse("<a href='/x'>x</a><a href='/y'>y</a>");
        let r = query(&tree, tree.root(), "//a/@href", QueryType::Auto);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].value.as_deref(), Some("/x"));
        assert_eq!(r[0].node, None);
        assert_eq!(r[1].text, "/y");
    }

    #[test]
    fn first_on_empty_is_none() {
        let tree = Tree::parse("<p>a</p>");
        assert!(query_first(&tree, tree.root(), ".nope", QueryType::Auto).is_none());
    }

    #[test]
    fn explicit_xpath_type() {
        let tree = Tree::parse("<div><b>x</b></div>");
        let r = query(&tree, tree.root(), "//b", QueryType::Xpath);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn matches_serialize_handles_as_raw_indices_and_values_as_null_nodes() {
        // The JSON wire format (pre-0.4): an element match carries its handle's raw u32
        // index, an XPath value match carries `"node": null`.
        let tree = Tree::parse("<a href='/x'>x</a>");
        let el = query_first(&tree, tree.root(), "a", QueryType::Css).unwrap();
        let raw = el.node.unwrap().raw();
        let json = serde_json::to_value(&el).unwrap();
        assert_eq!(json["node"], serde_json::json!(raw));
        assert_eq!(json["text"], "x");
        assert_eq!(json["value"], serde_json::Value::Null);
        let attr = query_first(&tree, tree.root(), "//a/@href", QueryType::Auto).unwrap();
        assert_eq!(
            serde_json::to_value(&attr).unwrap(),
            serde_json::json!({"node": null, "html": null, "text": "/x", "value": "/x"})
        );
    }
}
