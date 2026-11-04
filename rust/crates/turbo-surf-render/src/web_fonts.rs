//! `@font-face` / `FontFace` faces for live layout and canvases (turbo-surf#130).
//!
//! Page JS (`font_loading.js`) fetches a face's source over the page's network stack,
//! hands the bytes here to decode (WOFF/WOFF2 → sfnt) and parse with turbo-dom's
//! `FontFace::from_web_font_bytes`, and attaches each loaded face that is in a
//! document's `FontFaceSet`. Live layout and `set_canvas_fonts` then use the system
//! registry plus those faces; the per-document epoch keys the layout cache.
use deno_core::{op2, v8};
use lightningcss::rules::font_face::{FontFaceProperty, Source};
use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use turbo_dom_layout::text::FontFace;
use turbo_dom_layout::FontRegistry;

#[derive(Default)]
struct DocumentFonts {
    epoch: u64,
    faces: Vec<(u32, FontFace)>,
    registry: Option<Rc<FontRegistry>>,
}

#[derive(Default)]
struct Store {
    next: u32,
    parsed: HashMap<u32, FontFace>,
    documents: HashMap<u32, DocumentFonts>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::default();
}

/// Decode a web font's bytes to an sfnt: WOFF and WOFF2 are unwrapped, anything else
/// (TrueType, OpenType, a collection) is passed through for the parser to judge.
fn sfnt(bytes: &[u8]) -> Option<Vec<u8>> {
    match bytes.get(..4) {
        Some(b"wOFF") => wuff::decompress_woff1(bytes).ok(),
        Some(b"wOF2") => wuff::decompress_woff2(bytes).ok(),
        _ => Some(bytes.to_vec()),
    }
}

fn parse(bytes: &[u8], family: String, weight: u16, italic: bool) -> Option<FontFace> {
    FontFace::from_web_font_bytes(sfnt(bytes)?, family, weight, italic)
}

/// Parse a face's bytes; the handle (0 = not a usable font) is what `attach` takes.
#[op2(fast)]
pub(crate) fn op_font_face_parse(
    #[buffer] bytes: &[u8],
    #[string] family: String,
    weight: u32,
    italic: bool,
) -> u32 {
    let weight = u16::try_from(weight.clamp(1, 1000)).unwrap_or(400);
    let Some(face) = parse(bytes, family, weight, italic) else {
        return 0;
    };
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.next += 1;
        let id = store.next;
        store.parsed.insert(id, face);
        id
    })
}

fn document_of(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> u32 {
    turbo_dom_browser_env::host_internals::document_id_of(scope, document) as u32
}

/// Rebuild the document's registry and hand it to this realm's canvases.
fn changed(scope: &mut v8::PinScope, document: u32) {
    let registry = STORE.with(|store| {
        let mut store = store.borrow_mut();
        let entry = store.documents.entry(document).or_default();
        entry.epoch += 1;
        entry.registry = (!entry.faces.is_empty()).then(|| {
            let mut registry = crate::live_layout_backend::initialize_system_fonts().clone();
            for (_, face) in &entry.faces {
                registry.add(face.clone());
            }
            Rc::new(registry)
        });
        entry.registry.clone()
    });
    let context = scope.get_current_context();
    let fonts = registry.map_or_else(
        || crate::live_layout_backend::initialize_system_fonts().clone(),
        |registry| (*registry).clone(),
    );
    crate::browser_env::set_canvas_fonts(context, fonts);
}

/// A loaded face joined the document's `FontFaceSet`.
#[op2(fast, reentrant)]
pub(crate) fn op_font_face_attach(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    face: u32,
) -> bool {
    let document = document_of(scope, document);
    let attached = STORE.with(|store| {
        let mut store = store.borrow_mut();
        let Some(parsed) = store.parsed.get(&face).cloned() else {
            return false;
        };
        let entry = store.documents.entry(document).or_default();
        if entry.faces.iter().any(|(id, _)| *id == face) {
            return false;
        }
        entry.faces.push((face, parsed));
        true
    });
    if attached {
        changed(scope, document);
    }
    attached
}

/// A face left the document's `FontFaceSet`.
#[op2(fast, reentrant)]
pub(crate) fn op_font_face_detach(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    face: u32,
) -> bool {
    let document = document_of(scope, document);
    let detached = STORE.with(|store| {
        let mut store = store.borrow_mut();
        let entry = store.documents.entry(document).or_default();
        let before = entry.faces.len();
        entry.faces.retain(|(id, _)| *id != face);
        before != entry.faces.len()
    });
    if detached {
        changed(scope, document);
    }
    detached
}

/// The document's web-font registry and its epoch, when it has attached faces.
pub(crate) fn document_fonts(document: u32) -> (u64, Option<Rc<FontRegistry>>) {
    STORE.with(|store| {
        store
            .borrow()
            .documents
            .get(&document)
            .map_or((0, None), |entry| (entry.epoch, entry.registry.clone()))
    })
}

fn value_text(property: &FontFaceProperty<'_>) -> Option<String> {
    let text = property.to_css_string(PrinterOptions::default()).ok()?;
    Some(text.split_once(':')?.1.trim().to_string())
}

fn source_json(source: &Source<'_>) -> serde_json::Value {
    match source {
        Source::Url(url) => serde_json::json!({
            "url": url.url.url.to_string(),
            "format": url.format.as_ref().and_then(|format| format.to_css_string(PrinterOptions::default()).ok()).map(|format| format.trim_matches('"').to_ascii_lowercase()),
        }),
        Source::Local(family) => serde_json::json!({
            "local": family.to_css_string(PrinterOptions::default()).unwrap_or_default(),
        }),
    }
}

/// The descriptors of one `@font-face` rule's text as JSON (null when it is not one).
/// Only descriptors that parsed appear, so a missing key marks an invalid value.
pub(crate) fn rule_descriptors(css: &str) -> Option<serde_json::Value> {
    let sheet = StyleSheet::parse(css, ParserOptions::default()).ok()?;
    let [CssRule::FontFace(rule)] = sheet.rules.0.as_slice() else {
        return None;
    };
    let mut out = serde_json::Map::new();
    for property in &rule.properties {
        let (key, value) = match property {
            FontFaceProperty::Source(sources) => (
                "src",
                serde_json::Value::Array(sources.iter().map(source_json).collect()),
            ),
            FontFaceProperty::FontFamily(family) => (
                "family",
                crate::live_layout_resources::family_name(family)
                    .ok()?
                    .into(),
            ),
            FontFaceProperty::FontStyle(_) => ("style", value_text(property)?.into()),
            FontFaceProperty::FontWeight(_) => ("weight", value_text(property)?.into()),
            FontFaceProperty::FontStretch(_) => ("stretch", value_text(property)?.into()),
            FontFaceProperty::UnicodeRange(_) => ("unicodeRange", value_text(property)?.into()),
            FontFaceProperty::Custom(_) => continue,
        };
        out.insert(key.into(), value);
    }
    Some(serde_json::Value::Object(out))
}

#[op2]
#[string]
pub(crate) fn op_font_face_rule(#[string] css: String) -> String {
    rule_descriptors(&css).map_or_else(|| "null".into(), |value| value.to_string())
}

/// `FontFaceSet` hook: live layout reports the `@font-face` families it used.
struct LayoutHook(v8::Global<v8::Function>);

#[op2(fast, reentrant)]
pub(crate) fn op_font_face_layout_hook(scope: &mut v8::PinScope, hook: v8::Local<v8::Function>) {
    let context = scope.get_current_context();
    context.set_slot(Rc::new(LayoutHook(v8::Global::new(scope, hook))));
}

/// Hand the families the last layouts used to the realm's loader, which marks the
/// matching unloaded faces loading (synchronously, as Chrome's style update does)
/// and starts their fetches.
pub(crate) fn report_used<'s>(scope: &mut v8::PinScope<'s, '_>, used: &[String]) -> bool {
    let context = scope.get_current_context();
    let Some(hook) = context.get_slot::<LayoutHook>() else {
        return false;
    };
    let hook = v8::Local::new(scope, &hook.0);
    let names = used
        .iter()
        .filter_map(|name| v8::String::new(scope, name).map(Into::into))
        .collect::<Vec<v8::Local<v8::Value>>>();
    let names = v8::Array::new_with_elements(scope, &names);
    let receiver = v8::undefined(scope);
    v8::tc_scope!(let scope, scope);
    hook.call(scope, receiver.into(), &[names.into()]).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTF: &[u8] = include_bytes!(
        "../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"
    );
    const WOFF: &[u8] = include_bytes!(
        "../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.woff"
    );
    const WOFF2: &[u8] = include_bytes!(
        "../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular.woff2"
    );

    #[test]
    fn woff_and_woff2_decode_to_the_same_face_as_the_ttf() {
        let advance = |bytes: &[u8]| {
            let face = parse(bytes, "Plex".into(), 400, false).expect("face parses");
            assert_eq!(face.family(), "Plex");
            face.html_metrics(20.0)
        };
        let ttf = advance(TTF);
        assert_eq!(advance(WOFF), ttf);
        assert_eq!(advance(WOFF2), ttf);
        assert!(sfnt(b"wOF2 broken").is_none());
        assert!(sfnt(b"wOFF broken").is_none());
        assert!(parse(&[1, 2, 3], "Bad".into(), 400, false).is_none());
    }

    #[test]
    fn rule_descriptors_report_only_valid_descriptors() {
        let value = rule_descriptors(
            "@font-face{font-family:\"Surf A\";src:url(/a.woff2) format('woff2'),local(Foo);font-weight:700;font-style:italic;font-stretch:condensed;unicode-range:U+0-7F}",
        )
        .unwrap();
        assert_eq!(value["family"], "Surf A");
        assert_eq!(value["src"][0]["url"], "/a.woff2");
        assert_eq!(value["src"][0]["format"], "woff2");
        assert_eq!(value["src"][1]["local"], "Foo");
        assert_eq!(value["weight"], "700");
        assert_eq!(value["style"], "italic");
        assert!(value["stretch"].is_string());
        assert!(value["unicodeRange"].is_string());
        let bogus = rule_descriptors("@font-face{font-family:x;font-weight:bogus}").unwrap();
        assert!(bogus.get("weight").is_none());
        assert!(rule_descriptors("a{color:red}").is_none());
        assert!(rule_descriptors("@font-face{").is_some_and(|value| value.get("src").is_none()));
    }
}
