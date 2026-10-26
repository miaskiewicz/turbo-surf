//! Actual native layout behind CSSOM reads. Each read captures current native
//! tree/CSS state; no public getters or page-installed layout cache participates.
use crate::live_layout_media::{HostMedia, ViewportMedia};
use crate::live_layout_metrics::{ClientMetrics, OffsetSize};
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
use serde::Serialize;
use turbo_dom_layout::layout_snapshot::SnapshotInputs;
use turbo_dom_layout::{Diagnostics, FontRegistry, ImageCtx, NoImages};

// FontFace bytes and shaping caches are thread-safe and contain no V8 roots.
// Loading every installed font per isolate thread multiplies native memory.
static SYSTEM_FONTS: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();

// Native font discovery is process startup work, not part of a trusted input turn.
// Geometry installation primes this same immutable registry before page execution.
pub(crate) fn initialize_system_fonts() -> &'static FontRegistry {
    SYSTEM_FONTS.get_or_init(load_system_fonts)
}

fn load_system_fonts() -> FontRegistry {
    let mut registry = FontRegistry::new();
    registry.load_system_fonts_for_html();
    registry
}

pub(crate) async fn initialize_system_fonts_async() -> Result<(), String> {
    if SYSTEM_FONTS.get().is_some() {
        return Ok(());
    }
    // Cold discovery must leave a current-thread runtime free to retire pooled
    // connections while preserving the eager cache before any guest turn.
    tokio::task::spawn_blocking(|| {
        initialize_system_fonts();
    })
    .await
    .map_err(|error| format!("Native font initialization failed: {error}"))
}

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
    snapshot: turbo_dom_layout::layout_snapshot::LayoutSnapshot,
    styles: crate::live_layout_scroll::SourceStyles,
    omitted: std::collections::HashSet<turbo_dom_layout::layout_snapshot::SourceId>,
    quirks: bool,
    body: Option<turbo_dom_layout::layout_snapshot::SourceId>,
}
impl LayoutView {
    fn zoom(&self, id: turbo_dom_layout::layout_snapshot::SourceId) -> f32 {
        self.styles.get(&id).map_or(1.0, |entry| entry.zoom)
    }
    /// CSSOM View: the standards root and the quirks-mode body report the
    /// viewport's client size (excluding scrollbars), never their own box.
    fn viewport_client(&self, id: turbo_dom_layout::layout_snapshot::SourceId) -> bool {
        if self.quirks {
            self.body == Some(id)
        } else {
            crate::live_layout_scroll::is_root(&self.styles, id)
        }
    }
    /// Whether an element's scroll APIs address the viewport (the scrolling
    /// element): the standards root, or a quirks body that is not itself a
    /// scroll container.
    fn addresses_viewport(&self, id: turbo_dom_layout::layout_snapshot::SourceId) -> bool {
        use crate::live_layout_scroll as scrolling;
        if self.quirks {
            self.body == Some(id) && !scrolling::element_scroller(&self.styles, id)
        } else {
            scrolling::is_root(&self.styles, id)
        }
    }
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

const VIEWPORT_PENDING: &str = "native layout viewport was not captured";

fn viewport<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<std::rc::Rc<Viewport>, String> {
    viewport_at_depth(scope, context, 0)
}

fn viewport_at_depth<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    depth: usize,
) -> Result<std::rc::Rc<Viewport>, String> {
    if depth > 12 {
        return Err("native frame viewport hierarchy exceeds its depth limit".into());
    }
    let current = context.get_slot::<Viewport>().ok_or(VIEWPORT_PENDING)?;
    let [width, height] = if crate::runtime::native_frame_is_retired(context) {
        [0.0; 2]
    } else if let Some(host) = crate::runtime::native_frame_layout_host(scope, context) {
        frame_content_size(scope, host, depth + 1)?
    } else {
        return Ok(current);
    };
    if current.width == width && current.height == height {
        return Ok(current);
    }
    let mut updated = *current;
    updated.width = width;
    updated.height = height;
    let updated = std::rc::Rc::new(updated);
    context.set_slot(updated.clone());
    Ok(updated)
}

fn frame_content_size<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host: v8::Local<'s, v8::Object>,
    depth: usize,
) -> Result<[f32; 2], String> {
    let input = crate::live_layout_dom::capture(scope, host)?;
    if !input.connected || !input.browsing_context {
        return Ok([0.0; 2]);
    }
    let context = input
        .owner_context
        .ok_or("frame layout owner unavailable")?;
    let viewport = viewport_at_depth(scope, context, depth)?;
    let view = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
    let id = crate::live_layout::source_id(input.tree.document_id, input.handle);
    let source = view
        .snapshot
        .source(id)
        .ok_or("frame layout source unavailable")?;
    if source.excluded_from_layout || view.omitted.contains(&id) {
        return Ok([0.0; 2]);
    }
    let used = source
        .fragment_indices
        .iter()
        .find_map(|&index| {
            let fragment = &view.snapshot.fragments()[index];
            (fragment.kind == turbo_dom_layout::layout_snapshot::GeometryKind::BorderBox)
                .then_some(fragment.used_box)
                .flatten()
        })
        .ok_or("frame content box unavailable")?;
    // Child browsing-context viewports are snapped to the parent's layout pixel
    // grid; transforms and border/padding remain outside that content surface.
    let snap = |value: f32| (value * viewport.layout_scale + 0.5).floor() / viewport.layout_scale;
    Ok([snap(used.content_width), snap(used.content_height)])
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
    let input = crate::live_layout_dom::capture(scope, receiver)?;
    let geometry = if !input.connected || !input.browsing_context {
        crate::live_layout_metrics::empty_geometry()
    } else {
        let context = input
            .owner_context
            .ok_or("native layout owner realm unavailable")?;
        let viewport = viewport(scope, context)?;
        let snapshot = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
        let id = crate::live_layout::source_id(input.tree.document_id, input.handle);
        let mut geometry = element_geometry(&snapshot, id)?;
        unzoom_metrics(&mut geometry, snapshot.zoom(id));
        if snapshot.viewport_client(id) {
            geometry.client.width = viewport.width.floor() as i32;
            geometry.client.height = viewport.height.floor() as i32;
        }
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
    let rect = |r: turbo_dom_layout::layout_snapshot::Rect| [r.x, r.y, r.width, r.height];
    Ok(Geometry {
        bounds: rect(geometry.bounds),
        rects: geometry.rects.into_iter().map(rect).collect(),
        client: geometry.client,
        offset: geometry.offset,
    })
}
/// Offset and client metrics report an element's own unzoomed CSS pixels: the
/// pixel-snapped layout value divided by its effective zoom, then rounded.
fn unzoom_metrics(geometry: &mut crate::live_layout_metrics::ElementGeometry, zoom: f32) {
    if zoom == 1.0 {
        return;
    }
    let client = &mut geometry.client;
    for value in [
        &mut client.left,
        &mut client.top,
        &mut client.width,
        &mut client.height,
        &mut geometry.offset.width,
        &mut geometry.offset.height,
    ] {
        *value = unzoom(*value, zoom);
    }
}
fn unzoom(value: i32, zoom: f32) -> i32 {
    (f64::from(value) / f64::from(zoom)).round() as i32
}
/// Blink's AdjustForAbsoluteZoom::AdjustInt for integral content sizes: zooming
/// in compensates for truncation before dividing.
fn adjust_int(value: i32, zoom: f32) -> i32 {
    if zoom == 1.0 {
        return value;
    }
    let value = match (zoom > 1.0, value < 0) {
        (false, _) => value,
        (true, true) => value - 1,
        (true, false) => value + 1,
    };
    (f64::from(value) / f64::from(zoom)) as i32
}
fn element_geometry(
    view: &LayoutView,
    id: turbo_dom_layout::layout_snapshot::SourceId,
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
    scope: &v8::PinScope<'_, '_>,
    tree: &crate::live_layout::LiveTreeInput,
    sheets: &[crate::live_stylesheets::SheetSnapshot],
    viewport: &Viewport,
) -> Result<std::rc::Rc<LayoutView>, String> {
    let key_timer =
        crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::RulesKey);
    let profile = crate::layout_profile::LayoutProfile::current(scope);
    let rules = crate::live_stylesheets::active_rule_snapshots(sheets, viewport)?;
    let fonts = initialize_system_fonts();
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
    drop(key_timer);
    if let Some(profile) = profile {
        profile.cache(cached.is_some());
    }
    let snapshot = if let Some(snapshot) = cached {
        snapshot
    } else {
        let layout_timer =
            crate::layout_profile::Timer::new(scope, crate::layout_profile::Stage::Layout);
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
        drop(layout_timer);
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
            quirks: tree.quirks,
            body: tree.body,
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
        std::collections::BTreeMap<turbo_dom_layout::layout_snapshot::SourceId, [f64; 2]>,
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
    /// 0: unchanged, 1: the element's own box, 2: the viewport.
    changed: u32,
}
#[op2(reentrant)]
#[serde]
pub(crate) fn op_live_element_scroll<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    axis: u32,
    value: f64,
) -> Result<ScrollResult, JsErrorBox> {
    // Bit 4 addresses the viewport through the root (the Window scroll members).
    let target = match axis & 3 {
        1 => [Some(value), None],
        2 => [None, Some(value)],
        _ => [None; 2],
    };
    scroll(scope, receiver, target, false, axis & 4 != 0).map_err(geometry_error)
}
/// Scroll an element (or, for the root, the viewport) to absolute or relative
/// offsets. NaN keeps the current axis; other values are already finite.
#[op2(reentrant)]
#[serde]
pub(crate) fn op_live_element_scroll_to<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    x: f64,
    y: f64,
    relative: bool,
    viewport: bool,
) -> Result<ScrollResult, JsErrorBox> {
    let axis = |value: f64| (!value.is_nan()).then_some(value);
    scroll(scope, receiver, [axis(x), axis(y)], relative, viewport).map_err(geometry_error)
}
struct ScrollView<'s> {
    context: v8::Local<'s, v8::Context>,
    viewport: std::rc::Rc<Viewport>,
    view: std::rc::Rc<LayoutView>,
    id: turbo_dom_layout::layout_snapshot::SourceId,
}
fn scroll_view<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<Option<ScrollView<'s>>, String> {
    // A realm still running trusted bootstrap (for example a new frame probing
    // window members) has no layout viewport yet: it observes the initial origin
    // and must not capture another document's layout mid-initialization.
    if scope
        .get_entered_or_microtask_context()
        .get_slot::<Viewport>()
        .is_none()
    {
        return Ok(None);
    }
    let input = crate::live_layout_dom::capture(scope, receiver)?;
    if !input.connected || !input.browsing_context {
        return Ok(None);
    }
    let context = input
        .owner_context
        .ok_or("native layout owner realm unavailable")?;
    // An ancestor frame realm can also still be bootstrapping its viewport.
    let viewport = match viewport(scope, context) {
        Err(error) if error == VIEWPORT_PENDING => return Ok(None),
        result => result?,
    };
    let view = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
    let id = crate::live_layout::source_id(input.tree.document_id, input.handle);
    Ok((!view.omitted.contains(&id)).then_some(ScrollView {
        context,
        viewport,
        view,
        id,
    }))
}
/// The scrolling box an element's scroll APIs address, with its current extent.
fn scroller(
    view: &LayoutView,
    viewport: &Viewport,
    id: turbo_dom_layout::layout_snapshot::SourceId,
) -> Result<
    (
        turbo_dom_layout::layout_snapshot::SourceId,
        crate::live_layout_scroll::ScrollMetrics,
    ),
    String,
> {
    use crate::live_layout_scroll as scrolling;
    if view.addresses_viewport(id) {
        return viewport_scroller(view, viewport);
    }
    let mut metrics = scrolling::scroll_metrics(&view.snapshot, &view.styles, id)?;
    // A propagated overflow belongs to the viewport, never the element's own box;
    // a quirks root always propagates its overflow and is reached only as itself.
    if scrolling::propagated_body(&view.styles, id)
        || view.quirks && scrolling::is_root(&view.styles, id)
    {
        metrics.scroll_x = false;
        metrics.scroll_y = false;
    }
    Ok((id, metrics))
}
/// The viewport scrolling box, whose extent is the initial containing block plus
/// the root's non-fixed scrollable overflow.
fn viewport_scroller(
    view: &LayoutView,
    viewport: &Viewport,
) -> Result<
    (
        turbo_dom_layout::layout_snapshot::SourceId,
        crate::live_layout_scroll::ScrollMetrics,
    ),
    String,
> {
    use crate::live_layout_scroll as scrolling;
    let root = scroll_root(view).ok_or("native viewport root unavailable")?;
    let size = [viewport.width, viewport.height];
    let metrics = scrolling::viewport_metrics(&view.snapshot, &view.styles, root, size)?;
    Ok((scrolling::VIEWPORT, metrics))
}
fn snap(viewport: &Viewport, value: f64) -> f64 {
    (value * viewport.device_scale + 0.5).floor() / viewport.device_scale
}
/// Clamp retained state to the current extent, then apply requested positions.
fn apply_scroll(
    values: &mut std::collections::BTreeMap<turbo_dom_layout::layout_snapshot::SourceId, [f64; 2]>,
    key: turbo_dom_layout::layout_snapshot::SourceId,
    metrics: &crate::live_layout_scroll::ScrollMetrics,
    viewport: &Viewport,
    target: [Option<f64>; 2],
    relative: bool,
) -> ([f64; 2], bool) {
    let offset = values.entry(key).or_insert([0.0; 2]);
    let maximum = [snap(viewport, metrics.max_x), snap(viewport, metrics.max_y)];
    let enabled = [metrics.scroll_x, metrics.scroll_y];
    let mut changed = false;
    for i in 0..2 {
        offset[i] = if enabled[i] {
            offset[i].clamp(0.0, maximum[i])
        } else {
            0.0
        };
        let Some(value) = target[i].filter(|_| enabled[i]) else {
            continue;
        };
        let value = if relative { offset[i] + value } else { value };
        let next = if value.is_finite() {
            snap(viewport, value.clamp(0.0, maximum[i])).clamp(0.0, maximum[i])
        } else {
            0.0
        };
        changed |= next != offset[i];
        offset[i] = next;
    }
    (*offset, changed)
}
fn scroll<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    target: [Option<f64>; 2],
    relative: bool,
    viewport: bool,
) -> Result<ScrollResult, String> {
    let Some(current) = scroll_view(scope, receiver)? else {
        return Ok(ScrollResult {
            width: 0,
            height: 0,
            left: 0.0,
            top: 0.0,
            changed: 0,
        });
    };
    let (key, metrics) = if viewport {
        viewport_scroller(&current.view, &current.viewport)?
    } else {
        scroller(&current.view, &current.viewport, current.id)?
    };
    // Element scroll offsets and extents are in the scroller's own unzoomed CSS
    // pixels; the viewport uses the (unit) page zoom.
    let zoom = if key == crate::live_layout_scroll::VIEWPORT {
        1.0
    } else {
        current.view.zoom(key)
    };
    let target = target.map(|value| value.map(|value| value * f64::from(zoom)));
    let state = scroll_state(current.context);
    let mut values = state.values.borrow_mut();
    values.retain(|source, _| {
        *source == crate::live_layout_scroll::VIEWPORT || current.view.styles.contains_key(source)
    });
    let (offset, changed) = apply_scroll(
        &mut values,
        key,
        &metrics,
        &current.viewport,
        target,
        relative,
    );
    Ok(ScrollResult {
        width: unzoom(metrics.width, zoom),
        height: unzoom(metrics.height, zoom),
        left: offset[0] / f64::from(zoom),
        top: offset[1] / f64::from(zoom),
        changed: match (changed, key == crate::live_layout_scroll::VIEWPORT) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        },
    })
}

/// The window's retained viewport offset, for reads where the current document
/// cannot be laid out natively. It was clamped and snapped when it was set.
#[op2]
#[serde]
pub(crate) fn op_live_viewport_offset<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
) -> Result<Vec<f64>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .filter(|context| window.strict_equals(context.global(scope).into()))
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let state = scroll_state(context);
    let offset = state
        .values
        .borrow()
        .get(&crate::live_layout_scroll::VIEWPORT)
        .copied()
        .unwrap_or([0.0; 2]);
    Ok(offset.to_vec())
}

/// Scroll every ancestor scrolling box, innermost first, to expose an element.
/// Returns the changed element scrollers and whether the viewport moved.
#[op2(reentrant)]
pub(crate) fn op_live_scroll_into_view<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    block: u32,
    inline: u32,
    mode: u32,
    nearest: bool,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    scroll_into_view(scope, receiver, [inline, block], mode, nearest).map_err(geometry_error)
}
fn scroll_into_view<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    alignments: [u32; 2],
    mode: u32,
    nearest: bool,
) -> Result<v8::Local<'s, v8::Value>, String> {
    use crate::live_layout_scroll as scrolling;
    let behaviors = [
        scrolling::alignment(alignments[0], mode)?,
        scrolling::alignment(alignments[1], mode)?,
    ];
    let mut changed = Vec::new();
    let mut viewport_changed = false;
    if let Some(current) = scroll_view(scope, receiver)? {
        let view = &current.view;
        let boxed = element_geometry(view, current.id)?;
        if !boxed.rects.is_empty() {
            let state = scroll_state(current.context);
            for scroller_id in into_view_scrollers(view, current.id, nearest)? {
                let mut values = state.values.borrow_mut();
                let translation = scrolling::scroll_translation(
                    &view.snapshot,
                    &view.styles,
                    current.id,
                    &values,
                )?;
                let bounds = boxed.bounds;
                let world = [
                    f64::from(bounds.x - translation[0]),
                    f64::from(bounds.y - translation[1]),
                    f64::from(bounds.width),
                    f64::from(bounds.height),
                ];
                let (key, metrics, port, local) = if scroller_id == scrolling::VIEWPORT {
                    let (key, metrics) = viewport_scroller(view, &current.viewport)?;
                    let size = [
                        f64::from(current.viewport.width),
                        f64::from(current.viewport.height),
                    ];
                    (key, metrics, [0.0, 0.0, size[0], size[1]], world)
                } else {
                    let (key, metrics) = scroller(view, &current.viewport, scroller_id)?;
                    let (port, local) = scroller_space(view, scroller_id, world, &values)?;
                    (key, metrics, port, local)
                };
                let offset = values.get(&key).copied().unwrap_or([0.0; 2]);
                let mut target = [None; 2];
                for axis in 0..2 {
                    let start = local[axis] - port[axis] + offset[axis];
                    target[axis] = Some(scrolling::expose(
                        start,
                        local[axis + 2],
                        offset[axis],
                        port[axis + 2],
                        behaviors[axis],
                    ));
                }
                let (_, moved) =
                    apply_scroll(&mut values, key, &metrics, &current.viewport, target, false);
                if moved && key == scrolling::VIEWPORT {
                    viewport_changed = true;
                } else if moved {
                    changed.push(key);
                }
            }
        }
    }
    let elements = crate::live_layout_dom::wrap_elements(scope, receiver, &changed, 1)?;
    let result = v8::Object::new(scope);
    let null = v8::null(scope);
    if result.set_prototype(scope, null.into()) != Some(true) {
        return Err("native scroll record interrupted".into());
    }
    let viewport = v8::Boolean::new(scope, viewport_changed);
    for (name, value) in [("elements", elements.into()), ("viewport", viewport.into())] {
        let name = v8::String::new(scope, name).ok_or("native scroll record key interrupted")?;
        if result.create_data_property(scope, name.into(), value) != Some(true) {
            return Err("native scroll record interrupted".into());
        }
    }
    Ok(result.into())
}
fn scroll_root(view: &LayoutView) -> Option<turbo_dom_layout::layout_snapshot::SourceId> {
    view.styles
        .iter()
        .find(|(_, entry)| entry.parent.is_none())
        .map(|(id, _)| *id)
}
/// Ancestor scrolling boxes along the containing-block chain, then the viewport.
fn into_view_scrollers(
    view: &LayoutView,
    id: turbo_dom_layout::layout_snapshot::SourceId,
    nearest: bool,
) -> Result<Vec<turbo_dom_layout::layout_snapshot::SourceId>, String> {
    use crate::live_layout_scroll as scrolling;
    use turbo_dom_layout::layout::value::Position;
    let source = view
        .snapshot
        .source(id)
        .ok_or("native scroll source missing")?;
    let fragment = source
        .fragment_indices
        .iter()
        .map(|&i| &view.snapshot.fragments()[i])
        .find(|fragment| {
            fragment.kind == turbo_dom_layout::layout_snapshot::GeometryKind::BorderBox
        })
        .or_else(|| {
            view.snapshot
                .text_runs()
                .iter()
                .find(|run| run.source_id == Some(id) || run.source_ancestors.contains(&id))
                .map(|run| &view.snapshot.fragments()[run.fragment_index])
        })
        .ok_or("native scroll requires placed source provenance")?;
    let position = fragment.used_box.map(|used| used.position);
    let ancestors = &fragment.source_ancestors;
    let positioned = |ancestor: &turbo_dom_layout::layout_snapshot::SourceId, fixed: bool| {
        view.styles.get(ancestor).is_some_and(|entry| {
            entry.style.get("display") != Some("contents")
                && entry.style.get("position").is_some_and(|value| {
                    if fixed {
                        value.trim() == "fixed"
                    } else {
                        value.trim() != "static"
                    }
                })
        })
    };
    let mut result = Vec::new();
    if position != Some(Position::Fixed) {
        let containing = if position == Some(Position::Absolute) {
            ancestors
                .iter()
                .rposition(|ancestor| *ancestor != id && positioned(ancestor, false))
        } else {
            Some(ancestors.len())
        };
        for (i, ancestor) in ancestors.iter().enumerate().rev() {
            if *ancestor == id || containing.is_none_or(|index| i > index) {
                continue;
            }
            if scrolling::element_scroller(&view.styles, *ancestor) {
                result.push(*ancestor);
            }
        }
        if !ancestors
            .iter()
            .any(|ancestor| *ancestor != id && positioned(ancestor, true))
        {
            result.push(scrolling::VIEWPORT);
        }
    }
    if nearest {
        result.truncate(1);
    }
    Ok(result)
}
/// Map an actual world rectangle into a scroller's local layout space, returning
/// its scrollport (padding box) and the rectangle's local bounding box.
fn scroller_space(
    view: &LayoutView,
    id: turbo_dom_layout::layout_snapshot::SourceId,
    world: [f64; 4],
    offsets: &std::collections::BTreeMap<turbo_dom_layout::layout_snapshot::SourceId, [f64; 2]>,
) -> Result<([f64; 4], [f64; 4]), String> {
    let source = view
        .snapshot
        .source(id)
        .ok_or("native scroll source missing")?;
    let fragment = source
        .fragment_indices
        .iter()
        .map(|&i| &view.snapshot.fragments()[i])
        .find(|fragment| {
            fragment.kind == turbo_dom_layout::layout_snapshot::GeometryKind::BorderBox
        })
        .ok_or("native scroll requires inline border provenance")?;
    let used = fragment.used_box.ok_or("native scroll used box missing")?;
    let padding = used.padding_rect(fragment.rect);
    let matrix = view
        .snapshot
        .native_world_transform(fragment.index)
        .ok_or("native scroll transform unavailable")?;
    let determinant = matrix[0] * matrix[3] - matrix[1] * matrix[2];
    if determinant == 0.0 || !determinant.is_finite() {
        return Err(
            "native scroll degenerate-container relative transform provenance pending".into(),
        );
    }
    // The scroller itself moves with its ancestors' offsets; undo that first.
    let shift =
        crate::live_layout_scroll::scroll_translation(&view.snapshot, &view.styles, id, offsets)?;
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in [
        (world[0], world[1]),
        (world[0] + world[2], world[1]),
        (world[0], world[1] + world[3]),
        (world[0] + world[2], world[1] + world[3]),
    ] {
        let x = x + f64::from(shift[0]) - matrix[4];
        let y = y + f64::from(shift[1]) - matrix[5];
        let local = [
            (matrix[3] * x - matrix[2] * y) / determinant,
            (-matrix[1] * x + matrix[0] * y) / determinant,
        ];
        bounds = [
            bounds[0].min(local[0]),
            bounds[1].min(local[1]),
            bounds[2].max(local[0]),
            bounds[3].max(local[1]),
        ];
    }
    Ok((
        [
            f64::from(padding.x),
            f64::from(padding.y),
            f64::from(padding.width),
            f64::from(padding.height),
        ],
        [
            bounds[0],
            bounds[1],
            bounds[2] - bounds[0],
            bounds[3] - bounds[1],
        ],
    ))
}

/// Whether an element has a layout box (Blink's layout object): display:none
/// subtrees, display:contents and unassigned slottables have none.
fn has_box(view: &LayoutView, id: turbo_dom_layout::layout_snapshot::SourceId) -> bool {
    view.snapshot
        .source(id)
        .is_some_and(|source| !source.excluded_from_layout)
        && view
            .styles
            .get(&id)
            .is_some_and(|entry| entry.style.get("display") != Some("contents"))
}
/// Whether an element's own box is a scroll container (overflow not propagated
/// to the viewport); a quirks document then has no scrolling element.
#[op2(fast, reentrant)]
pub(crate) fn op_live_scroll_container<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<bool, JsErrorBox> {
    let Some(current) = scroll_view(scope, receiver).map_err(geometry_error)? else {
        return Ok(false);
    };
    Ok(has_box(&current.view, current.id)
        && crate::live_layout_scroll::element_scroller(&current.view.styles, current.id))
}
/// `Element.currentCSSZoom`: the effective zoom of an element with a layout box,
/// otherwise 1.
#[op2(fast, reentrant)]
pub(crate) fn op_live_element_zoom<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Result<f64, JsErrorBox> {
    let Some(current) = scroll_view(scope, receiver).map_err(geometry_error)? else {
        return Ok(1.0);
    };
    Ok(if has_box(&current.view, current.id) {
        f64::from(current.view.zoom(current.id))
    } else {
        1.0
    })
}
/// A rendered input's content-box width or height in its own unzoomed CSS
/// pixels (Blink's ImageInputType::Width/Height layout path), or null when the
/// element has no layout box.
#[op2(reentrant)]
pub(crate) fn op_live_input_size<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    height: bool,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    Ok(
        match input_size(scope, receiver, height).map_err(geometry_error)? {
            Some(value) => v8::Integer::new(scope, value).into(),
            None => v8::null(scope).into(),
        },
    )
}
fn input_size<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    height: bool,
) -> Result<Option<i32>, String> {
    let Some(current) = scroll_view(scope, receiver)? else {
        return Ok(None);
    };
    let view = &current.view;
    if !has_box(view, current.id) {
        return Ok(None);
    }
    let source = view
        .snapshot
        .source(current.id)
        .ok_or("native input layout source missing")?;
    let used = source
        .fragment_indices
        .iter()
        .find_map(|&index| {
            let fragment = &view.snapshot.fragments()[index];
            (fragment.kind == turbo_dom_layout::layout_snapshot::GeometryKind::BorderBox)
                .then_some(fragment.used_box)
                .flatten()
        })
        .ok_or("native input content box unavailable")?;
    let content = if height {
        used.content_height
    } else {
        used.content_width
    };
    // LayoutUnit::ToInt truncates before the zoom adjustment.
    Ok(Some(adjust_int(content as i32, view.zoom(current.id))))
}
/// CSSOM View checkVisibility over the actual box tree and computed styles.
#[op2(fast, reentrant)]
pub(crate) fn op_live_element_visibility<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    flags: u32,
) -> Result<bool, JsErrorBox> {
    visibility(scope, receiver, flags).map_err(geometry_error)
}
fn visibility<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    flags: u32,
) -> Result<bool, String> {
    let Some(current) = scroll_view(scope, receiver)? else {
        return Ok(false);
    };
    let view = &current.view;
    let has_box = view
        .snapshot
        .source(current.id)
        .is_some_and(|source| !source.excluded_from_layout)
        && view
            .styles
            .get(&current.id)
            .is_some_and(|entry| entry.style.get("display") != Some("contents"));
    if !has_box {
        return Ok(false);
    }
    let relevant = |ancestor| -> Result<bool, String> {
        let state = scroll_state(current.context);
        let geometry = element_geometry(view, ancestor)?;
        let translation = crate::live_layout_scroll::scroll_translation(
            &view.snapshot,
            &view.styles,
            ancestor,
            &state.values.borrow(),
        )?;
        Ok(crate::live_layout_visibility::near_viewport(
            geometry.bounds,
            translation,
            [current.viewport.width, current.viewport.height],
        ))
    };
    crate::live_layout_visibility::check(&view.styles, current.id, flags, relevant)
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
    // Frames can bootstrap while their owner's geometry capability is still
    // being installed. Actual content sizes are resolved by viewport() at every
    // observable read; initialization must not perform the parent's layout.
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
pub(crate) fn op_live_frame_dimension<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    height: bool,
    read: bool,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    Ok(match frame_dimension(scope, receiver, height, read)? {
        Some(value) => v8::Number::new(scope, value).into(),
        None => v8::null(scope).into(),
    })
}

fn frame_dimension<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    height: bool,
    read: bool,
) -> Result<Option<f64>, JsErrorBox> {
    let context = receiver
        .get_creation_context(scope)
        .filter(|context| receiver.strict_equals(context.global(scope).into()))
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let caller = scope.get_entered_or_microtask_context();
    if !caller
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope))
    {
        return Ok(None);
    }
    if !read {
        return Ok(Some(0.0));
    }
    let value = viewport(scope, context).map_err(JsErrorBox::generic)?;
    Ok(Some(
        f64::from(if height { value.height } else { value.width }).floor(),
    ))
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
    let input = crate::live_layout_dom::capture_document(scope, document)?;
    if !input.browsing_context {
        return Ok(v8::Array::new(scope, 0));
    }
    let context = input
        .owner_context
        .ok_or("native layout owner realm unavailable")?;
    let viewport = viewport(scope, context)?;
    let snapshot = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
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
    crate::live_layout_dom::wrap_elements(scope, document, &ids, 1)
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
    let input = crate::live_layout_dom::capture(scope, receiver)?;
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
        let viewport = viewport(scope, context)?;
        let snapshot = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
        let id = crate::live_layout::source_id(input.tree.document_id, input.handle);
        if snapshot.omitted.contains(&id) {
            crate::live_layout_metrics::OffsetPosition {
                left: 0,
                top: 0,
                parent: None,
            }
        } else {
            let mut position = crate::live_layout_metrics::offset_position(
                &snapshot.snapshot,
                &snapshot.styles,
                id,
            )?;
            let zoom = f64::from(snapshot.zoom(id));
            for value in [&mut position.left, &mut position.top] {
                *value = (f64::from(*value) / zoom).round() as i32;
            }
            position
        }
    };
    let parent = if let Some(parent) = position.parent {
        crate::live_layout_dom::wrap_elements(scope, receiver, &[parent], 1)?
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
    let input = crate::live_layout_dom::capture(scope, receiver)?;
    let selections = input.range.as_ref().ok_or("Illegal invocation")?;
    let mut rects = Vec::new();
    if input.connected && input.browsing_context {
        let context = input
            .owner_context
            .ok_or("native Range owner realm unavailable")?;
        let viewport = viewport(scope, context)?;
        let view = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
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
    let values =
        |rect: turbo_dom_layout::layout_snapshot::Rect| [rect.x, rect.y, rect.width, rect.height];
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
    let input = crate::live_layout_dom::capture_document(scope, document)?;
    if !input.browsing_context {
        return Ok(v8::null(scope).into());
    }
    let context = input
        .owner_context
        .ok_or("native caret owner realm unavailable")?;
    let viewport = viewport(scope, context)?;
    let view = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
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
    let node = crate::live_layout_dom::wrap_elements(scope, document, &[source], 3)?
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
    let input = crate::live_layout_dom::capture(scope, node)?;
    if !input.connected || !input.browsing_context {
        return Ok([0.0; 4]);
    }
    let context = input
        .owner_context
        .ok_or("native caret owner realm unavailable")?;
    let viewport = viewport(scope, context)?;
    let view = resolve(scope, &input.tree, &input.sheets, viewport.as_ref())?;
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

#[op2(nofast, reentrant)]
pub(crate) fn op_live_media_match<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    #[string] source: &str,
) -> Result<bool, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .filter(|context| window.strict_equals(context.global(scope).into()))
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let viewport = viewport(scope, context).map_err(JsErrorBox::generic)?;
    crate::live_stylesheets::Conditions::media(viewport.as_ref(), source)
        .map_err(JsErrorBox::generic)
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn release_initializer_after_network_progress(
        listener: tokio::net::TcpListener,
        release: std::sync::mpsc::SyncSender<()>,
    ) {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let address = listener.local_addr().unwrap();
        let client = async {
            let mut socket = tokio::net::TcpStream::connect(address).await?;
            socket.write_all(&[7]).await?;
            let mut reply = [0];
            socket.read_exact(&mut reply).await?;
            assert_eq!(reply, [9]);
            Ok::<_, std::io::Error>(())
        };
        let server = async {
            let (mut socket, _) = listener.accept().await?;
            let mut request = [0];
            socket.read_exact(&mut request).await?;
            assert_eq!(request, [7]);
            socket.write_all(&[9]).await?;
            Ok::<_, std::io::Error>(())
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::try_join!(client, server)
        })
        .await
        .expect("session startup prevented the network driver from progressing")
        .unwrap();
        release.send(()).unwrap();
    }

    fn cold_session_startup() {
        assert!(
            SYSTEM_FONTS.get().is_none(),
            "subprocess font cache was warm"
        );
        let (started, initialization_started) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let initializer = std::thread::spawn(move || {
            SYSTEM_FONTS.get_or_init(|| {
                started.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(2)).expect(
                    "PageSession blocked its current-thread network driver during font startup",
                );
                load_system_fonts()
            });
        });
        initialization_started.recv().unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let network = tokio::spawn(release_initializer_after_network_progress(
                    listener, release,
                ));
                let mut page = crate::PageSession::open(
                    "<body>cold startup</body>",
                    "https://startup.invalid/",
                    "",
                    "",
                    5000,
                )
                .await
                .unwrap();
                network.await.unwrap();
                assert_eq!(
                    page.eval("globalThis.__RESULT=document.body.textContent")
                        .await
                        .unwrap(),
                    "cold startup"
                );
                page.close();
            });
        initializer.join().unwrap();
        assert!(SYSTEM_FONTS.get().is_some());
    }

    #[test]
    fn cold_session_startup_keeps_current_thread_network_driven() {
        const CHILD: &str = "TURBO_SURF_FONT_STARTUP_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            cold_session_startup();
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "live_layout_backend::startup_tests::cold_session_startup_keeps_current_thread_network_driven",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cold startup subprocess failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
