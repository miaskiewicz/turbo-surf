# Native live geometry

The renderer uses the public Git-pinned `turbo-html2pdf-core 0.3.8` candidate
from producer PR #23. The latest published registry release is 0.3.7.
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

[Host media](./HOST-MEDIA.md) supplies real platform preferences and installed
input-driver capabilities to both live CSS and relevant media queries; cache keys
include that captured environment. [Modern at-rule admission](./NEUTRAL-AT-RULES.md)
uses typed rules and a transitive variable/declaration proof for bounded static
geometry. Unknown or potentially geometry-changing uses still reject. Retained
descriptor blocks alone do not implement CSS property registration or transitions.

The reusable `*-chrome-control.mjs` runners serve independent authored localhost
fixtures. The older controls retain their original Chrome155.0.8059.79 metadata;
new shadow/scoped controls record Chrome155.0.8059.80. Both use V815.5.35.23.
Source/collector/font hashes pin the executed inputs. Current combined actual155
checks pass22 geometry tests plus8 composed/conditional stylesheet and public CSS tests. These
validate the tested0.3.8 producer candidate, which is not yet published.

Implemented source geometry now includes shaped original UTF-16 clusters,
mixed-node Range rectangles, caret ownership/hits, ordinary rotated div/img,
outer SVG viewport boxes, composed shadow slots, named/manual reassignment,
retained/unassigned Range behavior, scoped native declarations and real CSP-applied
styles. Session-specific layout cache identities prevent snapshots crossing live
isolates with equal local document IDs. Native font loading includes platform-only
families (for example actual Menlo), while preserving ordinary PDF font behavior.

Remaining bounds:

- Actual decorated, empty and mixed-font inline border production. Plain inline
  support is bounded by real source-owned text runs; unsupported decoration rejects.
- Complete stacking contexts, scroll methods/events, RTL/vertical overflow and
  scrollbars; existing metrics/clamping and ancestor projection are tested subsets.
- Downloaded fonts, image source-density/intrinsic loading and animation/pseudo epochs.
- Stable iframe viewport/layout identities across all document migrations.
- Inner SVG graphical primitives; supporting outer SVG boxes does not certify SVG paint.
- Full fixed-position/containing-block and layout-unit arithmetic behavior. Returned
  rectangles are not quantized after layout to conceal differences.
- Remaining stylesheet at-rules and full selector/CSS parsing behavior. Property
  conditions use genuine private grammar; public CSS.supports now uses private native grammar and selector-capability checks.

Regression source is `rust/crates/turbo-surf-render/tests/live_geometry_contracts.rs`;
the `live_layout*` modules also test actual source provenance, rules, media,
transform metrics, overflow and inverse hit regions without V8.
