//! Actual native layout behind CSSOM reads. Each read captures current native
//! tree/CSS state; no public getters or page-installed layout cache participates.
use crate::live_layout_media::ViewportMedia;
use crate::live_layout_metrics::{ClientMetrics, OffsetSize};
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
use serde::Serialize;
use turbo_html2pdf_core::layout_snapshot::SnapshotInputs;
use turbo_html2pdf_core::{Diagnostics, FontRegistry, ImageCtx, NoImages};

thread_local! {
    // Immutable native font bytes, with no guest/context references. Downloaded
    // FontFace epochs must be added separately rather than leaking page state.
    static SYSTEM_FONTS:std::cell::OnceCell<std::rc::Rc<FontRegistry>>=const {std::cell::OnceCell::new()};
}

#[derive(PartialEq)]
struct CacheKey {
    document: u32,
    version: u64,
    width: u32,
    height: u32,
    rules: Vec<String>,
}
struct LayoutView {
    snapshot: turbo_html2pdf_core::layout_snapshot::LayoutSnapshot,
    styles: crate::live_layout_scroll::SourceStyles,
}
struct CacheEntry {
    key: CacheKey,
    snapshot: std::rc::Rc<LayoutView>,
}
thread_local! {static LAYOUT_CACHE:std::cell::RefCell<std::collections::VecDeque<CacheEntry>>=const {std::cell::RefCell::new(std::collections::VecDeque::new())};}
#[derive(Clone, Copy)]
pub(crate) struct Viewport {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) device_scale: f64,
}
impl crate::live_stylesheets::Conditions for Viewport {
    fn media(&self, query: &str) -> Result<bool, String> {
        ViewportMedia {
            width: self.width,
            height: self.height,
            initial_font_size: 16.0,
        }
        .matches(query)
    }
    fn supports(&self, _: &str) -> Result<bool, String> {
        Err("native layout supports condition evaluation is pending".into())
    }
}
#[derive(Serialize)]
pub(crate) struct Geometry {
    bounds: [f32; 4],
    rects: Vec<[f32; 4]>,
    client: ClientMetrics,
    offset: OffsetSize,
}
#[op2]
#[serde]
pub(crate) fn op_live_element_geometry<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<Geometry, JsErrorBox> {
    measure(scope, receiver).map_err(|error| {
        if error == "Illegal invocation" {
            JsErrorBox::type_error(error)
        } else {
            JsErrorBox::generic(error)
        }
    })
}
fn measure<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<Geometry, String> {
    let input = crate::browser_env::capture_native_layout(scope, receiver)?;
    let mut geometry = if !input.connected || !input.browsing_context {
        crate::live_layout_metrics::empty_geometry()
    } else {
        let context = input
            .owner_context
            .ok_or("native layout owner realm unavailable")?;
        let viewport = context
            .get_slot::<Viewport>()
            .ok_or("native layout viewport was not captured")?;
        let snapshot = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
        crate::live_layout_metrics::element_geometry(
            &snapshot.snapshot,
            crate::live_layout::source_id(input.tree.document_id, input.handle),
        )?
    };
    if input.connected && input.browsing_context {
        let context = input
            .owner_context
            .ok_or("native layout owner realm unavailable")?;
        let viewport = context
            .get_slot::<Viewport>()
            .ok_or("native layout viewport was not captured")?;
        let snapshot = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
        let state = scroll_state(context);
        let translation = crate::live_layout_scroll::scroll_translation(
            &snapshot.snapshot,
            &snapshot.styles,
            crate::live_layout::source_id(input.tree.document_id, input.handle),
            &state.values.borrow(),
        )?;
        geometry.bounds.x -= translation[0];
        geometry.bounds.y -= translation[1];
        for rect in &mut geometry.rects {
            rect.x -= translation[0];
            rect.y -= translation[1];
        }
    }
    let rect = |r: turbo_html2pdf_core::layout_snapshot::Rect| [r.x, r.y, r.width, r.height];
    Ok(Geometry {
        bounds: rect(geometry.bounds),
        rects: geometry.rects.into_iter().map(rect).collect(),
        client: geometry.client,
        offset: geometry.offset,
    })
}
fn resolve(
    tree: &crate::live_layout::LiveTreeInput,
    sheets: &[crate::live_stylesheets::SheetSnapshot],
    viewport: &Viewport,
) -> Result<std::rc::Rc<LayoutView>, String> {
    let rules = crate::live_stylesheets::active_rules(sheets, viewport)?;
    let fonts = SYSTEM_FONTS.with(|fonts| {
        fonts
            .get_or_init(|| {
                let mut registry = FontRegistry::new();
                registry.load_system_fonts();
                std::rc::Rc::new(registry)
            })
            .clone()
    });
    let images = ImageCtx {
        resolver: &NoImages,
        body_height: None,
    };
    let key = CacheKey {
        document: tree.document_id,
        version: tree.tree_version,
        width: viewport.width.to_bits(),
        height: viewport.height.to_bits(),
        rules,
    };
    let cached = LAYOUT_CACHE.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.snapshot.clone())
    });
    let snapshot = if let Some(snapshot) = cached {
        snapshot
    } else {
        let (snapshot, styles) = crate::live_layout::layout_rules_resolved(
            tree,
            &key.rules,
            &SnapshotInputs {
                width: viewport.width,
                height: viewport.height,
                fonts: &fonts,
                images: &images,
            },
            &mut Diagnostics::default(),
        )?;
        let snapshot = std::rc::Rc::new(LayoutView { snapshot, styles });
        LAYOUT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| entry.key.document != key.document);
            if cache.len() == 4 {
                cache.pop_front();
            }
            cache.push_back(CacheEntry {
                key,
                snapshot: snapshot.clone(),
            });
        });
        snapshot
    };
    Ok(snapshot)
}

#[derive(Default)]
struct ScrollState {
    values: std::cell::RefCell<
        std::collections::BTreeMap<turbo_html2pdf_core::layout_snapshot::SourceId, [f64; 2]>,
    >,
}
fn scroll_state(context: v8::Local<v8::Context>) -> std::rc::Rc<ScrollState> {
    if let Some(state) = context.get_slot::<ScrollState>() {
        return state;
    }
    let state = std::rc::Rc::new(ScrollState::default());
    context.set_slot(state.clone());
    state
}
#[derive(Serialize)]
pub(crate) struct ScrollResult {
    width: i32,
    height: i32,
    left: f64,
    top: f64,
}
#[op2]
#[serde]
pub(crate) fn op_live_element_scroll<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    axis: u32,
    value: f64,
) -> Result<ScrollResult, JsErrorBox> {
    scroll(scope, receiver, axis, value).map_err(|error| {
        if error == "Illegal invocation" {
            JsErrorBox::type_error(error)
        } else {
            JsErrorBox::generic(error)
        }
    })
}
fn scroll<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    axis: u32,
    value: f64,
) -> Result<ScrollResult, String> {
    let input = crate::browser_env::capture_native_layout(scope, receiver)?;
    if !input.connected || !input.browsing_context {
        return Ok(ScrollResult {
            width: 0,
            height: 0,
            left: 0.0,
            top: 0.0,
        });
    }
    let context = input
        .owner_context
        .ok_or("native layout owner realm unavailable")?;
    let viewport = context
        .get_slot::<Viewport>()
        .ok_or("native layout viewport was not captured")?;
    let snapshot = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
    let id = crate::live_layout::source_id(input.tree.document_id, input.handle);
    let metrics =
        crate::live_layout_scroll::scroll_metrics(&snapshot.snapshot, &snapshot.styles, id)?;
    let state = scroll_state(context);
    let mut values = state.values.borrow_mut();
    values.retain(|source, _| snapshot.styles.contains_key(source));
    let offset = values.entry(id).or_insert([0.0; 2]);
    let snap = |value: f64| (value * viewport.device_scale + 0.5).floor() / viewport.device_scale;
    let maximum = [snap(metrics.max_x), snap(metrics.max_y)];
    for i in 0..2 {
        if !(if i == 0 {
            metrics.scroll_x
        } else {
            metrics.scroll_y
        }) {
            offset[i] = 0.0;
        } else {
            offset[i] = offset[i].clamp(0.0, maximum[i]);
        }
    }
    if axis == 1 || axis == 2 {
        let index = (axis - 1) as usize;
        if if index == 0 {
            metrics.scroll_x
        } else {
            metrics.scroll_y
        } {
            offset[index] = if value.is_finite() {
                snap(value.clamp(0.0, maximum[index])).clamp(0.0, maximum[index])
            } else {
                0.0
            };
        }
    }
    Ok(ScrollResult {
        width: metrics.width,
        height: metrics.height,
        left: offset[0],
        top: offset[1],
    })
}
/// Capture trusted bootstrap values once, before authored page code can replace
/// the Window dimensions. This is not a second arbitrary default viewport.
pub(crate) fn capture_viewport(scope: &mut v8::PinScope) -> Result<(), String> {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let mut values = [0.0; 2];
    for (i, name) in ["innerWidth", "innerHeight"].into_iter().enumerate() {
        let key = v8::String::new(scope, name).ok_or("native layout viewport key interrupted")?;
        let value = global
            .get(scope, key.into())
            .ok_or("native layout viewport unavailable")?;
        if !value.is_number() {
            return Err("native layout viewport must be numeric".into());
        }
        values[i] = value
            .number_value(scope)
            .ok_or("native layout viewport interrupted")? as f32;
    }
    if !values.iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err("native layout viewport must be finite and positive".into());
    }
    let key =
        v8::String::new(scope, "devicePixelRatio").ok_or("native device scale key interrupted")?;
    let device_scale = global
        .get(scope, key.into())
        .filter(|value| value.is_number())
        .and_then(|value| value.number_value(scope))
        .ok_or("native device scale must be numeric")?;
    if !device_scale.is_finite() || device_scale <= 0.0 {
        return Err("native device scale must be positive and finite".into());
    }
    LAYOUT_CACHE.with(|cache| cache.borrow_mut().clear());
    context.set_slot(std::rc::Rc::new(Viewport {
        width: values[0],
        height: values[1],
        device_scale,
    }));
    Ok(())
}

#[op2]
pub(crate) fn op_live_elements_at_point<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    x: f64,
    y: f64,
) -> Result<v8::Local<'s, v8::Array>, JsErrorBox> {
    hit(scope, document, [x, y]).map_err(|error| {
        if error == "Illegal invocation" {
            JsErrorBox::type_error(error)
        } else {
            JsErrorBox::generic(error)
        }
    })
}
fn hit<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    point: [f64; 2],
) -> Result<v8::Local<'s, v8::Array>, String> {
    let input = crate::browser_env::capture_native_document_layout(scope, document)?;
    if !input.browsing_context {
        return Ok(v8::Array::new(scope, 0));
    }
    let context = input
        .owner_context
        .ok_or("native layout owner realm unavailable")?;
    let viewport = context
        .get_slot::<Viewport>()
        .ok_or("native layout viewport was not captured")?;
    let snapshot = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
    let state = scroll_state(context);
    let ids = crate::live_layout_hit::elements_at_point(
        &snapshot.snapshot,
        &snapshot.styles,
        &state.values.borrow(),
        point,
    )?;
    crate::browser_env::wrap_native_layout_elements(scope, document, &ids)
}

#[op2]
pub(crate) fn op_live_element_offset<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    offset(scope, receiver).map_err(|error| {
        if error == "Illegal invocation" {
            JsErrorBox::type_error(error)
        } else {
            JsErrorBox::generic(error)
        }
    })
}
fn offset<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, String> {
    let input = crate::browser_env::capture_native_layout(scope, receiver)?;
    let position = if !input.connected || !input.browsing_context {
        crate::live_layout_metrics::OffsetPosition {
            left: 0,
            top: 0,
            parent: None,
        }
    } else {
        let context = input
            .owner_context
            .ok_or("native layout owner realm unavailable")?;
        let viewport = context
            .get_slot::<Viewport>()
            .ok_or("native layout viewport was not captured")?;
        let snapshot = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
        crate::live_layout_metrics::offset_position(
            &snapshot.snapshot,
            &snapshot.styles,
            crate::live_layout::source_id(input.tree.document_id, input.handle),
        )?
    };
    let parent = if let Some(parent) = position.parent {
        crate::browser_env::wrap_native_layout_elements(scope, receiver, &[parent])?
            .get_index(scope, 0)
            .ok_or("native offset parent interrupted")?
    } else {
        v8::null(scope).into()
    };
    let result = v8::Object::new(scope);
    let null = v8::null(scope);
    if result.set_prototype(scope, null.into()) != Some(true) {
        return Err("native offset record interrupted".into());
    }
    let values = [
        ("left", v8::Integer::new(scope, position.left).into()),
        ("top", v8::Integer::new(scope, position.top).into()),
        ("parent", parent),
    ];
    for (name, value) in values {
        let name = v8::String::new(scope, name).ok_or("native offset record key interrupted")?;
        if result.create_data_property(scope, name.into(), value) != Some(true) {
            return Err("native offset record write interrupted".into());
        }
    }
    Ok(result)
}
