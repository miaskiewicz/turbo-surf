//! Actual native layout behind CSSOM reads. Each read captures current native
//! tree/CSS state; no public getters or page-installed layout cache participates.
use crate::live_layout_media::{HostMedia, ViewportMedia};
use crate::live_layout_metrics::{ClientMetrics, OffsetSize};
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
use serde::Serialize;
use turbo_html2pdf_core::layout_snapshot::SnapshotInputs;
use turbo_html2pdf_core::{Diagnostics, FontRegistry, ImageCtx, NoImages};

// FontFace bytes and shaping caches are thread-safe and contain no V8 roots.
// Loading every installed font per isolate thread multiplies native memory.
static SYSTEM_FONTS: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();

#[derive(PartialEq)]
struct CacheKey {
    realm: u64,
    document: u32,
    version: u64,
    width: u32,
    height: u32,
    layout_scale: u32,
    host: HostMedia,
    rules: Vec<crate::live_stylesheets::ActiveRule>,
    composition: Vec<(u32, Vec<u32>)>,
    inline_policy: Vec<(u32, String)>,
}
struct LayoutView {
    snapshot: turbo_html2pdf_core::layout_snapshot::LayoutSnapshot,
    styles: crate::live_layout_scroll::SourceStyles,
    omitted: std::collections::HashSet<turbo_html2pdf_core::layout_snapshot::SourceId>,
}
struct CacheEntry {
    key: CacheKey,
    snapshot: std::rc::Rc<LayoutView>,
}
thread_local! {static LAYOUT_CACHE:std::cell::RefCell<std::collections::VecDeque<CacheEntry>>=const {std::cell::RefCell::new(std::collections::VecDeque::new())};}
#[derive(Clone, Copy)]
pub(crate) struct Viewport {
    realm: u64,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) device_scale: f64,
    pub(crate) layout_scale: f32,
    host: HostMedia,
}
impl crate::live_stylesheets::Conditions for Viewport {
    fn media(&self, query: &str) -> Result<bool, String> {
        ViewportMedia {
            width: self.width,
            height: self.height,
            initial_font_size: 16.0,
            host: self.host,
        }
        .matches(query)
    }
    fn supports(&self, condition: &str) -> Result<bool, String> {
        crate::live_layout_supports::matches(condition)
    }
}
#[derive(Serialize)]
pub(crate) struct Geometry {
    bounds: [f32; 4],
    rects: Vec<[f32; 4]>,
    client: ClientMetrics,
    offset: OffsetSize,
}
#[op2(reentrant)]
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
    let geometry = if !input.connected || !input.browsing_context {
        crate::live_layout_metrics::empty_geometry()
    } else {
        let context = input
            .owner_context
            .ok_or("native layout owner realm unavailable")?;
        let viewport = context
            .get_slot::<Viewport>()
            .ok_or("native layout viewport was not captured")?;
        let snapshot = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
        let mut geometry = element_geometry(
            &snapshot,
            crate::live_layout::source_id(input.tree.document_id, input.handle),
        )?;
        if !geometry.rects.is_empty() {
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
        geometry
    };
    let rect = |r: turbo_html2pdf_core::layout_snapshot::Rect| [r.x, r.y, r.width, r.height];
    Ok(Geometry {
        bounds: rect(geometry.bounds),
        rects: geometry.rects.into_iter().map(rect).collect(),
        client: geometry.client,
        offset: geometry.offset,
    })
}
fn element_geometry(
    view: &LayoutView,
    id: turbo_html2pdf_core::layout_snapshot::SourceId,
) -> Result<crate::live_layout_metrics::ElementGeometry, String> {
    if view.omitted.contains(&id)
        || view
            .styles
            .get(&id)
            .is_some_and(|entry| entry.style.get("display") == Some("contents"))
    {
        return Ok(crate::live_layout_metrics::empty_geometry());
    }
    match crate::live_layout_metrics::element_geometry(&view.snapshot, id) {
        Err(error) if error == "native live layout requires inline border/cluster provenance for this element" =>
            crate::live_layout_inline::geometry(&view.snapshot, &view.styles, id),
        result => result,
    }
}
fn resolve(
    tree: &crate::live_layout::LiveTreeInput,
    sheets: &[crate::live_stylesheets::SheetSnapshot],
    viewport: &Viewport,
) -> Result<std::rc::Rc<LayoutView>, String> {
    let rules = crate::live_stylesheets::active_rule_snapshots(sheets, viewport)?;
    let fonts = SYSTEM_FONTS.get_or_init(|| {
        let mut registry = FontRegistry::new();
        registry.load_system_fonts_for_html();
        registry
    });
    let images = ImageCtx {
        resolver: &NoImages,
        body_height: None,
    };
    let key = CacheKey {
        realm: viewport.realm,
        document: tree.document_id,
        version: tree.tree_version,
        width: viewport.width.to_bits(),
        height: viewport.height.to_bits(),
        layout_scale: viewport.layout_scale.to_bits(),
        host: viewport.host,
        rules,
        composition: tree
            .composition
            .as_ref()
            .map_or_else(Vec::new, |graph| graph.cache_edges()),
        inline_policy: tree.composition.as_ref().map_or_else(Vec::new, |graph| {
            let mut rows = graph
                .inline_overrides
                .iter()
                .map(|(handle, text)| (handle.raw(), text.clone()))
                .collect::<Vec<_>>();
            rows.sort_by_key(|(handle, _)| *handle);
            rows
        }),
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
        let (snapshot, styles) = crate::live_layout::layout_active_rules_resolved(
            tree,
            &key.rules,
            viewport.layout_scale,
            &SnapshotInputs {
                width: viewport.width,
                height: viewport.height,
                fonts,
                images: &images,
            },
            &mut Diagnostics::default(),
        )?;
        // Native Range boundaries follow the original DOM. Unassigned light
        // children and inactive fallback nodes are connected but lack flat boxes.
        let omitted =
            tree.composition
                .as_ref()
                .map_or_else(std::collections::HashSet::new, |graph| {
                    let placed = tree
                        .identities
                        .iter()
                        .map(|identity| identity.id)
                        .collect::<std::collections::HashSet<_>>();
                    graph
                        .scopes
                        .keys()
                        .map(|handle| crate::live_layout::source_id(tree.document_id, *handle))
                        .filter(|source| !placed.contains(source))
                        .collect()
                });
        let snapshot = std::rc::Rc::new(LayoutView {
            snapshot,
            styles,
            omitted,
        });
        LAYOUT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache
                .retain(|entry| entry.key.realm != key.realm || entry.key.document != key.document);
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
#[op2(reentrant)]
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
    if snapshot.omitted.contains(&id) {
        return Ok(ScrollResult {
            width: 0,
            height: 0,
            left: 0.0,
            top: 0.0,
        });
    }
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
    // Native document counters restart in each isolate world. Cache snapshots
    // contain no V8 roots, so use a separate non-recycled host realm identity.
    static NEXT_REALM: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let realm = NEXT_REALM
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |next| next.checked_add(1),
        )
        .map_err(|_| "native layout realm identity exhausted")?;
    context.set_slot(std::rc::Rc::new(Viewport {
        realm,
        width: values[0],
        height: values[1],
        device_scale,
        // Calibrated by independent fractional Chrome155 controls; JS DPR is separate.
        layout_scale: 2.0,
        host: HostMedia::capture(crate::runtime::native_pointer_media_capability(context)),
    }));
    Ok(())
}

#[op2(reentrant)]
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
    let ids = if let Some(graph) = &input.tree.composition {
        let mut result = Vec::new();
        for id in ids {
            let handle = turbo_dom_parser::rtdom::tree::Handle::from_raw(id.0 as u32);
            let target = graph.retarget(handle, input.handle);
            let id = crate::live_layout::source_id(input.tree.document_id, target);
            if !result.contains(&id) {
                result.push(id);
            }
        }
        result
    } else {
        ids
    };
    crate::browser_env::wrap_native_layout_elements(scope, document, &ids, 1)
}

#[op2(reentrant)]
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
        let id = crate::live_layout::source_id(input.tree.document_id, input.handle);
        if snapshot.omitted.contains(&id) {
            crate::live_layout_metrics::OffsetPosition {
                left: 0,
                top: 0,
                parent: None,
            }
        } else {
            crate::live_layout_metrics::offset_position(&snapshot.snapshot, &snapshot.styles, id)?
        }
    };
    let parent = if let Some(parent) = position.parent {
        crate::browser_env::wrap_native_layout_elements(scope, receiver, &[parent], 1)?
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

#[derive(Serialize)]
pub(crate) struct RangeGeometry {
    bounds: [f32; 4],
    rects: Vec<[f32; 4]>,
}
#[op2(reentrant)]
#[serde]
pub(crate) fn op_live_range_geometry<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<RangeGeometry, JsErrorBox> {
    range_geometry(scope, receiver).map_err(geometry_error)
}
fn geometry_error(error: String) -> JsErrorBox {
    if error == "Illegal invocation" {
        JsErrorBox::type_error(error)
    } else {
        JsErrorBox::generic(error)
    }
}
fn range_geometry<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<RangeGeometry, String> {
    let input = crate::browser_env::capture_native_layout(scope, receiver)?;
    let selections = input.range.as_ref().ok_or("Illegal invocation")?;
    let mut rects = Vec::new();
    if input.connected && input.browsing_context {
        let context = input
            .owner_context
            .ok_or("native Range owner realm unavailable")?;
        let viewport = context
            .get_slot::<Viewport>()
            .ok_or("native Range viewport unavailable")?;
        let view = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
        let state = scroll_state(context);
        let offsets = state.values.borrow();
        for selection in selections {
            let selected_source = match *selection {
                crate::live_layout_range::Selection::Element(source)
                | crate::live_layout_range::Selection::Text { source, .. } => source,
            };
            if view.omitted.contains(&selected_source) {
                continue;
            }
            let (source, mut boxes) = match *selection {
                crate::live_layout_range::Selection::Element(id) => {
                    (id, element_geometry(&view, id)?.rects)
                }
                crate::live_layout_range::Selection::Text { source, start, end } => {
                    let source_info = view
                        .snapshot
                        .source(source)
                        .ok_or("native Range source missing")?;
                    if source_info.excluded_from_layout {
                        continue;
                    }
                    if view.snapshot.unsupported_svg_source_geometry(source) {
                        return Err("native SVG text geometry is unsupported".into());
                    }
                    let boxes = view
                        .snapshot
                        .text_range_boxes(source, start, end)
                        .map_err(|error| error.to_string())?
                        .into_iter()
                        .map(|cell| cell.transformed_bounds)
                        .collect();
                    (source, boxes)
                }
            };
            if boxes.is_empty() {
                continue;
            }
            let translation = crate::live_layout_scroll::scroll_translation(
                &view.snapshot,
                &view.styles,
                source,
                &offsets,
            )?;
            for rect in &mut boxes {
                rect.x -= translation[0];
                rect.y -= translation[1];
            }
            rects.extend(boxes);
        }
    }
    let bounds = crate::live_layout_metrics::bounding_box(&rects);
    let values = |rect: turbo_html2pdf_core::layout_snapshot::Rect| {
        [rect.x, rect.y, rect.width, rect.height]
    };
    Ok(RangeGeometry {
        bounds: values(bounds),
        rects: rects.into_iter().map(values).collect(),
    })
}

#[op2(reentrant)]
pub(crate) fn op_live_caret_at_point<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    x: f64,
    y: f64,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    caret_at_point(scope, document, [x, y]).map_err(geometry_error)
}
fn caret_at_point<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    point: [f64; 2],
) -> Result<v8::Local<'s, v8::Value>, String> {
    let input = crate::browser_env::capture_native_document_layout(scope, document)?;
    if !input.browsing_context {
        return Ok(v8::null(scope).into());
    }
    let context = input
        .owner_context
        .ok_or("native caret owner realm unavailable")?;
    let viewport = context
        .get_slot::<Viewport>()
        .ok_or("native caret viewport unavailable")?;
    let view = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
    let state = scroll_state(context);
    let endpoint = crate::live_layout_text::caret(
        &view.snapshot,
        &view.styles,
        &state.values.borrow(),
        point,
    )?;
    let Some((source, offset, rect)) = endpoint else {
        return Ok(v8::null(scope).into());
    };
    let node = crate::browser_env::wrap_native_layout_elements(scope, document, &[source], 3)?
        .get_index(scope, 0)
        .ok_or("native caret source wrapping interrupted")?;
    let result = v8::Object::new(scope);
    let null = v8::null(scope);
    result.set_prototype(scope, null.into());
    let rect = deno_core::serde_v8::to_v8(scope, [rect.x, rect.y, rect.width, rect.height])
        .map_err(|error| error.to_string())?;
    let offset = v8::Integer::new_from_unsigned(scope, offset as u32);
    for (name, value) in [("node", node), ("offset", offset.into()), ("rect", rect)] {
        let name = v8::String::new(scope, name).ok_or("native caret record key interrupted")?;
        if result.create_data_property(scope, name.into(), value) != Some(true) {
            return Err("native caret record interrupted".into());
        }
    }
    Ok(result.into())
}

#[derive(Serialize)]
pub(crate) struct CaretGeometry {
    rect: [f32; 4],
}
#[op2(reentrant)]
#[serde]
pub(crate) fn op_live_caret_geometry<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    node: v8::Local<'s, v8::Object>,
    offset: u32,
) -> Result<CaretGeometry, JsErrorBox> {
    caret_geometry(scope, node, offset as usize)
        .map(|rect| CaretGeometry { rect })
        .map_err(geometry_error)
}
fn caret_geometry<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    node: v8::Local<'s, v8::Object>,
    offset: usize,
) -> Result<[f32; 4], String> {
    let input = crate::browser_env::capture_native_layout(scope, node)?;
    if !input.connected || !input.browsing_context {
        return Ok([0.0; 4]);
    }
    let context = input
        .owner_context
        .ok_or("native caret owner realm unavailable")?;
    let viewport = context
        .get_slot::<Viewport>()
        .ok_or("native caret viewport unavailable")?;
    let view = resolve(&input.tree, &input.sheets, viewport.as_ref())?;
    let source = crate::live_layout::source_id(input.tree.document_id, input.handle);
    let _current = view
        .snapshot
        .text_sources()
        .iter()
        .find(|text| text.source_id == source)
        .ok_or("native caret source text is unavailable")?;
    let units = input
        .tree
        .original_text
        .get(&source)
        .ok_or("native caret original text is unavailable")?;
    if offset > units.len()
        || units.is_empty()
        || view
            .snapshot
            .source(source)
            .is_some_and(|source| source.excluded_from_layout)
    {
        return Ok([0.0; 4]);
    }
    let boxes = view
        .snapshot
        .text_range_boxes(source, offset, offset)
        .map_err(|error| error.to_string())?;
    let rect = boxes
        .first()
        .ok_or("native caret source edge is unplaced")?
        .transformed_bounds;
    let state = scroll_state(context);
    let translation = crate::live_layout_scroll::scroll_translation(
        &view.snapshot,
        &view.styles,
        source,
        &state.values.borrow(),
    )?;
    Ok([
        rect.x - translation[0],
        rect.y - translation[1],
        rect.width,
        rect.height,
    ])
}

#[op2(nofast)]
pub(crate) fn op_live_media_match(
    scope: &mut v8::PinScope,
    #[string] source: &str,
) -> Result<bool, JsErrorBox> {
    let context = scope.get_current_context();
    let viewport = context
        .get_slot::<Viewport>()
        .ok_or_else(|| JsErrorBox::generic("Native media environment was not captured"))?;
    crate::live_stylesheets::Conditions::media(viewport.as_ref(), source)
        .map_err(JsErrorBox::generic)
}
