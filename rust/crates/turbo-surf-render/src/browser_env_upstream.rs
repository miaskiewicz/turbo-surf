// Browser environment shim (P3) — the V8 BINDING + browser globals over turbo-dom's pure-Rust `rtdom` (the `turbo-dom-parser` crate, built with
// `rust-runtime`) to V8 via native rusty-v8 callbacks, so tests get `window`/`document`/Element with
// NO JS DOM (no `installGlobals`, no `.node` parser, no node_modules turbo-dom).
//
// Model: the DOM lives in Rust as one `Tree` per context (handle-based — each node is a `u32`
// `Handle`). A JS DOM node is a V8 object with the handle in internal field 0; methods/accessors
// are native callbacks that read the handle, call `Tree`, and wrap result handles back into node
// objects. A `handle → V8 object` cache preserves identity (`el === el`).
//
// This is the P3.2 vertical slice: document/createElement/createTextNode, appendChild/insertBefore/
// removeChild, get/setAttribute, id/className/tagName/textContent, querySelector(All)/getElementById.
// Events, cssom/style, classList, forms etc. grow in P3.3. Gated by `TURBO_RUST_DOM`.

use std::cell::RefCell;
use std::collections::HashMap;

use turbo_dom_parser::rtdom::node_ref::DocumentExt;
use turbo_dom_parser::rtdom::tree::{Handle, Namespace, Tree};
use turbo_dom_parser::rtdom::NodeRef;

/// The all-Rust DOM (rtdom) is now the ONLY DOM environment — the JS turbo-dom bootstrap
/// (installGlobals + the `.node` parser) was removed in v0.3.0. Kept as a function (always true) so
/// the per-file install/reset call sites read intent; `TURBO_RUST_DOM` is no longer consulted.
pub fn enabled() -> bool {
    true
}

/// Whether to RECORD missing-member accesses (debug surface map). The interceptor itself is always
/// installed (for graceful degradation — an unimplemented member returns undefined instead of
/// crashing the file); recording is the only extra cost and is gated here so production pays nothing
/// but a cached bool check + a small fall-through. Cached once (no per-access getenv).
pub fn log_enabled() -> bool {
    use std::sync::OnceLock;
    static LOG: OnceLock<bool> = OnceLock::new();
    *LOG.get_or_init(|| std::env::var("TURBO_RUST_DOM_LOG").map(|v| !v.is_empty() && v != "0").unwrap_or(false))
}

/// Names the native DOM binds (template methods/accessors + document own methods) + JS internals —
/// the interceptor falls through for these and only records the REST as "missing".
const KNOWN: &[&str] = &[
    // element methods + accessors
    "appendChild", "removeChild", "insertBefore", "setAttribute", "getAttribute", "hasAttribute",
    "removeAttribute", "querySelector", "querySelectorAll", "tagName", "parentNode", "firstChild",
    "nextSibling", "textContent", "id", "className",
    "style", "oninput", "onchange", "onclick",
    "nodeType", "nodeName", "childNodes", "ownerDocument",
    "addEventListener", "removeEventListener", "dispatchEvent",
    "matches", "contains",
    "innerHTML", "outerHTML", "children", "parentElement", "firstElementChild", "namespaceURI",
    "value", "append", "prepend", "remove", "focus", "blur", "click", "scrollIntoView",
    "getBoundingClientRect", "createElementNS", "createDocumentFragment", "createComment",
    "cloneNode", "isConnected", "attributes", "dataset", "createRange",
    "getRootNode", "getSelection",
    "closest",
    "data", "nodeValue", "constructor",
    "documentMode",
    "activeElement", "focus", "blur", "selectionStart", "selectionEnd", "setSelectionRange",
    "getAttributeNode", "TEXT_NODE", "ELEMENT_NODE", "COMMENT_NODE", "DOCUMENT_NODE", "DOCUMENT_FRAGMENT_NODE",
    // document own methods/props
    "createElement", "createTextNode", "getElementById", "body", "documentElement",
    // JS internals V8 / libs probe constantly — never DOM
    "then", "catch", "finally", "constructor", "prototype", "toString", "valueOf", "toJSON",
    "length", "name", "call", "apply", "bind", "hasOwnProperty", "nodeName",
];

thread_local! {
    /// debug logger: unknown property name → access count.
    static MISSING: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
}

/// Interceptor getter (debug only): fall through for known/symbol names; record + return undefined
/// for anything unimplemented.
fn missing_getter(scope: &mut v8::PinScope, name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) -> v8::Intercepted {
    // V8 inline-cache hazard with NON_MASKING interceptors: once a load site reads a property while
    // it is ABSENT (we intercept → undefined), V8 may cache "this site goes to the interceptor" and
    // keep routing there EVEN AFTER the property is later added as a real own property — masking it
    // (reads return undefined though getOwnPropertyNames/hasOwnProperty see it). React stores
    // `node.__reactFiber$…`/`__reactProps$…` exactly this way: it reads the key (miss → IC poisoned)
    // before precaching the fiber, then later reads it again and gets undefined → it walks up to the
    // wrong ancestor, so delegated onClick/onChange never fire (only after a prior render primed the
    // IC — a brutal Heisenbug). Guard: when the interceptor fires, first look for a REAL property
    // (interceptors bypassed). If one exists, return IT — so a poisoned IC still yields the truth.
    let this = args.holder();
    if let Some(real) = this.get_real_named_property(scope, name) {
        rv.set(real);
        return v8::Intercepted::kYes;
    }
    let key = name.to_rust_string_lossy(scope);
    // symbols (Symbol.toPrimitive, Symbol.iterator, …) + known names → fall through.
    if key.starts_with("Symbol(") || KNOWN.contains(&key.as_str()) {
        return v8::Intercepted::kNo;
    }
    if log_enabled() {
        MISSING.with(|m| *m.borrow_mut().entry(key).or_insert(0) += 1);
    }
    // graceful degradation: an unimplemented member reads as undefined (no crash).
    rv.set(v8::undefined(scope).into());
    v8::Intercepted::kYes
}

/// Passthrough setter interceptor: NEVER intercept (kNo) → JS property sets create normal own
/// properties. Required so `document.x = …` shims AND React's expando writes on DOM nodes
/// (`node.__reactProps$…`, `_reactListening…`) persist instead of being swallowed by the
/// getter-only interceptor.
fn passthrough_setter(_scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, _value: v8::Local<v8::Value>, _args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) -> v8::Intercepted {
    v8::Intercepted::kNo
}

/// Print + clear the accumulated missing-access log (call between files when debugging).
pub fn dump_missing() {
    let mut items: Vec<(String, u64)> = MISSING.with(|m| m.borrow_mut().drain().collect());
    if items.is_empty() {
        return;
    }
    items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    eprintln!("[rust-dom] {} missing DOM members (name×accesses):", items.len());
    for (name, n) in &items {
        eprintln!("[rust-dom]   {name} ×{n}");
    }
}

struct DomState {
    tree: Tree,
    /// handle → JS node object, for identity (`el === el`).
    cache: HashMap<Handle, v8::Global<v8::Object>>,
    /// shared element object template (internal_field_count = 1).
    el_template: v8::Global<v8::ObjectTemplate>,
}

thread_local! {
    static DOM: RefCell<Option<DomState>> = const { RefCell::new(None) };
}

/// Read a node object's handle from internal field 0.
fn handle_of(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>) -> Option<Handle> {
    let f = obj.get_internal_field(scope, 0)?;
    let v = v8::Local::<v8::Value>::try_from(f).ok()?;
    v.uint32_value(scope).map(Handle::from_raw)
}

/// Wrap a handle as a JS node object (cached for identity).
fn wrap<'s>(scope: &mut v8::PinScope<'s, '_>, handle: Handle) -> v8::Local<'s, v8::Object> {
    if let Some(g) = DOM.with(|d| d.borrow().as_ref().and_then(|s| s.cache.get(&handle).cloned())) {
        return v8::Local::new(scope, &g);
    }
    let tmpl_g = DOM.with(|d| d.borrow().as_ref().unwrap().el_template.clone());
    let tmpl = v8::Local::new(scope, &tmpl_g);
    let obj = tmpl.new_instance(scope).unwrap();
    let h = v8::Integer::new_from_unsigned(scope, handle.raw());
    obj.set_internal_field(0, h.into());
    let g = v8::Global::new(scope, obj);
    DOM.with(|d| {
        if let Some(s) = d.borrow_mut().as_mut() {
            s.cache.insert(handle, g);
        }
    });
    obj
}

/// Wrap `Option<Handle>` as object-or-null.
fn wrap_opt<'s>(scope: &mut v8::PinScope<'s, '_>, h: Option<Handle>) -> v8::Local<'s, v8::Value> {
    match h {
        Some(h) => wrap(scope, h).into(),
        None => v8::null(scope).into(),
    }
}

fn arg_str(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> String {
    args.get(i).to_rust_string_lossy(scope)
}

/// Resolve argument `i` to a node handle, or `None` for null/undefined/non-node. CRITICAL: guards
/// `to_object()`, which THROWS "Cannot convert undefined or null to object" on null/undefined — and
/// `insertBefore(node, null)` / `removeChild`-style calls pass that constantly (emotion/react-dom).
fn arg_handle(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> Option<Handle> {
    let v = args.get(i);
    if v.is_null_or_undefined() {
        return None;
    }
    v.to_object(scope).and_then(|o| handle_of(scope, o))
}

// ---- with-tree helpers (borrow the thread-local Tree) ----------------------------------------

fn with_tree<R>(f: impl FnOnce(&Tree) -> R) -> Option<R> {
    DOM.with(|d| d.borrow().as_ref().map(|s| f(&s.tree)))
}
fn with_tree_mut<R>(f: impl FnOnce(&mut Tree) -> R) -> Option<R> {
    DOM.with(|d| d.borrow_mut().as_mut().map(|s| f(&mut s.tree)))
}

// ---- element methods -------------------------------------------------------------------------

fn el_append_child(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(parent) = handle_of(scope, args.this()) else { return };
    let Some(child) = arg_handle(scope, &args, 0) else { return };
    with_tree_mut(|t| t.append_child(parent, child));
    rv.set(args.get(0));
}

fn el_remove_child(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(parent) = handle_of(scope, args.this()) else { return };
    let Some(child) = arg_handle(scope, &args, 0) else { return };
    with_tree_mut(|t| t.remove_child(parent, child));
    rv.set(args.get(0));
}

// ---- CharacterData methods (text/comment nodes) ----------------------------------------------
fn arg_usize(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> usize {
    let n = args.get(i).integer_value(scope).unwrap_or(0);
    if n < 0 { 0 } else { n as usize }
}
fn el_insert_data(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let off = arg_usize(scope, &args, 0);
    let data = arg_str(scope, &args, 1);
    with_tree_mut(|t| t.insert_data(h, off, &data));
}
fn el_delete_data(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let off = arg_usize(scope, &args, 0);
    let count = arg_usize(scope, &args, 1);
    with_tree_mut(|t| t.delete_data(h, off, count));
}
fn el_append_data(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let data = arg_str(scope, &args, 0);
    with_tree_mut(|t| t.append_data(h, &data));
}
fn el_replace_data(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let off = arg_usize(scope, &args, 0);
    let count = arg_usize(scope, &args, 1);
    let data = arg_str(scope, &args, 2);
    with_tree_mut(|t| t.replace_data(h, off, count, &data));
}
fn el_substring_data(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let off = arg_usize(scope, &args, 0);
    let count = arg_usize(scope, &args, 1);
    let s = with_tree(|t| t.substring_data(h, off, count)).unwrap_or_default();
    rv.set(v8::String::new(scope, &s).unwrap().into());
}
fn el_split_text(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let off = arg_usize(scope, &args, 0);
    let Some(new_h) = with_tree_mut(|t| t.split_text(h, off)) else { return };
    let node = wrap(scope, new_h);
    rv.set(node.into());
}

// rtdom's native style cascade (turbo-dom `cascade::computed_style`) resolves a property's computed
// value WITH inheritance for the inherited set (color, visibility, font-*, …). getComputedStyle's
// JS cascade only sees an element's OWN matched rules + inline style, so an inherited property like
// `visibility: hidden` set on an ancestor never reached descendants — testing-library then treated a
// hidden subtree as query-visible. JS getComputedStyle calls this for those inherited properties.
// (The result is version-cached on the Tree, so repeated calls within a render are cheap.)
fn el_cascade_prop(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0);
    let val = with_tree(|t| turbo_dom_parser::rtdom::cascade::computed_style(t, h).get(&name).cloned())
        .flatten()
        .unwrap_or_default();
    rv.set(v8::String::new(scope, &val).unwrap().into());
}

// Resolve a run of variadic (Node | string) arguments [start..] into handles, turning bare strings
// into text nodes (per the ChildNode/ParentNode spec). Used by before/after/replaceWith/etc.
fn resolve_nodes(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, start: i32) -> Vec<Handle> {
    let mut out = Vec::new();
    for i in start..args.length() {
        let v = args.get(i);
        if let Some(h) = v.to_object(scope).and_then(|o| handle_of(scope, o)) {
            out.push(h);
        } else if !v.is_null_or_undefined() {
            let s = v.to_rust_string_lossy(scope);
            if let Some(h) = with_tree_mut(|t| t.create_text_node(&s)) {
                out.push(h);
            }
        }
    }
    out
}
fn el_before(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let nodes = resolve_nodes(scope, &args, 0);
    with_tree_mut(|t| t.before(h, &nodes));
}
fn el_after(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let nodes = resolve_nodes(scope, &args, 0);
    with_tree_mut(|t| t.after(h, &nodes));
}
fn el_replace_with(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let nodes = resolve_nodes(scope, &args, 0);
    with_tree_mut(|t| t.replace_with(h, &nodes));
}
fn el_replace_children(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let nodes = resolve_nodes(scope, &args, 0);
    with_tree_mut(|t| t.replace_children(h, &nodes));
}
fn el_insert_adjacent_element(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let pos = arg_str(scope, &args, 0);
    let Some(el) = arg_handle(scope, &args, 1) else { return };
    let ok = with_tree_mut(|t| t.insert_adjacent_element(h, &pos, el)).unwrap_or(false);
    if ok { rv.set(args.get(1)); } else { rv.set(v8::null(scope).into()); }
}
fn el_insert_adjacent_html(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let pos = arg_str(scope, &args, 0);
    let html = arg_str(scope, &args, 1);
    with_tree_mut(|t| t.insert_adjacent_html(h, &pos, &html));
}
fn el_toggle_attribute(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0).to_ascii_lowercase();
    let force = if args.length() > 1 && !args.get(1).is_undefined() {
        Some(args.get(1).boolean_value(scope))
    } else { None };
    let present = with_tree_mut(|t| t.toggle_attribute(h, &name, force)).unwrap_or(false);
    rv.set(v8::Boolean::new(scope, present).into());
}
fn el_get_attribute_ns(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let local = arg_str(scope, &args, 1);
    match with_tree(|t| t.get_attribute_ns(h, None, &local)).flatten() {
        Some(s) => rv.set(v8::String::new(scope, &s).unwrap().into()),
        None => rv.set(v8::null(scope).into()),
    }
}

fn el_insert_before(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(parent) = handle_of(scope, args.this()) else { return };
    let Some(child) = arg_handle(scope, &args, 0) else { return };
    let reference = arg_handle(scope, &args, 1);
    with_tree_mut(|t| t.insert_before(parent, child, reference));
    rv.set(args.get(0));
}

fn el_set_attribute(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0);
    let value = arg_str(scope, &args, 1);
    // HTML setAttribute lowercases the qualified name (React calls setAttribute('colSpan', …); jsdom
    // stores 'colspan'). Lowercase for HTML-namespace elements only (ns 0); SVG (ns 1) keeps camelCase
    // attrs like viewBox.
    let is_html = with_tree(|t| t.namespace_id(h) == 0).unwrap_or(true);
    let key = if is_html { name.to_ascii_lowercase() } else { name.clone() };
    with_tree_mut(|t| t.set_attribute(h, &key, &value));

    // When an <input> becomes a checkbox/radio (React sets `type` via setAttribute after createElement),
    // strip the value-change tracker: those inputs track `checked`, not `value`, so a value-tracker
    // would mis-detect changes and break onChange. Removing the own `value`/`_valueTracker` lets the
    // framework use its checked path (we keep the own `checked` accessor).
    if key == "type" {
        let v = value.to_ascii_lowercase();
        if v == "checkbox" || v == "radio" {
            let this = args.this();
            for k in ["value", "_valueTracker"] {
                if let Some(key) = v8::String::new(scope, k) {
                    if this.has_own_property(scope, key.into()).unwrap_or(false) {
                        this.delete(scope, key.into());
                    }
                }
            }
        }
    }
}

/// Attribute lookup with the HTML case-insensitivity rule: try the exact name, then its lowercase
/// form (React writes `tabindex`/`maxlength` lowercased; tests read `tabIndex`/`maxLength`). Exact-
/// first preserves SVG camelCase attributes (`viewBox`).
fn attr_get(t: &Tree, h: Handle, name: &str) -> Option<String> {
    if let Some(v) = t.get_attribute(h, name) { return Some(v.to_string()); }
    let lower = name.to_ascii_lowercase();
    if lower != name { if let Some(v) = t.get_attribute(h, &lower) { return Some(v.to_string()); } }
    None
}

fn el_get_attribute(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0);
    let val = with_tree(|t| attr_get(t, h, &name)).flatten();
    match val {
        Some(s) => rv.set(v8::String::new(scope, &s).unwrap().into()),
        None => rv.set(v8::null(scope).into()),
    }
}

fn el_get_attribute_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0);
    let val = with_tree(|t| attr_get(t, h, &name)).flatten();
    match val {
        Some(v) => {
            let o = v8::Object::new(scope);
            for (pk, pv) in [("name", name.as_str()), ("nodeName", name.as_str()), ("localName", name.as_str()), ("value", v.as_str()), ("nodeValue", v.as_str())] {
                let key = v8::String::new(scope, pk).unwrap();
                let s = v8::String::new(scope, pv).unwrap();
                o.set(scope, key.into(), s.into());
            }
            let sk = v8::String::new(scope, "specified").unwrap();
            let st = v8::Boolean::new(scope, true);
            o.set(scope, sk.into(), st.into());
            rv.set(o.into());
        }
        None => rv.set(v8::null(scope).into()),
    }
}

// setAttributeNode(attr) / removeAttributeNode(attr) — operate on an Attr node (the plain object
// el_get_attribute_node returns: { name, value, … }). React's releaseSingletonInstance resets the
// <html>/<head>/<body> singletons during commit via instance.removeAttributeNode(attributes[i]);
// without these it throws "removeAttributeNode is not a function" and aborts the commit.
fn attr_name_value(scope: &mut v8::PinScope, v: v8::Local<v8::Value>) -> Option<(String, String)> {
    let o = v.to_object(scope)?;
    let name = get_str_prop(scope, o, "name").or_else(|| get_str_prop(scope, o, "nodeName"))?;
    let value = get_str_prop(scope, o, "value").or_else(|| get_str_prop(scope, o, "nodeValue")).unwrap_or_default();
    if name.is_empty() { None } else { Some((name, value)) }
}
fn el_set_attribute_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    if let Some((name, value)) = attr_name_value(scope, args.get(0)) {
        let is_html = with_tree(|t| t.namespace_id(h) == 0).unwrap_or(true);
        let key = if is_html { name.to_ascii_lowercase() } else { name };
        with_tree_mut(|t| t.set_attribute(h, &key, &value));
    }
    rv.set(v8::null(scope).into()); // replaced Attr (none modeled) → null
}
fn el_remove_attribute_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    if let Some((name, _)) = attr_name_value(scope, args.get(0)) {
        with_tree_mut(|t| t.remove_attribute(h, &name));
        let lower = name.to_ascii_lowercase();
        if lower != name { with_tree_mut(|t| t.remove_attribute(h, &lower)); }
    }
    rv.set(args.get(0)); // spec returns the removed Attr
}

fn el_has_attribute(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0);
    let lower = name.to_ascii_lowercase();
    let has = with_tree(|t| t.has_attribute(h, &name) || (lower != name && t.has_attribute(h, &lower))).unwrap_or(false);
    rv.set(v8::Boolean::new(scope, has).into());
}

fn el_remove_attribute(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let name = arg_str(scope, &args, 0);
    let lower = name.to_ascii_lowercase();
    with_tree_mut(|t| { t.remove_attribute(h, &name); if lower != name { t.remove_attribute(h, &lower); } });
}

/// element-scoped querySelector via NodeRef.
fn el_query_selector(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let sel = arg_str(scope, &args, 0);
    let found = with_tree(|t| NodeRef::new(t, h).query_selector(&sel).map(|n| n.handle())).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

// ---- innerHTML / outerHTML / children / form props / classList (P3.3 batch) -----------------

fn get_inner_html(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let html = with_tree(|t| turbo_dom_parser::rtdom::serialize::serialize_inner(t, h)).unwrap_or_default();
    rv.set(v8::String::new(scope, &html).unwrap().into());
}
fn set_inner_html(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let html = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_inner_html(h, &html));
}
fn get_outer_html(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let html = with_tree(|t| turbo_dom_parser::rtdom::serialize::serialize_outer(t, h)).unwrap_or_default();
    rv.set(v8::String::new(scope, &html).unwrap().into());
}

/// `children` = element-only child nodes (HTMLCollection-ish array).
fn get_children(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let kids: Vec<Handle> = with_tree(|t| {
        t.children(h).into_iter().filter(|&c| t.node_type_id(c) == 1).collect()
    }).unwrap_or_default();
    let arr = v8::Array::new(scope, kids.len() as i32);
    for (i, k) in kids.into_iter().enumerate() {
        let node = wrap(scope, k);
        arr.set_index(scope, i as u32, node.into());
    }
    bind_method(scope, arr.into(), "item", nodelist_item);
    rv.set(arr.into());
}

fn get_parent_element(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let p = with_tree(|t| NodeRef::new(t, h).parent().map(|x| x.handle()).filter(|&ph| t.node_type_id(ph) == 1)).flatten();
    let v = wrap_opt(scope, p);
    rv.set(v);
}

fn get_first_element_child(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let c = with_tree(|t| NodeRef::new(t, h).first_element_child().map(|x| x.handle())).flatten();
    let v = wrap_opt(scope, c);
    rv.set(v);
}

fn get_namespace_uri(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, _args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    rv.set(v8::String::new(scope, "http://www.w3.org/1999/xhtml").unwrap().into());
}

/// `value`/`checked`/`disabled` etc — stored on the element as expandos by default (React's
/// controlled inputs set them directly); fall back to the attribute for `value`.
fn get_value(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let v = with_tree(|t| t.get_attribute(h, "value").map(|s| s.to_string())).flatten().unwrap_or_default();
    rv.set(v8::String::new(scope, &v).unwrap().into());
}
fn set_value(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let v = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_attribute(h, "value", &v));
}

// no-op / stub element methods commonly called during render + queries.
thread_local! {
    /// the focused element handle (document.activeElement). userEvent.type targets it.
    static ACTIVE: RefCell<Option<Handle>> = const { RefCell::new(None) };
}
/// Native `HTMLElement.focus()` — like a real browser / jsdom: blur the previously-focused element
/// (blur + bubbling focusout), make this the activeElement, then fire focus + bubbling focusin. The
/// bubbling focusout/focusin reach React's delegated onBlur/onFocus at the root container. userEvent
/// drives blur-on-tab purely through native focus(), so this must dispatch the events itself.
fn el_focus(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let prev = ACTIVE.with(|a| *a.borrow());
    if prev == Some(h) { return; }
    ACTIVE.with(|a| *a.borrow_mut() = Some(h));
    // Only blur a previously-focused element that is still attached. A detached `prev` is a stale
    // pointer left by a prior, now-unmounted tree (testing-library cleanup); dispatching blur/focusout
    // on it walks a detached subtree and would corrupt the focus transition into the new element.
    if let Some(p) = prev.filter(|&p| with_tree(|t| is_connected(t, p)).unwrap_or(false)) {
        dispatch_synthetic(scope, p, "blur", false, false);
        dispatch_synthetic(scope, p, "focusout", true, false);
    }
    dispatch_synthetic(scope, h, "focus", false, false);
    dispatch_synthetic(scope, h, "focusin", true, false);
}
fn el_blur(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    if ACTIVE.with(|a| *a.borrow()) != Some(h) { return; }
    ACTIVE.with(|a| *a.borrow_mut() = None);
    dispatch_synthetic(scope, h, "blur", false, false);
    dispatch_synthetic(scope, h, "focusout", true, false);
}
/// `document.activeElement` → the focused element, or `<body>`.
fn get_active_element(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, _args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    // Real DOM resets focus to <body> when the active element is removed from the tree. If our
    // tracked handle is detached (e.g. the focused input of a prior, now-unmounted React tree —
    // testing-library cleanup leaves the JS ACTIVE pointer dangling), drop it so the stale node
    // never leaks into the next test's focus/userEvent logic.
    let active = ACTIVE.with(|a| *a.borrow());
    let active = active.filter(|&h| with_tree(|t| is_connected(t, h)).unwrap_or(false));
    if active.is_none() {
        ACTIVE.with(|a| *a.borrow_mut() = None);
    }
    let h = active.or_else(|| with_tree(|t| t.query_selector("body")).flatten());
    if let Some(h) = h {
        let node = wrap(scope, h);
        rv.set(node.into());
    }
}
/// Native `HTMLElement.click()` — dispatch a bubbling, cancelable click event so React's delegated
/// onClick at the root container fires. Tests call `el.click()` directly (no userEvent).
fn el_click(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    dispatch_synthetic(scope, h, "click", true, true);
}
fn el_scroll_into_view(_scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {}
fn el_get_bounding_client_rect(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let o = v8::Object::new(scope);
    for k in ["x", "y", "top", "left", "right", "bottom", "width", "height"] {
        let key = v8::String::new(scope, k).unwrap();
        let zero = v8::Number::new(scope, 0.0);
        o.set(scope, key.into(), zero.into());
    }
    rv.set(o.into());
}

/// `append(...nodes)` / `prepend(...)` / `remove()` (DOM ChildNode/ParentNode mixins).
fn el_append(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(parent) = handle_of(scope, args.this()) else { return };
    for i in 0..args.length() {
        if let Some(child) = arg_handle(scope, &args, i) {
            with_tree_mut(|t| t.append_child(parent, child));
        }
    }
}
fn el_remove_self(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let parent = with_tree(|t| NodeRef::new(t, h).parent().map(|p| p.handle())).flatten();
    if let Some(p) = parent {
        with_tree_mut(|t| t.remove_child(p, h));
    }
}

// document native methods (reliable — no JS-bootstrap dependency).
fn doc_create_element_ns(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let ns = arg_str(scope, &args, 0).to_ascii_lowercase();
    let tag = arg_str(scope, &args, 1);
    // Map the namespace URI to rtdom's ns id (svg=1, math=2, html=0) so SVG/MathML elements keep
    // their namespace + case-preserved attributes (e.g. `viewBox`), unlike HTML which lowercases.
    let ns_id: u8 = if ns.contains("svg") { 1 } else if ns.contains("mathml") || ns.contains("math") { 2 } else { 0 };
    let made = with_tree_mut(|t| if ns_id == 0 { t.create_element(&tag) } else { t.create_element_ns(&tag, Namespace::from_id(ns_id)) });
    if let Some(h) = made {
        let node = wrap(scope, h);
        rv.set(node.into());
    }
}
fn doc_create_fragment(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    if let Some(h) = with_tree_mut(|t| t.create_element("#document-fragment")) {
        let node = wrap(scope, h);
        rv.set(node.into());
    }
}
fn doc_create_comment(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let data = arg_str(scope, &args, 0);
    if let Some(h) = with_tree_mut(|t| t.create_comment(&data)) {
        let node = wrap(scope, h);
        rv.set(node.into());
    }
}

/// recursively clone a subtree, returning the new root handle.
fn clone_subtree(t: &mut Tree, h: Handle, deep: bool) -> Handle {
    let nt = t.node_type_id(h);
    let new_h = match nt {
        3 => t.create_text_node(&t.node_value(h).unwrap_or_default()),
        8 => t.create_comment(&t.node_value(h).unwrap_or_default()),
        _ => {
            let tag = t.tag_name(h).unwrap_or_else(|| "div".into());
            let e = t.create_element(&tag);
            for (k, v) in t.attributes(h) {
                t.set_attribute(e, &k, &v);
            }
            e
        }
    };
    if deep && nt != 3 && nt != 8 {
        for c in t.children(h) {
            let cc = clone_subtree(t, c, true);
            t.append_child(new_h, cc);
        }
    }
    new_h
}

/// `getRootNode()` → the topmost ancestor (the document root if connected). jest-dom's
/// `toBeInTheDocument` compares `ownerDocument === getRootNode()`.
fn el_get_root_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let root = with_tree(|t| {
        let mut cur = h;
        while let Some(p) = NodeRef::new(t, cur).parent() {
            cur = p.handle();
        }
        cur
    });
    if let Some(r) = root {
        let node = wrap(scope, r);
        rv.set(node.into());
    }
}

fn el_clone_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let deep = args.get(0).boolean_value(scope);
    if let Some(new_h) = with_tree_mut(|t| clone_subtree(t, h, deep)) {
        let node = wrap(scope, new_h);
        // Carry over the source's JS prototype so a cloned control element keeps its interface
        // accessors (type/value/checked/selected). createElement applies these protos, but cloneNode
        // bypasses it; without this a cloned <input type=date> loses date-value validation, which
        // userEvent's isValidDateOrTimeValue (clone, assign, check it stuck) relies on.
        let src_proto = args.this().get_prototype(scope);
        if let Some(p) = src_proto {
            node.set_prototype(scope, p);
        }
        rv.set(node.into());
    }
}

/// whether `h`'s root (walking parents) is the document root → attached to the document.
fn is_connected(t: &Tree, h: Handle) -> bool {
    let root = t.root();
    let mut cur = Some(h);
    while let Some(c) = cur {
        if c == root {
            return true;
        }
        cur = NodeRef::new(t, c).parent().map(|p| p.handle());
    }
    false
}

fn get_is_connected(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let c = with_tree(|t| is_connected(t, h)).unwrap_or(false);
    rv.set(v8::Boolean::new(scope, c).into());
}

/// `parent.contains(node)` — true if node === parent or a descendant.
fn el_contains_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(p) = handle_of(scope, args.this()) else { return };
    let other = arg_handle(scope, &args, 0);
    let yes = match other {
        Some(o) => with_tree(|t| {
            let mut cur = Some(o);
            while let Some(c) = cur {
                if c == p { return true; }
                cur = NodeRef::new(t, c).parent().map(|x| x.handle());
            }
            false
        }).unwrap_or(false),
        None => false,
    };
    rv.set(v8::Boolean::new(scope, yes).into());
}

/// `attributes` → array of `{ name, value, nodeName, nodeValue }` (NamedNodeMap-lite, indexable).
fn get_attributes(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let attrs = with_tree(|t| t.attributes(h)).unwrap_or_default();
    let arr = v8::Array::new(scope, attrs.len() as i32);
    for (i, (k, v)) in attrs.into_iter().enumerate() {
        let o = v8::Object::new(scope);
        for (pk, pv) in [("name", k.as_str()), ("nodeName", k.as_str()), ("value", v.as_str()), ("nodeValue", v.as_str())] {
            let key = v8::String::new(scope, pk).unwrap();
            let val = v8::String::new(scope, pv).unwrap();
            o.set(scope, key.into(), val.into());
        }
        arr.set_index(scope, i as u32, o.into());
    }
    rv.set(arr.into());
}

/// `dataset` → `{ camelCaseKey: value }` for each `data-*` attribute.
fn get_dataset(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let attrs = with_tree(|t| t.attributes(h)).unwrap_or_default();
    let o = v8::Object::new(scope);
    for (k, v) in attrs {
        if let Some(rest) = k.strip_prefix("data-") {
            // kebab → camelCase
            let mut camel = String::new();
            let mut up = false;
            for ch in rest.chars() {
                if ch == '-' { up = true; }
                else if up { camel.extend(ch.to_uppercase()); up = false; }
                else { camel.push(ch); }
            }
            let key = v8::String::new(scope, &camel).unwrap();
            let val = v8::String::new(scope, &v).unwrap();
            o.set(scope, key.into(), val.into());
        }
    }
    rv.set(o.into());
}

/// `closest(selector)` → nearest ancestor (incl. self) that matches, or null.
fn el_closest(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let sel = arg_str(scope, &args, 0);
    let found = with_tree(|t| {
        let mut cur = Some(h);
        while let Some(c) = cur {
            if t.node_type_id(c) == 1 && t.matches(c, &sel) {
                return Some(c);
            }
            cur = NodeRef::new(t, c).parent().map(|p| p.handle());
        }
        None
    }).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

fn el_matches(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let sel = arg_str(scope, &args, 0);
    let m = with_tree(|t| t.matches(h, &sel)).unwrap_or(false);
    rv.set(v8::Boolean::new(scope, m).into());
}

fn el_contains(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let other = arg_handle(scope, &args, 0);
    let contains = match other {
        Some(o) => with_tree(|t| {
            // walk up from `o` to see if `h` is an ancestor (or equal).
            let mut cur = Some(o);
            while let Some(c) = cur {
                if c == h { return true; }
                cur = NodeRef::new(t, c).parent().map(|p| p.handle());
            }
            false
        }).unwrap_or(false),
        None => false,
    };
    rv.set(v8::Boolean::new(scope, contains).into());
}

fn el_query_selector_all(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let sel = arg_str(scope, &args, 0);
    let handles = with_tree(|t| {
        NodeRef::new(t, h).query_selector_all(&sel).into_iter().map(|n| n.handle()).collect::<Vec<_>>()
    })
    .unwrap_or_default();
    let arr = v8::Array::new(scope, handles.len() as i32);
    for (i, hh) in handles.into_iter().enumerate() {
        let node = wrap(scope, hh);
        arr.set_index(scope, i as u32, node.into());
    }
    bind_method(scope, arr.into(), "item", nodelist_item);
    rv.set(arr.into());
}

// ---- element accessors -----------------------------------------------------------------------
// Getter sig: (scope, name, PropertyCallbackArguments, ReturnValue<Value>).
// Setter sig: (scope, name, Local<Value>, PropertyCallbackArguments, ReturnValue<()>).

fn get_tag_name(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let tag = with_tree(|t| t.tag_name(h)).flatten().unwrap_or_default().to_uppercase();
    rv.set(v8::String::new(scope, &tag).unwrap().into());
}

fn get_text_content(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let txt = with_tree(|t| t.text_content(h)).unwrap_or_default();
    rv.set(v8::String::new(scope, &txt).unwrap().into());
}

fn set_text_content(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let txt = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_text_content(h, &txt));
}

fn get_id(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let id = with_tree(|t| t.get_attribute(h, "id").map(|s| s.to_string())).flatten().unwrap_or_default();
    rv.set(v8::String::new(scope, &id).unwrap().into());
}

fn set_id(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let v = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_attribute(h, "id", &v));
}

fn get_class_name(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let c = with_tree(|t| t.get_attribute(h, "class").map(|s| s.to_string())).flatten().unwrap_or_default();
    rv.set(v8::String::new(scope, &c).unwrap().into());
}

fn set_class_name(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let v = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_attribute(h, "class", &v));
}

// `element.contentEditable` IDL property — reflects the `contenteditable` content attribute as the
// enumerated string 'true' | 'false' | 'plaintext-only' | 'inherit'. Lexical reads this property
// (NOT the attribute) to decide the root is editable and to wire up its beforeinput pipeline; with
// it missing (undefined) Lexical treats the editor as non-editable and never emits onChange.
fn get_content_editable(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let attr = with_tree(|t| attr_get(t, h, "contenteditable")).flatten();
    let out = match attr.as_deref() {
        Some("") | Some("true") => "true",
        Some("false") => "false",
        Some("plaintext-only") => "plaintext-only",
        _ => "inherit",
    };
    rv.set(v8::String::new(scope, out).unwrap().into());
}

fn set_content_editable(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let v = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_attribute(h, "contenteditable", &v));
}

// `element.isContentEditable` — true when the element itself is editable (contenteditable=""|"true")
// or it inherits editability from an ancestor.
fn get_is_content_editable(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(mut h) = handle_of(scope, args.holder()) else { return };
    let mut editable = false;
    loop {
        match with_tree(|t| attr_get(t, h, "contenteditable")).flatten().as_deref() {
            Some("") | Some("true") | Some("plaintext-only") => { editable = true; break; }
            Some("false") => { editable = false; break; }
            _ => {}
        }
        match with_tree(|t| NodeRef::new(t, h).parent().map(|p| p.handle()).filter(|&ph| t.node_type_id(ph) == 1)).flatten() {
            Some(p) => h = p,
            None => break,
        }
    }
    rv.set(v8::Boolean::new(scope, editable).into());
}

fn get_parent_node(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let found = with_tree(|t| NodeRef::new(t, h).parent().map(|x| x.handle())).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

fn get_first_child(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let found = with_tree(|t| NodeRef::new(t, h).first_child().map(|x| x.handle())).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

fn get_next_sibling(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let found = with_tree(|t| NodeRef::new(t, h).next_sibling().map(|x| x.handle())).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

// ---- events: real dispatch (addEventListener/removeEventListener/dispatchEvent) --------------
// React attaches one delegated listener at the root container and relies on bubbling; fireEvent /
// userEvent call `node.dispatchEvent(new Event(type, {bubbles:true}))`. So we store JS listeners
// per (handle, type) and walk target→ancestors firing them, with target/currentTarget set on the
// event object (the `Event` class lives in the JS bootstrap).

thread_local! {
    /// (node handle, event type) → registered JS listeners (function, capture-phase flag).
    static LISTENERS: RefCell<HashMap<(Handle, String), Vec<(v8::Global<v8::Function>, bool)>>> = RefCell::new(HashMap::new());
}

/// useCapture from the 3rd addEventListener arg (boolean, or `{capture: true}` options object).
fn arg_capture(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments) -> bool {
    let v = args.get(2);
    if v.is_boolean() {
        return v.boolean_value(scope);
    }
    // 2-arg addEventListener leaves arg 2 undefined; to_object() on null/undefined THROWS
    // ("Cannot convert undefined or null to object"). Guard before converting.
    if v.is_null_or_undefined() {
        return false;
    }
    if let Some(o) = v.to_object(scope) {
        if let Some(k) = v8::String::new(scope, "capture") {
            if let Some(c) = o.get(scope, k.into()) {
                return c.boolean_value(scope);
            }
        }
    }
    false
}

fn el_add_event_listener(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let ty = arg_str(scope, &args, 0);
    let Some(f) = v8::Local::<v8::Function>::try_from(args.get(1)).ok() else { return };
    let capture = arg_capture(scope, &args);
    let g = v8::Global::new(scope, f);
    LISTENERS.with(|m| m.borrow_mut().entry((h, ty)).or_default().push((g, capture)));
}

fn el_remove_event_listener(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let Some(h) = handle_of(scope, args.this()) else { return };
    let ty = arg_str(scope, &args, 0);
    let Some(f) = v8::Local::<v8::Function>::try_from(args.get(1)).ok() else { return };
    LISTENERS.with(|m| {
        if let Some(v) = m.borrow_mut().get_mut(&(h, ty)) {
            v.retain(|(g, _)| !v8::Local::new(scope, g).eq(&f));
        }
    });
}

fn el_dispatch_event(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(target) = handle_of(scope, args.this()) else {
        rv.set(v8::Boolean::new(scope, true).into());
        return;
    };
    let event = args.get(0);
    if event.is_null_or_undefined() { return; }
    let Some(event_obj) = event.to_object(scope) else { return };
    let ty = get_str_prop(scope, event_obj, "type").unwrap_or_default();
    let bubbles = get_bool_prop(scope, event_obj, "bubbles");

    // event.target = the dispatching node.
    let target_node = wrap(scope, target);
    set_prop(scope, event_obj, "target", target_node.into());

    // PRE-CLICK ACTIVATION: a checkbox/radio toggles `checked` BEFORE the click listeners run (per
    // the activation-behavior spec), so React's ChangeEventPlugin — which detects checkbox change on
    // the `click` event by reading node.checked — sees the new value. Reverted below if the click is
    // canceled. `change`/`input` DOM events fire afterward (also below).
    let click_check = if ty == "click" { checkable_kind(target) } else { None };
    let check_was = click_check.map(|_| get_bool_prop(scope, target_node, "checked"));
    if let (Some(kind), Some(was)) = (click_check, check_was) {
        let next = if kind == CheckKind::Radio { true } else { !was };
        let b = v8::Boolean::new(scope, next);
        set_prop(scope, target_node, "checked", b.into());
    }

    // propagation path: ancestors (root … parent), then the target. Bubbling reverses it.
    let mut ancestors = Vec::new();
    let mut cur = with_tree(|t| NodeRef::new(t, target).parent().map(|p| p.handle())).flatten();
    while let Some(c) = cur {
        ancestors.push(c);
        cur = with_tree(|t| NodeRef::new(t, c).parent().map(|p| p.handle())).flatten();
    }

    // CAPTURE phase: root → target's parent (capture listeners), then TARGET (both), then BUBBLE:
    // target's parent → root (non-capture listeners). React 18 registers some delegated root
    // listeners in capture phase, so a bubble-only dispatch would miss them.
    let mut capture_path: Vec<Handle> = ancestors.iter().rev().copied().collect();
    let bubble_path: Vec<Handle> = if bubbles { ancestors.clone() } else { Vec::new() };
    capture_path.push(target); // target fires in both phases (handled by the at_target flag below)

    let fire = |scope: &mut v8::PinScope, node_h: Handle, want_capture: bool, at_target: bool| -> bool {
        if get_bool_prop(scope, event_obj, "__stop") {
            return false;
        }
        let listeners: Vec<(v8::Global<v8::Function>, bool)> =
            LISTENERS.with(|m| m.borrow().get(&(node_h, ty.clone())).cloned().unwrap_or_default());
        if listeners.is_empty() {
            return true;
        }
        let cur_node = wrap(scope, node_h);
        set_prop(scope, event_obj, "currentTarget", cur_node.into());
        for (g, capture) in listeners {
            // at the target, both capture and bubble listeners fire; elsewhere only the phase's.
            if !at_target && capture != want_capture {
                continue;
            }
            if get_bool_prop(scope, event_obj, "__stopImmediate") {
                break;
            }
            let f = v8::Local::new(scope, &g);
            let recv: v8::Local<v8::Value> = cur_node.into();
            f.call(scope, recv, &[event]);
        }
        true
    };

    // capture: root → parent
    for &node_h in capture_path.iter().take(capture_path.len().saturating_sub(1)) {
        if !fire(scope, node_h, true, false) { break; }
    }
    // at target (both phases)
    fire(scope, target, false, true);
    // bubble: parent → root
    for node_h in bubble_path {
        if !fire(scope, node_h, false, false) { break; }
    }
    let not_prevented = !get_bool_prop(scope, event_obj, "defaultPrevented");

    // DEFAULT ACTION: a non-prevented click on a submit control fires the form's `submit` event
    // (jsdom does this natively; userEvent/fireEvent.click rely on it for form submission).
    if not_prevented && ty == "click" && is_submit_control(target) {
        if let Some(form) = closest_form(target) {
            dispatch_synthetic(scope, form, "submit", true, true);
        }
    }

    // Finish the checkbox/radio activation: a canceled click reverts the pre-toggle; otherwise fire
    // the DOM `input` + `change` events (jsdom does this; some libs listen for them directly).
    if let (Some(_kind), Some(was)) = (click_check, check_was) {
        if not_prevented {
            dispatch_synthetic(scope, target, "input", true, false);
            dispatch_synthetic(scope, target, "change", true, false);
        } else {
            let el = wrap(scope, target);
            let b = v8::Boolean::new(scope, was);
            set_prop(scope, el, "checked", b.into());
        }
    }

    rv.set(v8::Boolean::new(scope, not_prevented).into());
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum CheckKind { Checkbox, Radio }

/// `<input type=checkbox|radio>` → its kind, else None (drives the click toggle default action).
fn checkable_kind(h: Handle) -> Option<CheckKind> {
    with_tree(|t| {
        if t.node_type_id(h) != 1 || t.tag_name(h).unwrap_or_default().to_ascii_uppercase() != "INPUT" {
            return None;
        }
        match t.get_attribute(h, "type").map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("checkbox") => Some(CheckKind::Checkbox),
            Some("radio") => Some(CheckKind::Radio),
            _ => None,
        }
    })
    .flatten()
}

/// A `<button>` (type submit or unset — submit is the default) or `<input type=submit>`.
fn is_submit_control(h: Handle) -> bool {
    with_tree(|t| {
        if t.node_type_id(h) != 1 {
            return false;
        }
        let tag = t.tag_name(h).unwrap_or_default().to_ascii_uppercase();
        let ty = t.get_attribute(h, "type").map(|s| s.to_ascii_lowercase());
        match tag.as_str() {
            "BUTTON" => ty.as_deref().map(|t| t == "submit").unwrap_or(true),
            "INPUT" => ty.as_deref() == Some("submit"),
            _ => false,
        }
    })
    .unwrap_or(false)
}

/// nearest ancestor `<form>` (incl. self).
fn closest_form(h: Handle) -> Option<Handle> {
    with_tree(|t| {
        let mut cur = Some(h);
        while let Some(c) = cur {
            if t.tag_name(c).map(|s| s.eq_ignore_ascii_case("form")).unwrap_or(false) {
                return Some(c);
            }
            cur = NodeRef::new(t, c).parent().map(|p| p.handle());
        }
        None
    })
    .flatten()
}

/// Construct `new Event(type, {bubbles, cancelable})` and dispatch it on `target` via its native
/// dispatchEvent (re-enters the event system → fires listeners).
fn dispatch_synthetic(scope: &mut v8::PinScope, target: Handle, ty: &str, bubbles: bool, cancelable: bool) {
    let global = scope.get_current_context().global(scope);
    let Some(ctor) = v8::String::new(scope, "Event")
        .and_then(|k| global.get(scope, k.into()))
        .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok())
    else { return };
    let init = v8::Object::new(scope);
    for (k, val) in [("bubbles", bubbles), ("cancelable", cancelable)] {
        if let Some(key) = v8::String::new(scope, k) {
            let b = v8::Boolean::new(scope, val);
            init.set(scope, key.into(), b.into());
        }
    }
    let ty_str = v8::String::new(scope, ty).unwrap();
    let Some(ev) = ctor.new_instance(scope, &[ty_str.into(), init.into()]) else { return };
    let node = wrap(scope, target);
    if let Some(de) = v8::String::new(scope, "dispatchEvent")
        .and_then(|k| node.get(scope, k.into()))
        .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok())
    {
        let recv: v8::Local<v8::Value> = node.into();
        de.call(scope, recv, &[ev.into()]);
    }
}

// small prop helpers
fn get_str_prop(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str) -> Option<String> {
    let key = v8::String::new(scope, name)?;
    let v = obj.get(scope, key.into())?;
    if v.is_string() { Some(v.to_rust_string_lossy(scope)) } else { None }
}
fn get_bool_prop(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str) -> bool {
    v8::String::new(scope, name)
        .and_then(|k| obj.get(scope, k.into()))
        .map(|v| v.boolean_value(scope))
        .unwrap_or(false)
}
fn set_prop(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str, val: v8::Local<v8::Value>) {
    if let Some(k) = v8::String::new(scope, name) {
        obj.set(scope, k.into(), val);
    }
}

/// `constructor` → the named global DOM constructor for this node's type, so libraries keying on
/// `node.constructor.name` (pretty-format's DOMElementFilter regex `/^((HTML|SVG)\w*)?Element$/`)
/// treat it as a DOM node, not a plain Object they'd recurse into and crash on.
fn get_constructor(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let (nt, tag) = with_tree(|t| (t.node_type_id(h), t.tag_name(h))).unwrap_or((1, None));
    let cname = match nt {
        3 => "Text",
        8 => "Comment",
        9 => "HTMLDocument",
        11 => "DocumentFragment",
        1 => match tag.as_deref().map(|s| s.to_ascii_uppercase()).as_deref() {
            Some("INPUT") => "HTMLInputElement",
            Some("TEXTAREA") => "HTMLTextAreaElement",
            Some("SELECT") => "HTMLSelectElement",
            Some("OPTION") => "HTMLOptionElement",
            // Extra elements whose ctor globals the bootstrap registers — keep `.constructor.name`
            // specific (not generic HTMLElement) so libs keying on it resolve the right interface.
            Some("DIALOG") => "HTMLDialogElement",
            Some("DATALIST") => "HTMLDataListElement",
            Some("FIELDSET") => "HTMLFieldSetElement",
            Some("LEGEND") => "HTMLLegendElement",
            Some("OL") => "HTMLOListElement",
            Some("DL") => "HTMLDListElement",
            Some("PRE") => "HTMLPreElement",
            Some("TR") => "HTMLTableRowElement",
            Some("TD") | Some("TH") => "HTMLTableCellElement",
            Some("THEAD") | Some("TBODY") | Some("TFOOT") => "HTMLTableSectionElement",
            Some("COL") | Some("COLGROUP") => "HTMLTableColElement",
            Some("CAPTION") => "HTMLTableCaptionElement",
            Some("PROGRESS") => "HTMLProgressElement",
            Some("METER") => "HTMLMeterElement",
            Some("DETAILS") => "HTMLDetailsElement",
            Some("PICTURE") => "HTMLPictureElement",
            Some("SOURCE") => "HTMLSourceElement",
            Some("VIDEO") => "HTMLVideoElement",
            Some("AUDIO") => "HTMLAudioElement",
            Some("TEMPLATE") => "HTMLTemplateElement",
            Some("SLOT") => "HTMLSlotElement",
            Some("BODY") => "HTMLBodyElement",
            Some("HTML") => "HTMLHtmlElement",
            Some("HEAD") => "HTMLHeadElement",
            Some("META") => "HTMLMetaElement",
            Some("LINK") => "HTMLLinkElement",
            Some("TITLE") => "HTMLTitleElement",
            Some("BASE") => "HTMLBaseElement",
            Some("BR") => "HTMLBRElement",
            Some("HR") => "HTMLHRElement",
            Some("OPTGROUP") => "HTMLOptGroupElement",
            Some("MAP") => "HTMLMapElement",
            Some("AREA") => "HTMLAreaElement",
            Some("OBJECT") => "HTMLObjectElement",
            Some("EMBED") => "HTMLEmbedElement",
            Some("OUTPUT") => "HTMLOutputElement",
            Some("MENU") => "HTMLMenuElement",
            Some("DATA") => "HTMLDataElement",
            Some("TIME") => "HTMLTimeElement",
            Some("Q") | Some("BLOCKQUOTE") => "HTMLQuoteElement",
            _ => "HTMLElement",
        },
        _ => "HTMLElement",
    };
    let global = scope.get_current_context().global(scope);
    if let Some(key) = v8::String::new(scope, cname) {
        if let Some(ctor) = global.get(scope, key.into()) {
            if !ctor.is_undefined() {
                rv.set(ctor);
            }
        }
    }
}

fn get_node_type(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let nt = with_tree(|t| t.node_type_id(h)).unwrap_or(1);
    rv.set(v8::Integer::new(scope, nt as i32).into());
}

/// `data` / `nodeValue` → text of a text/comment node (null for elements). Serializers call
/// `.replace` on these, so they must be strings (not undefined) for text nodes.
fn get_node_data(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let nt = with_tree(|t| t.node_type_id(h)).unwrap_or(1);
    if nt == 3 || nt == 8 {
        let data = with_tree(|t| t.node_value(h)).flatten().unwrap_or_default();
        rv.set(v8::String::new(scope, &data).unwrap().into());
    } else {
        rv.set(v8::null(scope).into());
    }
}
fn set_node_data(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, value: v8::Local<v8::Value>, args: v8::PropertyCallbackArguments, _rv: v8::ReturnValue<()>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let data = value.to_rust_string_lossy(scope);
    with_tree_mut(|t| t.set_text_content(h, &data));
}

// `Element.localName` — the lowercase local name (rtdom stores tags lowercased). Non-elements have
// no local name (null). React/Next devtools read node.localName in places.
fn get_local_name(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    match with_tree(|t| t.local_name(h).map(|s| s.to_string())).flatten() {
        Some(ln) => rv.set(v8::String::new(scope, &ln).unwrap().into()),
        None => rv.set(v8::null(scope).into()),
    }
}

fn get_node_name(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let (nt, tag) = with_tree(|t| (t.node_type_id(h), t.tag_name(h))).unwrap_or((1, None));
    let name = match nt {
        3 => "#text".to_string(),
        8 => "#comment".to_string(),
        9 => "#document".to_string(),
        11 => "#document-fragment".to_string(),
        _ => tag.unwrap_or_default().to_uppercase(),
    };
    rv.set(v8::String::new(scope, &name).unwrap().into());
}

fn get_child_nodes(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    let kids = with_tree(|t| t.children(h)).unwrap_or_default();
    let arr = v8::Array::new(scope, kids.len() as i32);
    for (i, k) in kids.into_iter().enumerate() {
        let node = wrap(scope, k);
        arr.set_index(scope, i as u32, node.into());
    }
    bind_method(scope, arr.into(), "item", nodelist_item);
    rv.set(arr.into());
}

/// `NodeList.item(i)` → the i-th node, or null when out of range. Bound on every node-collection
/// array we hand back (childNodes / querySelectorAll / getElementsBy*), so libs calling `.item(i)`
/// instead of `[i]` (e.g. React-DOM's child reconciliation) don't crash.
fn nodelist_item(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let i = args.get(0).number_value(scope).unwrap_or(f64::NAN);
    if i.is_nan() || i < 0.0 { rv.set_null(); return; }
    match args.this().get_index(scope, i as u32) {
        Some(v) if !v.is_undefined() => rv.set(v),
        _ => rv.set_null(),
    }
}

fn get_owner_document(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, _args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    // every node's ownerDocument is the single document (root).
    if let Some(root) = with_tree(|t| t.root()) {
        let doc = wrap(scope, root);
        rv.set(doc.into());
    }
}

thread_local! {
    /// per-element `style` object (a plain JS object React reads/writes); cleared per file.
    static STYLE: RefCell<HashMap<Handle, v8::Global<v8::Object>>> = RefCell::new(HashMap::new());
}

/// `element.style` → a cached plain JS object (CSSStyleDeclaration-lite). React sets `style.color`
/// etc. as own props; `el.style.x === x` works. (Full CSSOM / attribute sync is a later refinement.)
fn get_style(scope: &mut v8::PinScope, _name: v8::Local<v8::Name>, args: v8::PropertyCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(h) = handle_of(scope, args.holder()) else { return };
    if let Some(g) = STYLE.with(|s| s.borrow().get(&h).cloned()) {
        let o = v8::Local::new(scope, &g);
        rv.set(o.into());
        return;
    }
    let obj = v8::Object::new(scope);
    // setProperty/getPropertyValue/removeProperty so libs using the CSSOM methods don't crash.
    bind_method(scope, obj, "setProperty", style_set_property);
    bind_method(scope, obj, "getPropertyValue", style_get_property);
    bind_method(scope, obj, "getPropertyPriority", style_get_property_priority);
    bind_method(scope, obj, "removeProperty", style_remove_property);
    // stash the element handle so the proxy set-trap can reflect to the `style` content attribute.
    if let Some(hk) = v8::String::new(scope, "__h") {
        let hv = v8::Number::new(scope, f64::from(h.raw()));
        obj.set(scope, hk.into(), hv.into());
    }
    // Wrap in a Proxy whose set-trap reflects every style mutation (React writes el.style.x = v) to
    // the `style` content attribute in the tree, so attribute selectors (`[style]`) and
    // getAttribute('style') see it. Reads/methods pass through to the target.
    let handler = v8::Object::new(scope);
    bind_method(scope, handler, "set", style_proxy_set);
    let proxy = v8::Proxy::new(scope, obj, handler).map(|p| p.into()).unwrap_or(obj);
    let g = v8::Global::new(scope, proxy);
    STYLE.with(|s| { s.borrow_mut().insert(h, g); });
    rv.set(proxy.into());
}

/// Proxy `set` trap for `element.style`: set on the target, then serialize the declaration to the
/// element's `style` content attribute so it's observable via attribute selectors / getAttribute.
fn style_proxy_set(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let target = match v8::Local::<v8::Object>::try_from(args.get(0)) { Ok(o) => o, Err(_) => { rv.set_bool(false); return; } };
    let key = args.get(1);
    let mut value = args.get(2);
    // CSSOM values are always strings (jest-dom assigns numbers like `style.fontWeight = 600`; a
    // browser stores "600"). Coerce to string, then normalize hex colors to rgb()/rgba() so
    // el.style.X reads match getComputedStyle's normalized cascade values for jest-dom toHaveStyle.
    if value.is_number() || value.is_boolean() {
        if let Some(s) = value.to_string(scope) { value = s.into(); }
    }
    if value.is_string() {
        if let Some(n) = normalize_css_color(&value.to_rust_string_lossy(scope)) {
            if let Some(s) = v8::String::new(scope, &n) { value = s.into(); }
        }
    }
    target.set(scope, key, value);
    // element handle stashed as __h
    let h = v8::String::new(scope, "__h")
        .and_then(|k| target.get(scope, k.into()))
        .and_then(|v| v.number_value(scope))
        .map(|n| Handle::from_raw(n as u32));
    if let Some(h) = h {
        let css = serialize_style(scope, target);
        with_tree_mut(|t| { if css.is_empty() { t.remove_attribute(h, "style"); } else { t.set_attribute(h, "style", &css); } });
    }
    rv.set_bool(true);
}

/// Normalize a `#rgb`/`#rrggbb`/`#rrggbbaa` hex color to `rgb(r, g, b)` / `rgba(r, g, b, a)` (the form
/// a browser's CSS parser stores), so a hex from one source matches a hex from another. Returns None
/// for non-hex values (left untouched).
fn normalize_css_color(v: &str) -> Option<String> {
    let s = v.trim();
    let hex = s.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let h = |a: u8, b: u8| -> Option<u8> { u8::from_str_radix(&format!("{}{}", a as char, b as char), 16).ok() };
    let b = hex.as_bytes();
    match hex.len() {
        3 => { let (r, g, bl) = (h(b[0], b[0])?, h(b[1], b[1])?, h(b[2], b[2])?); Some(format!("rgb({}, {}, {})", r, g, bl)) }
        6 => { let (r, g, bl) = (h(b[0], b[1])?, h(b[2], b[3])?, h(b[4], b[5])?); Some(format!("rgb({}, {}, {})", r, g, bl)) }
        8 => { let (r, g, bl, a) = (h(b[0], b[1])?, h(b[2], b[3])?, h(b[4], b[5])?, h(b[6], b[7])?);
               let af = (a as f64 / 255.0 * 100.0).round() / 100.0; Some(format!("rgba({}, {}, {}, {})", r, g, bl, af)) }
        _ => None,
    }
}

/// Serialize a style declaration object's own string properties to `prop: value;` cssText (camelCase
/// names → kebab-case), skipping internals (`__*`) and methods.
fn serialize_style(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>) -> String {
    let mut out = String::new();
    let Some(names) = obj.get_own_property_names(scope, Default::default()) else { return out };
    for i in 0..names.length() {
        let Some(k) = names.get_index(scope, i) else { continue };
        let kn = k.to_rust_string_lossy(scope);
        if kn.starts_with("__") { continue; }
        let Some(v) = obj.get(scope, k) else { continue };
        if v.is_function() || v.is_undefined() || v.is_null() { continue; }
        let vs = v.to_rust_string_lossy(scope);
        if vs.is_empty() { continue; }
        // camelCase -> kebab-case
        let mut prop = String::with_capacity(kn.len() + 4);
        for ch in kn.chars() {
            if ch.is_ascii_uppercase() { prop.push('-'); prop.push(ch.to_ascii_lowercase()); }
            else { prop.push(ch); }
        }
        if !out.is_empty() { out.push(' '); }
        out.push_str(&prop); out.push_str(": "); out.push_str(&vs); out.push(';');
    }
    out
}

fn style_set_property(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let name = arg_str(scope, &args, 0);
    let value = args.get(1);
    if let Some(key) = v8::String::new(scope, &name) {
        args.this().set(scope, key.into(), value);
    }
    // 3rd arg = priority ("important" | ""). Track it in a non-style `__prio` map for
    // getPropertyPriority (the `__`-prefixed key is skipped by getComputedStyle's inline overlay).
    let prio = arg_str(scope, &args, 2);
    if let Some(pmap) = style_prio_map(scope, args.this()) {
        if let Some(nk) = v8::String::new(scope, &name) {
            if prio.eq_ignore_ascii_case("important") {
                let pv = v8::String::new(scope, "important").unwrap();
                pmap.set(scope, nk.into(), pv.into());
            } else {
                pmap.delete(scope, nk.into());
            }
        }
    }
}

/// the `__prio` object on a style declaration (created on first use) holding per-property priorities.
fn style_prio_map<'s>(scope: &mut v8::PinScope<'s, '_>, style: v8::Local<v8::Object>) -> Option<v8::Local<'s, v8::Object>> {
    let key = v8::String::new(scope, "__prio")?;
    let existing = style.get(scope, key.into())?;
    if let Ok(o) = v8::Local::<v8::Object>::try_from(existing) {
        return Some(o);
    }
    let o = v8::Object::new(scope);
    style.set(scope, key.into(), o.into());
    Some(o)
}

fn style_get_property_priority(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let name = arg_str(scope, &args, 0);
    let prio = style_prio_map(scope, args.this())
        .and_then(|m| v8::String::new(scope, &name).and_then(|k| m.get(scope, k.into())))
        .filter(|v| v.is_string())
        .map(|v| v.to_rust_string_lossy(scope))
        .unwrap_or_default();
    rv.set(v8::String::new(scope, &prio).unwrap().into());
}

fn style_get_property(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let name = arg_str(scope, &args, 0);
    if let Some(key) = v8::String::new(scope, &name) {
        if let Some(v) = args.this().get(scope, key.into()) {
            if !v.is_undefined() { rv.set(v); return; }
        }
    }
    rv.set(v8::String::new(scope, "").unwrap().into());
}

fn style_remove_property(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let name = arg_str(scope, &args, 0);
    if let Some(key) = v8::String::new(scope, &name) {
        args.this().delete(scope, key.into());
    }
}

// ---- document methods ------------------------------------------------------------------------

fn doc_create_element(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let tag = arg_str(scope, &args, 0);
    let Some(h) = with_tree_mut(|t| t.create_element(&tag)) else { return };
    let node = wrap(scope, h);
    rv.set(node.into());
}

fn doc_create_text_node(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let data = arg_str(scope, &args, 0);
    let Some(h) = with_tree_mut(|t| t.create_text_node(&data)) else { return };
    let node = wrap(scope, h);
    rv.set(node.into());
}

fn doc_get_element_by_id(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = arg_str(scope, &args, 0);
    let found = with_tree(|t| t.get_element_by_id(&id)).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

fn doc_query_selector(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let sel = arg_str(scope, &args, 0);
    let found = with_tree(|t| t.query_selector(&sel)).flatten();
    let v = wrap_opt(scope, found);
    rv.set(v);
}

fn doc_query_selector_all(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let sel = arg_str(scope, &args, 0);
    let handles: Vec<Handle> = with_tree(|t| t.query_selector_all(&sel).to_vec()).unwrap_or_default();
    let arr = v8::Array::new(scope, handles.len() as i32);
    for (i, hh) in handles.into_iter().enumerate() {
        let node = wrap(scope, hh);
        arr.set_index(scope, i as u32, node.into());
    }
    bind_method(scope, arr.into(), "item", nodelist_item);
    rv.set(arr.into());
}

// ---- install ---------------------------------------------------------------------------------

fn bind_method(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str, f: impl v8::MapFnTo<v8::FunctionCallback>) {
    let tmpl = v8::FunctionTemplate::new(scope, f);
    let func = tmpl.get_function(scope).unwrap();
    let key = v8::String::new(scope, name).unwrap();
    obj.set(scope, key.into(), func.into());
}

/// Set a method (FunctionTemplate) on an object template.
fn tmpl_method(scope: &mut v8::PinScope, tmpl: v8::Local<v8::ObjectTemplate>, name: &str, f: impl v8::MapFnTo<v8::FunctionCallback>) {
    let ft = v8::FunctionTemplate::new(scope, f);
    let key = v8::String::new(scope, name).unwrap();
    tmpl.set(key.into(), ft.into());
}

fn tmpl_getter(scope: &mut v8::PinScope, tmpl: v8::Local<v8::ObjectTemplate>, name: &str, g: impl v8::MapFnTo<v8::AccessorNameGetterCallback>) {
    let key = v8::String::new(scope, name).unwrap();
    tmpl.set_accessor(key.into(), g);
}

fn tmpl_accessor(scope: &mut v8::PinScope, tmpl: v8::Local<v8::ObjectTemplate>, name: &str, g: impl v8::MapFnTo<v8::AccessorNameGetterCallback>, s: impl v8::MapFnTo<v8::AccessorNameSetterCallback>) {
    let key = v8::String::new(scope, name).unwrap();
    tmpl.set_accessor_with_setter(key.into(), g, s);
}

/// Build the shared element template (methods + accessors).
fn build_el_template<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::ObjectTemplate> {
    let tmpl = v8::ObjectTemplate::new(scope);
    tmpl.set_internal_field_count(1);

    tmpl_method(scope, tmpl, "appendChild", el_append_child);
    tmpl_method(scope, tmpl, "removeChild", el_remove_child);
    tmpl_method(scope, tmpl, "insertBefore", el_insert_before);
    tmpl_method(scope, tmpl, "setAttribute", el_set_attribute);
    tmpl_method(scope, tmpl, "getAttribute", el_get_attribute);
    tmpl_method(scope, tmpl, "getAttributeNode", el_get_attribute_node);
    tmpl_method(scope, tmpl, "setAttributeNode", el_set_attribute_node);
    tmpl_method(scope, tmpl, "removeAttributeNode", el_remove_attribute_node);
    tmpl_method(scope, tmpl, "hasAttribute", el_has_attribute);
    tmpl_method(scope, tmpl, "removeAttribute", el_remove_attribute);
    tmpl_method(scope, tmpl, "querySelector", el_query_selector);
    tmpl_method(scope, tmpl, "querySelectorAll", el_query_selector_all);
    tmpl_method(scope, tmpl, "addEventListener", el_add_event_listener);
    tmpl_method(scope, tmpl, "removeEventListener", el_remove_event_listener);
    tmpl_method(scope, tmpl, "dispatchEvent", el_dispatch_event);
    tmpl_method(scope, tmpl, "matches", el_matches);
    tmpl_method(scope, tmpl, "closest", el_closest);
    tmpl_method(scope, tmpl, "contains", el_contains);
    tmpl_method(scope, tmpl, "cloneNode", el_clone_node);
    tmpl_method(scope, tmpl, "getRootNode", el_get_root_node);
    tmpl_method(scope, tmpl, "containsNode", el_contains_node);
    tmpl_method(scope, tmpl, "append", el_append);
    tmpl_method(scope, tmpl, "prepend", el_append);
    tmpl_method(scope, tmpl, "remove", el_remove_self);
    tmpl_method(scope, tmpl, "focus", el_focus);
    tmpl_method(scope, tmpl, "blur", el_blur);
    tmpl_method(scope, tmpl, "click", el_click);
    tmpl_method(scope, tmpl, "scrollIntoView", el_scroll_into_view);
    tmpl_method(scope, tmpl, "getBoundingClientRect", el_get_bounding_client_rect);
    // CharacterData (text/comment node) mutation — native over rtdom's text storage.
    tmpl_method(scope, tmpl, "insertData", el_insert_data);
    tmpl_method(scope, tmpl, "deleteData", el_delete_data);
    tmpl_method(scope, tmpl, "appendData", el_append_data);
    tmpl_method(scope, tmpl, "replaceData", el_replace_data);
    tmpl_method(scope, tmpl, "substringData", el_substring_data);
    tmpl_method(scope, tmpl, "splitText", el_split_text);
    // native cascade lookup (inherited-property resolution) used by getComputedStyle.
    tmpl_method(scope, tmpl, "__cascadeProp", el_cascade_prop);
    // ChildNode / ParentNode manipulation + insertAdjacent* + toggleAttribute + getAttributeNS (native).
    tmpl_method(scope, tmpl, "before", el_before);
    tmpl_method(scope, tmpl, "after", el_after);
    tmpl_method(scope, tmpl, "replaceWith", el_replace_with);
    tmpl_method(scope, tmpl, "replaceChildren", el_replace_children);
    tmpl_method(scope, tmpl, "insertAdjacentElement", el_insert_adjacent_element);
    tmpl_method(scope, tmpl, "insertAdjacentHTML", el_insert_adjacent_html);
    tmpl_method(scope, tmpl, "toggleAttribute", el_toggle_attribute);
    tmpl_method(scope, tmpl, "getAttributeNS", el_get_attribute_ns);

    tmpl_getter(scope, tmpl, "tagName", get_tag_name);
    tmpl_getter(scope, tmpl, "parentNode", get_parent_node);
    tmpl_getter(scope, tmpl, "firstChild", get_first_child);
    tmpl_getter(scope, tmpl, "nextSibling", get_next_sibling);
    tmpl_getter(scope, tmpl, "style", get_style);
    tmpl_getter(scope, tmpl, "constructor", get_constructor);
    tmpl_getter(scope, tmpl, "nodeType", get_node_type);
    tmpl_getter(scope, tmpl, "activeElement", get_active_element);
    tmpl_accessor(scope, tmpl, "data", get_node_data, set_node_data);
    tmpl_accessor(scope, tmpl, "nodeValue", get_node_data, set_node_data);
    tmpl_getter(scope, tmpl, "nodeName", get_node_name);
    tmpl_getter(scope, tmpl, "localName", get_local_name);
    tmpl_getter(scope, tmpl, "childNodes", get_child_nodes);
    tmpl_getter(scope, tmpl, "ownerDocument", get_owner_document);
    tmpl_getter(scope, tmpl, "outerHTML", get_outer_html);
    tmpl_getter(scope, tmpl, "children", get_children);
    tmpl_getter(scope, tmpl, "parentElement", get_parent_element);
    tmpl_getter(scope, tmpl, "firstElementChild", get_first_element_child);
    tmpl_getter(scope, tmpl, "namespaceURI", get_namespace_uri);
    tmpl_getter(scope, tmpl, "isConnected", get_is_connected);
    tmpl_getter(scope, tmpl, "attributes", get_attributes);
    tmpl_getter(scope, tmpl, "dataset", get_dataset);
    tmpl_accessor(scope, tmpl, "innerHTML", get_inner_html, set_inner_html);

    tmpl_accessor(scope, tmpl, "textContent", get_text_content, set_text_content);
    tmpl_accessor(scope, tmpl, "id", get_id, set_id);
    tmpl_accessor(scope, tmpl, "className", get_class_name, set_class_name);
    tmpl_accessor(scope, tmpl, "contentEditable", get_content_editable, set_content_editable);
    tmpl_getter(scope, tmpl, "isContentEditable", get_is_content_editable);

    // Node-type constants on every node (`node.TEXT_NODE === 3`, …). dom-accessibility-api compares
    // `node.nodeType === node.TEXT_NODE`, so without these the accessible-name walk skips all text
    // nodes → empty names → getByRole({name}) finds nothing.
    for (name, val) in [
        ("ELEMENT_NODE", 1), ("ATTRIBUTE_NODE", 2), ("TEXT_NODE", 3), ("CDATA_SECTION_NODE", 4),
        ("PROCESSING_INSTRUCTION_NODE", 7), ("COMMENT_NODE", 8), ("DOCUMENT_NODE", 9),
        ("DOCUMENT_TYPE_NODE", 10), ("DOCUMENT_FRAGMENT_NODE", 11),
    ] {
        let key = v8::String::new(scope, name).unwrap();
        let v = v8::Integer::new(scope, val);
        tmpl.set(key.into(), v.into());
    }

    // Always-on graceful-degradation interceptor: unimplemented members read as undefined (no
    // crash); recording the access list is gated by log_enabled() (TURBO_RUST_DOM_LOG).
    let handler = v8::NamedPropertyHandlerConfiguration::new()
        .getter(missing_getter)
        .setter(passthrough_setter)
        .flags(v8::PropertyHandlerFlags::NON_MASKING | v8::PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS);
    tmpl.set_named_property_handler(handler);

    tmpl
}

/// Install `window`/`document` + the DOM onto `globalThis` for the current context.
pub fn install(scope: &mut v8::PinScope) {
    let tree = Tree::parse("<!DOCTYPE html><html><head></head><body></body></html>");
    let el_template = {
        let t = build_el_template(scope);
        v8::Global::new(scope, t)
    };
    DOM.with(|d| {
        *d.borrow_mut() = Some(DomState { tree, cache: HashMap::new(), el_template });
    });

    let root = with_tree(|t| t.root()).unwrap();
    let body_h = with_tree(|t| t.query_selector("body")).flatten();
    let html_h = with_tree(|t| t.query_selector("html")).flatten();

    // document = the root node object + document methods.
    let document = wrap(scope, root);
    bind_method(scope, document, "createElement", doc_create_element);
    bind_method(scope, document, "createTextNode", doc_create_text_node);
    bind_method(scope, document, "getElementById", doc_get_element_by_id);
    bind_method(scope, document, "querySelector", doc_query_selector);
    bind_method(scope, document, "querySelectorAll", doc_query_selector_all);
    bind_method(scope, document, "createElementNS", doc_create_element_ns);
    bind_method(scope, document, "createDocumentFragment", doc_create_fragment);
    bind_method(scope, document, "createComment", doc_create_comment);
    if let Some(b) = body_h {
        let body = wrap(scope, b);
        let key = v8::String::new(scope, "body").unwrap();
        document.set(scope, key.into(), body.into());
    }
    if let Some(html) = html_h {
        let de = wrap(scope, html);
        let key = v8::String::new(scope, "documentElement").unwrap();
        document.set(scope, key.into(), de.into());
    }

    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, "document").unwrap();
    global.set(scope, key.into(), document.into());
    // window = globalThis
    let key = v8::String::new(scope, "window").unwrap();
    global.set(scope, key.into(), global.into());

    // Window-level globals that libs read as BARE identifiers (`navigator`, `HTMLElement`, …) — a
    // missing bare global is a ReferenceError the element interceptor can't catch, so they must
    // exist on globalThis. Plain-data / stub surface (refined natively as the logger surfaces hot
    // members); document extras that are pure JS go here too.
    run_js(scope, BOOTSTRAP);
}

/// Compile + run a JS snippet in the current context (best-effort).
fn run_js(scope: &mut v8::PinScope, src: &str) {
    let tc = std::pin::pin!(v8::TryCatch::new(scope));
    let tc = &mut tc.init();
    if let Some(code) = v8::String::new(tc, src) {
        if let Some(s) = v8::Script::compile(tc, code, None) {
            s.run(tc);
        }
    }
    if tc.has_caught() && log_enabled() {
        let msg = tc.exception().map(|e| e.to_rust_string_lossy(tc)).unwrap_or_default();
        eprintln!("[rust-dom] bootstrap threw: {msg}");
    }
}

/// JS enrichment shims layered over the native rtdom binding — browser globals/constructors
/// (navigator, Event subclasses, DOM-interface ctors with Symbol.hasInstance), CSSOM `.sheet`,
/// and pure-JS document extras. Kept in a sibling .js file (not Rust) so the shims are editable
/// as JS; the Rust side is purely the rtdom↔V8 binding.
const BOOTSTRAP: &str = include_str!("browser_env.js");

/// Reset the per-thread DOM (call between test files when reusing an isolate).
pub fn reset() {
    STYLE.with(|s| s.borrow_mut().clear());
    LISTENERS.with(|s| s.borrow_mut().clear());
    ACTIVE.with(|a| *a.borrow_mut() = None);
    if log_enabled() {
        dump_missing();
    }
    DOM.with(|d| *d.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    static INIT: Once = Once::new();
    fn init_v8() {
        INIT.call_once(|| {
            let platform = v8::new_default_platform(0, false).make_shared();
            v8::V8::initialize_platform(platform);
            v8::V8::initialize();
        });
    }

    #[test]
    fn dom_slice_smoke() {
        init_v8();
        let isolate = &mut v8::Isolate::new(Default::default());
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        install(scope);

        let run = |scope: &mut v8::PinScope, code: &str| -> String {
            let src = v8::String::new(scope, code).unwrap();
            let script = v8::Script::compile(scope, src, None).unwrap();
            let r = script.run(scope).unwrap();
            r.to_rust_string_lossy(scope)
        };

        // create + append + attribute + textContent + querySelector + identity
        let out = run(scope, r#"
            const d = document.createElement('div');
            d.setAttribute('id', 'x');
            d.className = 'card';
            d.textContent = 'hello';
            document.body.appendChild(d);
            const found = document.querySelector('#x');
            JSON.stringify({
                tag: d.tagName,
                id: d.id,
                cls: d.className,
                text: found.textContent,
                identity: found === d,
                getAttr: d.getAttribute('id'),
                inBody: document.body.firstChild === d,
            });
        "#);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tag"], "DIV", "{out}");
        assert_eq!(v["id"], "x", "{out}");
        assert_eq!(v["cls"], "card", "{out}");
        assert_eq!(v["text"], "hello", "{out}");
        assert_eq!(v["identity"], true, "querySelector must return the SAME object: {out}");
        assert_eq!(v["getAttr"], "x", "{out}");
        assert_eq!(v["inBody"], true, "{out}");
        reset();
    }
}
