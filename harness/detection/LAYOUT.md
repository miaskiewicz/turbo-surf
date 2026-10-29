# Native browser geometry feasibility audit

Read-only audit, 2026-10-13. No geometry implementation or parity claim.

> Paths below are as of the audit. Since the turbo-dom consolidation the raster
> adapter lives in turbo-dom's `turbo-dom-raster` crate and the html2pdf layout
> half (incl. `html_layout.rs`) in `turbo-dom-layout`; turbo-surf no longer
> depends on turbo-html2pdf.

## What exists

- Surf `rust/crates/turbo-surf-raster/src/lib.rs:350` has a private `lay_out` adapter using real turbo-html2pdf CSS/layout/fonts/image sizing. The public APIs return screenshots, not a stable browser geometry index. The adapter rewrites lazy images, strips non-visual markup, concatenates sheets in screenshot order, and uses no root percentage-height basis. Reusing it unchanged would change page semantics.
- Html2pdf `crates/turbo-html2pdf-core/src/html_layout.rs:176` exposes `layout_boxes`, returning border boxes identified by `data-cid`. It is explicitly a conformance/debug seam. Inline elements flatten into anonymous runs; only the first outer fragment is reported. Injecting attributes into a snapshot can itself alter attribute selectors, so this is unsuitable as the DOM identity bridge.
- Public `layout_with_images`, `layout::boxgen::build_box_tree`, `resolve_box_style`, `Fragment`, `FragmentContent`, and `Fragment::paint_order` provide actual layout, border/padding styles, transforms, coordinates, and basic paint ordering.
- DOM `rtdom::Tree.version` is public and advances on every mutation (`turbo-dom/crates/turbo-dom/src/rtdom/tree.rs:480`). The canonical V8 binding has private node/tree/context ownership. Current binding `el_get_bounding_client_rect` returns an ordinary zero-valued object, and runtime offset dimensions measure text rather than boxes.

## Reusable native seam

Add an Html2pdf entry accepting an immutable live DOM snapshot with an external source identity map. Keep source IDs out of DOM attributes and CSS matching. Preserve IDs through styling, box generation, anonymous inline flattening, and line splitting. Return a `LayoutSnapshot` indexing all fragments by source ID, with border/content/padding boxes, overflow extents, ancestor clips, transforms, paint order, and source text ranges. Existing PDF callers retain their current entry points.

Use a canonical turbo-test host-only layout-provider seam for geometry methods, then vendor committed source. Native callbacks derive the real node/tree/document from private records; no page getter or guest-supplied node ID selects host data. Surf owns the provider and immutable snapshots.

The cache key is document/tree identity + `Tree.version` + stylesheet/CSSOM epoch + viewport/frame-content size epoch + image/font epoch. Scroll has a separate coordinate generation. Intrinsic live form values, focus/pseudo-state and private control state also need explicit invalidation; serializing HTML attributes alone loses current control values. Geometry reads synchronously flush dirty native layout.

## Algorithms and boundaries

- Client rects retain each generated border fragment; bounding rects union transformed fragments and subtract document scrolling. Do not clip these rectangles merely because they are outside the viewport.
- Offset dimensions/offset-parent positions use untransformed border boxes and browser rounding. Client dimensions subtract borders and real scrollbar space. Scroll dimensions require actual overflow extents and persistent scroll state; they are not aliases of bounding dimensions.
- Hit testing traverses reverse paint order, inverts transforms, applies ancestor clips/visibility/pointer-events, and performs shadow retargeting. Parent-document hits return the iframe element; a child uses its own content viewport and local coordinates.
- Each frame needs a distinct layout snapshot and content viewport derived from its parent's actual frame box. Frame borders/padding and document scroll participate when composing coordinates across frames. Navigation invalidates source ownership.
- Caret/Range geometry needs text source provenance, whitespace-to-original-offset mapping, UTF-16 offsets, bidi/ligature affinity, and glyph clusters. `FontFace::shape` exposes clusters, but current `FragmentContent::TextLine` retains only rendered text and positioned glyphs; `StyledNode::Text` loses DOM identity. Nearest-character arithmetic would be a placeholder.

## Remaining layout-engine work

`layout/value.rs:552` explicitly treats sticky as relative because snapshot/PDF layout has no scroll containers. Fragment output lacks scrollbars, overflow/clipping and complete inline origin provenance. Browser viewport percentage heights, fixed positioning, native controls, shadow/composed layout, writing modes, web-font loading/fallback and complete stacking/hit policies require additional engine behavior and measurements.

First validate real block/replaced geometry and transforms with authored Chrome fixtures and mutation invalidation. Then add inline fragments, overflow/scrolling, frame boxes, hit testing and text provenance/caret tests. A block-only provider would close its measured subset, not full browser geometry.
