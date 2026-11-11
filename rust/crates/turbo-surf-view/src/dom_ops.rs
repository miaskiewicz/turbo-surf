//! Element read/write helpers (port of `src/dom-ops.mjs`) backing the
//! Playwright-style accessors and locator actions. Over the static Tree, "live"
//! state (`.value`/`.checked`/`.selected`) is modeled by attributes — `fill`/
//! `set_checked`/`select_option` mutate them, the readers reflect them.
//!
//! IDL-only / geometry accessors from the JS impl (`jsProperty`,
//! `getBoundingClientRect`/`viewportRatio`, `activeElement`/`isFocused`) are not
//! modeled in Lane A; `css_value` is available via the cascade.

use turbo_dom_parser::rtdom::cascade::{computed_style, get_property_value};
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::{DocumentExt, Tree};

const EDITABLE_TAGS: &[&str] = &["INPUT", "TEXTAREA", "SELECT"];

/// Collapsed, trimmed text content.
pub fn text_of(tree: &Tree, h: Handle) -> String {
    tree.text_content(h)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `value` attribute (the filled value), or "".
pub fn input_value_of(tree: &Tree, h: Handle) -> String {
    tree.get_attribute(h, "value").unwrap_or("").to_string()
}

/// Not `disabled`.
pub fn is_enabled(tree: &Tree, h: Handle) -> bool {
    tree.get_attribute(h, "disabled").is_none()
}

/// Native checkbox/radio or supported ARIA checked state.
pub fn is_checked(tree: &Tree, h: Handle) -> bool {
    if tree.tag_name(h).as_deref() == Some("INPUT")
        && matches!(tree.get_attribute(h, "type"), Some("checkbox" | "radio"))
    {
        return tree.get_attribute(h, "checked").is_some();
    }
    tree.get_attribute(h, "role").is_some_and(|roles| {
        roles.split_whitespace().any(|role| {
            matches!(
                role,
                "checkbox" | "radio" | "switch" | "menuitemcheckbox" | "menuitemradio"
            )
        })
    }) && tree.get_attribute(h, "aria-checked") == Some("true")
}

fn is_content_editable(tree: &Tree, h: Handle) -> bool {
    matches!(
        tree.get_attribute(h, "contenteditable"),
        Some("") | Some("true")
    )
}

/// Editable per Playwright: contenteditable, or an enabled, non-readonly
/// input/textarea/select.
pub fn is_editable(tree: &Tree, h: Handle) -> bool {
    if is_content_editable(tree, h) {
        return true;
    }
    let tag = tree.tag_name(h).unwrap_or_default();
    EDITABLE_TAGS.contains(&tag.as_str())
        && is_enabled(tree, h)
        && tree.get_attribute(h, "readonly").is_none()
}

/// `toBeEmpty`: control value or descendant textual content is empty.
pub fn is_empty(tree: &Tree, h: Handle) -> bool {
    match tree.tag_name(h).as_deref() {
        Some("INPUT") => input_value_of(tree, h).trim().is_empty(),
        Some("TEXTAREA") => tree
            .get_attribute(h, "value")
            .map_or_else(|| tree.text_content(h), str::to_string)
            .trim()
            .is_empty(),
        _ => text_of(tree, h).is_empty(),
    }
}

// An <option>'s submitted value: `value` attr, else its label text.
fn option_value(tree: &Tree, opt: Handle) -> String {
    tree.get_attribute(opt, "value")
        .map_or_else(|| text_of(tree, opt), str::to_string)
}

/// Selected option values of a `<select>` (those with the `selected` attr).
pub fn selected_values(tree: &Tree, select: Handle) -> Vec<String> {
    tree.node(select)
        .query_selector_all("option")
        .iter()
        .filter(|o| tree.get_attribute(o.handle(), "selected").is_some())
        .map(|o| option_value(tree, o.handle()))
        .collect()
}

/// Computed CSS value (real cascade), backing `toHaveCSS`.
pub fn css_value(tree: &Tree, h: Handle, name: &str) -> String {
    get_property_value(&computed_style(tree, h), name)
}

/// Set/clear the `checked` attribute.
pub fn set_checked(tree: &mut Tree, h: Handle, on: bool) {
    if on {
        tree.set_attribute(h, "checked", "");
    } else {
        tree.remove_attribute(h, "checked");
    }
}

/// Select `<option>`(s) of a `<select>` by value or visible label. Returns true
/// if any matched.
pub fn select_option(tree: &mut Tree, select: Handle, value: &str) -> bool {
    let opts: Vec<Handle> = tree
        .node(select)
        .query_selector_all("option")
        .iter()
        .map(|o| o.handle())
        .collect();
    let mut matched = false;
    for opt in opts {
        let hit = tree.get_attribute(opt, "value") == Some(value) || text_of(tree, opt) == value;
        set_checked_attr(tree, opt, "selected", hit);
        matched |= hit;
    }
    matched
}

fn set_checked_attr(tree: &mut Tree, h: Handle, attr: &str, on: bool) {
    if on {
        tree.set_attribute(h, attr, "");
    } else {
        tree.remove_attribute(h, attr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(tree: &Tree, sel: &str) -> Handle {
        tree.query_selector(sel).unwrap()
    }

    #[test]
    fn enabled_checked_editable() {
        let tree = Tree::parse(
            "<input id='a'><input id='b' disabled><input id='c' type='checkbox' checked><input id='d' readonly>",
        );
        assert!(is_enabled(&tree, first(&tree, "#a")));
        assert!(!is_enabled(&tree, first(&tree, "#b")));
        assert!(is_checked(&tree, first(&tree, "#c")));
        assert!(!is_checked(&tree, first(&tree, "#a")));
        assert!(is_editable(&tree, first(&tree, "#a")));
        assert!(!is_editable(&tree, first(&tree, "#b"))); // disabled
        assert!(!is_editable(&tree, first(&tree, "#d"))); // readonly
    }

    #[test]
    fn checked_state_requires_native_type_or_aria_role() {
        let tree = Tree::parse("<div id='aria' role='checkbox' aria-checked='true'></div><div id='plain' checked></div><input id='text' checked><div id='mixed' role='switch' aria-checked='mixed'></div>");
        assert!(is_checked(&tree, first(&tree, "#aria")));
        for selector in ["#plain", "#text", "#mixed"] {
            assert!(!is_checked(&tree, first(&tree, selector)));
        }
    }

    #[test]
    fn contenteditable_is_editable() {
        let t1 = Tree::parse("<div contenteditable>x</div>");
        assert!(is_editable(&t1, first(&t1, "div")));
        let t2 = Tree::parse("<div contenteditable='true'>x</div>");
        assert!(is_editable(&t2, first(&t2, "div")));
        let t3 = Tree::parse("<div>x</div>");
        assert!(!is_editable(&t3, first(&t3, "div")));
    }

    #[test]
    fn empty_element() {
        let t1 = Tree::parse("<div id='a'></div>");
        assert!(is_empty(&t1, first(&t1, "#a")));
        let t2 = Tree::parse("<div id='a'>text</div>");
        assert!(!is_empty(&t2, first(&t2, "#a")));
        let t3 = Tree::parse("<div id='a'><span></span></div>");
        assert!(is_empty(&t3, first(&t3, "#a")));
        let controls = Tree::parse("<input id='filled' value='present'><input id='blank'><textarea id='text'>present</textarea>");
        assert!(!is_empty(&controls, first(&controls, "#filled")));
        assert!(is_empty(&controls, first(&controls, "#blank")));
        assert!(!is_empty(&controls, first(&controls, "#text")));
    }

    #[test]
    fn input_value_and_selected_values() {
        let t1 = Tree::parse("<input value='hi'>");
        assert_eq!(input_value_of(&t1, first(&t1, "input")), "hi");
        let t2 = Tree::parse(
            "<select><option value='a'>A</option><option selected value='b'>B</option><option selected>C</option></select>",
        );
        assert_eq!(selected_values(&t2, first(&t2, "select")), vec!["b", "C"]);
    }

    #[test]
    fn set_checked_toggles_attribute() {
        let mut tree = Tree::parse("<input type='checkbox'>");
        let h = first(&tree, "input");
        set_checked(&mut tree, h, true);
        assert!(is_checked(&tree, h));
        set_checked(&mut tree, h, false);
        assert!(!is_checked(&tree, h));
    }

    #[test]
    fn select_option_by_value_and_label() {
        let mut tree = Tree::parse(
            "<select><option value='a'>Apple</option><option value='b'>Banana</option></select>",
        );
        let sel = first(&tree, "select");
        assert!(select_option(&mut tree, sel, "b")); // by value
        assert_eq!(selected_values(&tree, sel), vec!["b"]);
        assert!(select_option(&mut tree, sel, "Apple")); // by label → switches
        assert_eq!(selected_values(&tree, sel), vec!["a"]);
        assert!(!select_option(&mut tree, sel, "nope"));
    }

    #[test]
    fn css_value_from_cascade() {
        let tree = Tree::parse("<div id='a' style='color:red'>x</div>");
        let v = css_value(&tree, first(&tree, "#a"), "color");
        // turbo-dom may canonicalize the color; just assert it resolved to red.
        assert!(v.contains("255") || v == "red", "got: {v}");
    }
}
