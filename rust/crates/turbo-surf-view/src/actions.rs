//! Interaction = link/form graph traversal (port of `src/actions.mjs`, SPEC §6).
//! No JS runs, so we resolve the page's *intent graph*: `<a href>` navigates,
//! `<form>` serializes its successful controls into a GET/POST submission.
//! Pure over the Tree (Page wires these to navigation). Control "values" are the
//! `value`/`checked`/`selected` attributes (see `dom_ops` for the mutators).

use crate::dom_ops::text_of;
use std::sync::atomic::{AtomicUsize, Ordering};
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::{DocumentExt, Tree};
use turbo_surf_core::url::resolve;
use url::form_urlencoded;

const SUBMIT_TYPES: &[&str] = &["submit", "button", "reset", "image"];

/// What clicking an element does in Lane A (no JS).
#[derive(Debug, Clone, PartialEq)]
pub enum ClickIntent {
    /// Follow an `<a href>` to this absolute URL.
    Navigate(String),
    /// Submit the owning `<form>`.
    Submit(Submission),
    /// A JS-only handler — nothing to fire without JS.
    Inert,
}

/// A navigation produced by submitting a form.
#[derive(Debug, Clone, PartialEq)]
pub struct Submission {
    pub method: String,
    pub url: String,
    pub body: Option<String>,
    pub content_type: Option<String>,
}

/// Set a control's value (and checked state for checkbox/radio) via attributes.
pub fn fill_value(tree: &mut Tree, h: Handle, value: &str) {
    let tag = tree.tag_name(h).unwrap_or_default();
    let ty = tree
        .get_attribute(h, "type")
        .map(|t| t.to_ascii_lowercase());
    if tag == "INPUT" && matches!(ty.as_deref(), Some("checkbox") | Some("radio")) {
        set_bool_attr(tree, h, "checked", !value.is_empty());
        return;
    }
    tree.set_attribute(h, "value", value);
}

fn set_bool_attr(tree: &mut Tree, h: Handle, attr: &str, on: bool) {
    if on {
        tree.set_attribute(h, attr, "");
    } else {
        tree.remove_attribute(h, attr);
    }
}

fn lower_tag(tree: &Tree, h: Handle) -> String {
    tree.tag_name(h).unwrap_or_default().to_ascii_lowercase()
}

fn lower_type(tree: &Tree, h: Handle) -> Option<String> {
    tree.get_attribute(h, "type")
        .map(|t| t.to_ascii_lowercase())
}

fn control_value(tree: &Tree, h: Handle) -> String {
    tree.get_attribute(h, "value").unwrap_or("").to_string()
}

fn is_submit_control(tag: &str, ty: Option<&str>) -> bool {
    tag == "button" || (tag == "input" && ty.is_some_and(|t| SUBMIT_TYPES.contains(&t)))
}

fn is_checkable(tag: &str, ty: Option<&str>) -> bool {
    tag == "input" && matches!(ty, Some("checkbox") | Some("radio"))
}

fn select_pairs(tree: &Tree, select: Handle, name: &str) -> Vec<(String, String)> {
    tree.node(select)
        .query_selector_all("option")
        .iter()
        .filter(|o| tree.get_attribute(o.handle(), "selected").is_some())
        .map(|o| {
            let v = tree
                .get_attribute(o.handle(), "value")
                .map_or_else(|| text_of(tree, o.handle()), str::to_string);
            (name.to_string(), v)
        })
        .collect()
}

fn typed_pairs(
    tree: &Tree,
    h: Handle,
    name: &str,
    tag: &str,
    ty: Option<&str>,
) -> Vec<(String, String)> {
    if is_checkable(tag, ty) {
        return match tree.get_attribute(h, "checked") {
            Some(_) => vec![(
                name.to_string(),
                tree.get_attribute(h, "value").unwrap_or("on").to_string(),
            )],
            None => Vec::new(),
        };
    }
    if tag == "select" {
        return select_pairs(tree, h, name);
    }
    vec![(name.to_string(), control_value(tree, h))]
}

fn is_image_button(tree: &Tree, h: Handle) -> bool {
    lower_tag(tree, h) == "input" && lower_type(tree, h).as_deref() == Some("image")
}

// HTML "constructing the entry list": an image-button submitter contributes the click
// coordinates as `name.x`/`name.y` (bare `x`/`y` when unnamed), never its value. A
// synthetic click has no position, so both are 0, as in Chrome.
fn image_coordinate_pairs(name: &str) -> Vec<(String, String)> {
    let prefix = if name.is_empty() {
        String::new()
    } else {
        format!("{name}.")
    };
    vec![
        (format!("{prefix}x"), "0".to_string()),
        (format!("{prefix}y"), "0".to_string()),
    ]
}

// One control → its successful [name,value] pairs (HTML successful-controls).
fn control_pairs(tree: &Tree, h: Handle, submitter: Option<Handle>) -> Vec<(String, String)> {
    if Some(h) == submitter && is_image_button(tree, h) {
        return image_coordinate_pairs(tree.get_attribute(h, "name").unwrap_or(""));
    }
    let Some(name) = tree.get_attribute(h, "name") else {
        return Vec::new();
    };
    if name.is_empty() || tree.get_attribute(h, "disabled").is_some() {
        return Vec::new();
    }
    let tag = lower_tag(tree, h);
    let ty = lower_type(tree, h);
    if is_submit_control(&tag, ty.as_deref()) {
        // Only the activated submitter is successful.
        return if Some(h) == submitter {
            vec![(name.to_string(), control_value(tree, h))]
        } else {
            Vec::new()
        };
    }
    typed_pairs(tree, h, name, &tag, ty.as_deref())
}

/// A form's successful controls as `[name, value]` pairs. `submitter` (the
/// activated submit button), if named, contributes its name/value.
pub fn serialize_form(
    tree: &Tree,
    form: Handle,
    submitter: Option<Handle>,
) -> Vec<(String, String)> {
    let controls: Vec<Handle> = tree
        .node(form)
        .query_selector_all("input,select,textarea,button")
        .iter()
        .map(|c| c.handle())
        .collect();
    controls
        .into_iter()
        .flat_map(|c| control_pairs(tree, c, submitter))
        .collect()
}

fn encode(pairs: &[(String, String)]) -> String {
    let mut ser = form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

fn build_get(action: &str, pairs: &[(String, String)]) -> Submission {
    let url = match url::Url::parse(action) {
        Ok(mut u) => {
            u.set_query(Some(&encode(pairs)));
            u.to_string()
        }
        Err(_) => action.to_string(), // defensive: action is pre-resolved to absolute
    };
    Submission {
        method: "GET".into(),
        url,
        body: None,
        content_type: None,
    }
}

fn build_post(action: &str, pairs: &[(String, String)]) -> Submission {
    Submission {
        method: "POST".into(),
        url: action.to_string(),
        body: Some(encode(pairs)),
        content_type: Some("application/x-www-form-urlencoded".into()),
    }
}

static BOUNDARY_SEQ: AtomicUsize = AtomicUsize::new(0);

fn build_multipart(action: &str, pairs: &[(String, String)]) -> Submission {
    let seq = BOUNDARY_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let boundary = format!("----turboCrawlFormBoundary{seq}");
    let mut body = String::new();
    for (name, value) in pairs {
        body.push_str(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    Submission {
        method: "POST".into(),
        url: action.to_string(),
        body: Some(body),
        content_type: Some(format!("multipart/form-data; boundary={boundary}")),
    }
}

fn form_method(tree: &Tree, form: Handle) -> &'static str {
    let m = tree
        .get_attribute(form, "method")
        .unwrap_or("GET")
        .to_ascii_uppercase();
    if m == "POST" {
        "POST"
    } else {
        "GET"
    }
}

fn is_multipart(tree: &Tree, form: Handle) -> bool {
    tree.get_attribute(form, "enctype")
        .unwrap_or("")
        .to_ascii_lowercase()
        .contains("multipart")
}

fn form_action_url(tree: &Tree, form: Handle, base: &str) -> String {
    let action = tree.get_attribute(form, "action").unwrap_or("");
    resolve(base, action).unwrap_or_else(|| base.to_string())
}

fn ancestor_anchor_href(tree: &Tree, h: Handle) -> Option<String> {
    let mut cur = Some(h);
    while let Some(n) = cur {
        if tree.tag_name(n).as_deref() == Some("A") {
            if let Some(href) = tree.get_attribute(n, "href") {
                return Some(href.to_string());
            }
        }
        cur = tree.parent(n);
    }
    None
}

fn ancestor_form(tree: &Tree, h: Handle) -> Option<Handle> {
    let mut cur = Some(h);
    while let Some(n) = cur {
        if tree.tag_name(n).as_deref() == Some("FORM") {
            return Some(n);
        }
        cur = tree.parent(n);
    }
    None
}

fn is_submitter(tree: &Tree, h: Handle) -> bool {
    let tag = tree.tag_name(h).unwrap_or_default();
    let ty = tree
        .get_attribute(h, "type")
        .map(|t| t.to_ascii_lowercase());
    tag == "BUTTON" || (tag == "INPUT" && matches!(ty.as_deref(), Some("submit") | Some("image")))
}

/// Resolve what clicking element `h` does (no JS): follow an `<a href>`, submit
/// the owning `<form>`, or inert.
pub fn click_intent(tree: &Tree, h: Handle, base: &str) -> ClickIntent {
    if let Some(href) = ancestor_anchor_href(tree, h) {
        return ClickIntent::Navigate(resolve(base, &href).unwrap_or(href));
    }
    if let Some(form) = ancestor_form(tree, h) {
        let submitter = is_submitter(tree, h).then_some(h);
        return ClickIntent::Submit(build_submission(tree, form, base, submitter));
    }
    ClickIntent::Inert
}

/// Build the navigation a form submit produces.
pub fn build_submission(
    tree: &Tree,
    form: Handle,
    base: &str,
    submitter: Option<Handle>,
) -> Submission {
    let action = form_action_url(tree, form, base);
    let pairs = serialize_form(tree, form, submitter);
    if form_method(tree, form) == "GET" {
        return build_get(&action, &pairs);
    }
    if is_multipart(tree, form) {
        build_multipart(&action, &pairs)
    } else {
        build_post(&action, &pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(tree: &Tree, sel: &str) -> Handle {
        tree.query_selector(sel).unwrap()
    }

    const BASE: &str = "https://x.test/page";

    #[test]
    fn fill_text_and_checkbox() {
        let mut tree = Tree::parse("<input id='t'><input id='c' type='checkbox'>");
        let (t, c) = (first(&tree, "#t"), first(&tree, "#c"));
        fill_value(&mut tree, t, "hello");
        assert_eq!(tree.get_attribute(t, "value"), Some("hello"));
        fill_value(&mut tree, c, "on");
        assert!(tree.get_attribute(c, "checked").is_some());
        fill_value(&mut tree, c, "");
        assert!(tree.get_attribute(c, "checked").is_none());
    }

    #[test]
    fn get_form_serializes_to_query() {
        let tree = Tree::parse(
            "<form action='/search' method='get'><input name='q' value='rust'><input name='p' value='2'></form>",
        );
        let s = build_submission(&tree, first(&tree, "form"), BASE, None);
        assert_eq!(s.method, "GET");
        assert_eq!(s.url, "https://x.test/search?q=rust&p=2");
        assert_eq!(s.body, None);
    }

    #[test]
    fn post_form_serializes_to_body() {
        let tree =
            Tree::parse("<form action='/submit' method='POST'><input name='a' value='1'></form>");
        let s = build_submission(&tree, first(&tree, "form"), BASE, None);
        assert_eq!(s.method, "POST");
        assert_eq!(s.url, "https://x.test/submit");
        assert_eq!(s.body.as_deref(), Some("a=1"));
        assert_eq!(
            s.content_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn disabled_and_unnamed_controls_excluded() {
        let tree = Tree::parse(
            "<form method='get'><input name='a' value='1'><input name='b' value='2' disabled><input value='3'></form>",
        );
        let s = build_submission(&tree, first(&tree, "form"), BASE, None);
        assert_eq!(s.url, "https://x.test/page?a=1");
    }

    #[test]
    fn only_activated_submitter_contributes() {
        let tree = Tree::parse(
            "<form method='get'><input name='q' value='x'><button name='go' value='1'>Go</button><button name='cancel' value='1'>Cancel</button></form>",
        );
        let go = first(&tree, "[name=go]");
        let s = build_submission(&tree, first(&tree, "form"), BASE, Some(go));
        assert_eq!(s.url, "https://x.test/page?q=x&go=1");
    }

    #[test]
    fn checkbox_and_select_serialization() {
        let tree = Tree::parse(
            "<form method='get'><input type='checkbox' name='c' checked><input type='checkbox' name='d'><select name='s'><option value='a'>A</option><option value='b' selected>B</option></select></form>",
        );
        let s = build_submission(&tree, first(&tree, "form"), BASE, None);
        // checked checkbox → c=on; unchecked d omitted; selected option → s=b
        assert_eq!(s.url, "https://x.test/page?c=on&s=b");
    }

    #[test]
    fn multipart_form_body() {
        let tree = Tree::parse(
            "<form method='post' enctype='multipart/form-data'><input name='a' value='1'></form>",
        );
        let s = build_submission(&tree, first(&tree, "form"), BASE, None);
        assert_eq!(s.method, "POST");
        assert!(s
            .content_type
            .as_deref()
            .unwrap()
            .starts_with("multipart/form-data; boundary="));
        let body = s.body.unwrap();
        assert!(body.contains("Content-Disposition: form-data; name=\"a\""));
        assert!(body.contains("\r\n\r\n1\r\n"));
        assert!(body.trim_end().ends_with("--"));
    }

    #[test]
    fn action_defaults_to_base_when_absent() {
        let tree = Tree::parse("<form method='get'><input name='a' value='1'></form>");
        let s = build_submission(&tree, first(&tree, "form"), BASE, None);
        assert_eq!(s.url, "https://x.test/page?a=1");
    }

    #[test]
    fn click_intent_submits_the_owning_form_with_only_a_real_submitter() {
        let tree = Tree::parse(
            "<form action='/s' method='get'><span id='inner'><b id='deep'>x</b></span>\
             <input id='q' name='q' value='v'><button id='b' name='go' value='1'>Go</button>\
             <input id='sub' type='Submit' name='t' value='3'>\
             <input id='ib' type='button' name='b' value='4'><input id='ir' type='reset' name='r' value='5'></form>\
             <p id='out'>outside</p><a href='/l'><em id='em'>link</em></a>",
        );
        let submit = |sel: &str| match click_intent(&tree, first(&tree, sel), BASE) {
            ClickIntent::Submit(s) => s.url,
            other => panic!("{sel}: expected a submit, got {other:?}"),
        };
        // A non-submitter (text input, or any descendant) submits without a submitter entry.
        assert_eq!(submit("#q"), "https://x.test/s?q=v");
        assert_eq!(submit("#deep"), "https://x.test/s?q=v");
        // <button> (any type) and input type=submit (case-insensitive) are submitters.
        assert_eq!(submit("#b"), "https://x.test/s?q=v&go=1");
        assert_eq!(submit("#sub"), "https://x.test/s?q=v&t=3");
        // input type=button / type=reset are not submitters: never their own pair.
        assert_eq!(submit("#ib"), "https://x.test/s?q=v");
        assert_eq!(submit("#ir"), "https://x.test/s?q=v");
        assert_eq!(
            click_intent(&tree, first(&tree, "#out"), BASE),
            ClickIntent::Inert
        );
        assert_eq!(
            click_intent(&tree, first(&tree, "#em"), BASE),
            ClickIntent::Navigate("https://x.test/l".into())
        );
    }

    #[test]
    fn image_submitter_contributes_click_coordinates_like_chrome() {
        // Chrome 156: `new FormData(form, submitter)` for an element.click() on an
        // <input type=image> gives `q=v&i.x=0&i.y=0` (named) and `q=v&x=0&y=0` (unnamed);
        // the image button's value is never submitted (turbo-surf#55).
        let tree = Tree::parse(
            "<form action='/s' method='get'><input name='q' value='v'>\
             <input id='named' type='IMAGE' name='i' value='2'><input id='bare' type='image' value='5'></form>",
        );
        let submit = |sel: &str| match click_intent(&tree, first(&tree, sel), BASE) {
            ClickIntent::Submit(s) => s.url,
            other => panic!("{sel}: expected a submit, got {other:?}"),
        };
        assert_eq!(submit("#named"), "https://x.test/s?q=v&i.x=0&i.y=0");
        assert_eq!(submit("#bare"), "https://x.test/s?q=v&x=0&y=0");
        // Not the submitter: an image button contributes nothing.
        let form = first(&tree, "form");
        assert_eq!(
            build_submission(&tree, form, BASE, None).url,
            "https://x.test/s?q=v"
        );
    }

    #[test]
    fn get_submission_keeps_an_unparseable_action_verbatim() {
        // With no absolute base the action cannot be resolved; the submission keeps the
        // raw action (no query appended) rather than inventing a URL.
        let tree = Tree::parse("<form method='get'><input name='a' value='1'></form>");
        let s = build_submission(&tree, first(&tree, "form"), "relative/base", None);
        assert_eq!(s.method, "GET");
        assert_eq!(s.url, "relative/base");
        assert_eq!(s.body, None);
        assert_eq!(s.content_type, None);
    }
}
