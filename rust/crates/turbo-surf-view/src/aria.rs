//! ARIA role + accessible-name helpers (port of `src/aria.mjs`) — pragmatic,
//! no-layout heuristics for getByRole/getByText resolution and the agent view.

use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;

/// Collapsed, trimmed text content (the `dom-ops` `textOf` helper).
pub fn text_of(tree: &Tree, h: Handle) -> String {
    tree.text_content(h)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn input_role(type_attr: Option<&str>) -> &'static str {
    match type_attr {
        Some("checkbox") => "checkbox",
        Some("radio") => "radio",
        Some("button") | Some("submit") | Some("reset") => "button",
        _ => "textbox",
    }
}

/// Implicit ARIA role for a (lowercase) tag + optional input `type`.
pub fn implicit_role(tag: &str, type_attr: Option<&str>) -> &'static str {
    const ROLES: &[(&str, &str)] = &[
        ("a", "link"),
        ("button", "button"),
        ("select", "combobox"),
        ("textarea", "textbox"),
        ("h1", "heading"),
        ("h2", "heading"),
        ("h3", "heading"),
        ("h4", "heading"),
        ("h5", "heading"),
        ("h6", "heading"),
        ("img", "img"),
        ("table", "table"),
        ("thead", "rowgroup"),
        ("tbody", "rowgroup"),
        ("tfoot", "rowgroup"),
        ("tr", "row"),
        ("th", "columnheader"),
        ("td", "cell"),
        ("ul", "list"),
        ("ol", "list"),
        ("li", "listitem"),
        ("nav", "navigation"),
        ("main", "main"),
        ("article", "article"),
        ("aside", "complementary"),
        ("fieldset", "group"),
        ("option", "option"),
    ];
    if tag == "input" {
        return input_role(type_attr);
    }
    ROLES
        .iter()
        .find(|(name, _)| *name == tag)
        .map_or("generic", |(_, role)| *role)
}

pub fn heading_level(tree: &Tree, h: Handle) -> Option<u32> {
    tree.get_attribute(h, "aria-level")
        .and_then(|value| value.parse().ok())
        .or_else(|| match tree.tag_name(h).as_deref() {
            Some("H1") => Some(1),
            Some("H2") => Some(2),
            Some("H3") => Some(3),
            Some("H4") => Some(4),
            Some("H5") => Some(5),
            Some("H6") => Some(6),
            _ => None,
        })
}

/// Resolved role: explicit `role` attribute, else the implicit tag/type role.
pub fn role_of(tree: &Tree, h: Handle) -> String {
    if let Some(explicit) = tree.get_attribute(h, "role") {
        return explicit.to_string();
    }
    let tag = tree.tag_name(h).unwrap_or_default().to_ascii_lowercase();
    if tag == "th" && matches!(tree.get_attribute(h, "scope"), Some("row" | "rowgroup")) {
        return "rowheader".to_string();
    }
    let ty = tree
        .get_attribute(h, "type")
        .map(|s| s.to_ascii_lowercase());
    implicit_role(&tag, ty.as_deref()).to_string()
}

fn trimmed(s: Option<&str>) -> String {
    s.map(|v| v.trim().to_string()).unwrap_or_default()
}

/// Accessible name: aria-label > text > placeholder > value > title.
pub fn accessible_name(tree: &Tree, h: Handle) -> String {
    let candidates = [
        trimmed(tree.get_attribute(h, "aria-label")),
        if tree.tag_name(h).as_deref() == Some("IMG") {
            trimmed(tree.get_attribute(h, "alt"))
        } else {
            String::new()
        },
        tree.text_content(h).trim().to_string(),
        trimmed(tree.get_attribute(h, "placeholder")),
        trimmed(tree.get_attribute(h, "value")),
        trimmed(tree.get_attribute(h, "title")),
    ];
    candidates
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

fn id_list(tree: &Tree, h: Handle, attr: &str) -> Vec<String> {
    tree.get_attribute(h, attr)
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn resolve_ids(tree: &Tree, ids: &[String]) -> String {
    ids.iter()
        .filter_map(|id| tree.get_element_by_id(id))
        .map(|e| text_of(tree, e))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Accessible description: aria-describedby targets, else the `title` attribute.
pub fn accessible_description(tree: &Tree, h: Handle) -> String {
    let ids = id_list(tree, h, "aria-describedby");
    if !ids.is_empty() {
        return resolve_ids(tree, &ids);
    }
    trimmed(tree.get_attribute(h, "title"))
}

/// Accessible error message: aria-errormessage targets, only when aria-invalid.
pub fn accessible_error_message(tree: &Tree, h: Handle) -> String {
    if tree.get_attribute(h, "aria-invalid") != Some("true") {
        return String::new();
    }
    resolve_ids(tree, &id_list(tree, h, "aria-errormessage"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(tree: &Tree, sel: &str) -> Handle {
        tree.query_selector(sel).unwrap()
    }

    #[test]
    fn implicit_roles() {
        assert_eq!(implicit_role("a", None), "link");
        assert_eq!(implicit_role("button", None), "button");
        assert_eq!(implicit_role("select", None), "combobox");
        assert_eq!(implicit_role("textarea", None), "textbox");
        assert_eq!(implicit_role("input", Some("checkbox")), "checkbox");
        assert_eq!(implicit_role("input", Some("submit")), "button");
        assert_eq!(implicit_role("input", None), "textbox");
        assert_eq!(implicit_role("div", None), "generic");
    }

    #[test]
    fn native_heading_table_and_image_roles() {
        let tree = Tree::parse("<h2 id='heading'>Title</h2><table><tr><th id='column'>Name</th><th id='row' scope='row'>Alice</th><td id='cell'>Value</td></tr></table><img id='image' alt='Company logo'>");
        for (id, role) in [
            ("heading", "heading"),
            ("column", "columnheader"),
            ("row", "rowheader"),
            ("cell", "cell"),
            ("image", "img"),
        ] {
            assert_eq!(role_of(&tree, tree.get_element_by_id(id).unwrap()), role);
        }
        assert_eq!(
            heading_level(&tree, tree.get_element_by_id("heading").unwrap()),
            Some(2)
        );
        assert_eq!(
            accessible_name(&tree, tree.get_element_by_id("image").unwrap()),
            "Company logo"
        );
    }

    #[test]
    fn role_explicit_beats_implicit() {
        let tree = Tree::parse("<button role='tab'>x</button>");
        assert_eq!(role_of(&tree, first(&tree, "button")), "tab");
    }

    #[test]
    fn role_implicit_for_input_type() {
        let tree = Tree::parse("<input type='RADIO'>");
        assert_eq!(role_of(&tree, first(&tree, "input")), "radio");
    }

    #[test]
    fn accessible_name_priority() {
        let t1 = Tree::parse("<button aria-label='Lbl'>Text</button>");
        assert_eq!(accessible_name(&t1, first(&t1, "button")), "Lbl");
        let t2 = Tree::parse("<button>  Text  </button>");
        assert_eq!(accessible_name(&t2, first(&t2, "button")), "Text");
        let t3 = Tree::parse("<input placeholder='Ph'>");
        assert_eq!(accessible_name(&t3, first(&t3, "input")), "Ph");
        let t4 = Tree::parse("<input value='V'>");
        assert_eq!(accessible_name(&t4, first(&t4, "input")), "V");
        let t5 = Tree::parse("<a title='T'></a>");
        assert_eq!(accessible_name(&t5, first(&t5, "a")), "T");
        let t6 = Tree::parse("<span></span>");
        assert_eq!(accessible_name(&t6, first(&t6, "span")), "");
    }

    #[test]
    fn description_from_ids_then_title() {
        let t1 =
            Tree::parse("<input aria-describedby='d1 d2'><p id='d1'>one</p><p id='d2'>two</p>");
        assert_eq!(accessible_description(&t1, first(&t1, "input")), "one two");
        let t2 = Tree::parse("<input title='Tip'>");
        assert_eq!(accessible_description(&t2, first(&t2, "input")), "Tip");
    }

    #[test]
    fn error_message_only_when_invalid() {
        let ok = Tree::parse("<input aria-errormessage='e'><p id='e'>bad</p>");
        assert_eq!(accessible_error_message(&ok, first(&ok, "input")), "");
        let bad = Tree::parse("<input aria-invalid='true' aria-errormessage='e'><p id='e'>bad</p>");
        assert_eq!(accessible_error_message(&bad, first(&bad, "input")), "bad");
    }
}
