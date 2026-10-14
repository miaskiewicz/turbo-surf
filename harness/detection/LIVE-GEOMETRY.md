# Native live geometry

The renderer uses the published `turbo-html2pdf-core 0.3.5` live-tree snapshot API.
It copies the actual native DOM through public rtdom methods and assigns external
source identities. Native CSSOM exports supply current inline, linked, imported
and adopted rules, media and disabled state. The bridge does not serialize or
rewrite HTML, add identity attributes, or call page-owned text/style/layout getters.
Its cache retains at most four native document snapshots and no JavaScript roots.

Implemented behavior includes genuine per-realm DOMRect/DOMRectReadOnly/DOMRectList
interfaces and private scalar structured clones; block/replaced border rectangles,
transforms, client/offset dimensions and actual offset parents; element scroll
extents, clamping and captured device-pixel snapping; ancestor-scroll projection;
and Document point-hit methods over native transformed border regions, clipping,
visibility, pointer-events and paint order. Inert documents and disconnected nodes
return empty regions before requiring active layout capabilities. Unsupported
active layout paths reject rather than return invented dimensions.

The reusable `*-chrome-control.mjs` runners in this directory serve authored
localhost fixtures and accept `--runner /absolute/chrome-cdp-trace.mjs --out
/absolute/control.json`. Captures must have zero collector errors before replacing
an oracle. Their metadata records the localhost-only extension flags. The saved
controls use Chrome 155.0.8059.79 / V8 15.5.35.23. The initial native proof uses
the default official V8 150 SDK, with 17 meaningful V8 contracts and 17 native
layout contracts. These results do not certify the genuine V8 155 configuration
or a fresh Google search.

Remaining bounds:

- Shaped text/UTF-16 source clusters, fragmented inline borders and caret provenance.
- Full stacking-context paint order and composed shadow-tree layout/CSS scoping.
- Root/viewport, RTL and vertical scrolling, scrollbars, events and scroll methods.
- Image intrinsic resources/source density, downloaded font and dynamic pseudo epochs.
- Actual iframe viewport dimensions and stable identities across document migration.
- Active SVG layout: any connected SVG namespace currently rejects the document
  snapshot, including HTML receivers in a page containing SVG icons. Degenerate
  transformed scroll-container projection is also incomplete.
- Fixed descendants of transformed containing blocks, explicitly rejected by core 0.3.5.
- Actual layout-unit arithmetic: authored `left/top: 0.1px` remains `0.1px` natively,
  while Chrome returns `0.09375px`. The complete `geometry-edges` oracle retains
  these fields; its scoped regression checks conversion, integer offsets and
  scrolling separately. Returned rectangles are not rounded to hide this difference.

Regression source is `rust/crates/turbo-surf-render/tests/live_geometry_contracts.rs`;
the `live_layout*` modules also test actual source provenance, rules, media,
transform metrics, overflow and inverse hit regions without V8.
