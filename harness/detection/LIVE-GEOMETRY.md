# Native live geometry

The renderer uses the public Git-pinned `turbo-html2pdf-core 0.3.8` candidate
`5a602097` from producer PR #23. The latest published registry release is 0.3.7.
This revision repairs source-run advance admission and distinguishes staged native
Range/block/replaced projection from accumulated ordinary inline mapping. Producer
762-test validation plus strict checks pass, with an equivalent test/fixture-only
supplement. Surf's stock inline target passes4/4 and its genuine-native155
messaging/layout/DOM cohort passes78/78, with all1455 source hashes unchanged.
Producer coverage needs two additional controls after9656/9658 covered lines;
other CI checks pass. Final full consumer gates remain pending. The current
follow-up also integrates default/zero iframe corrections and officially
re-vendored computed-font resolution;
the implementation and exact tested source are tracked in [PARITY-GAPS.md](./PARITY-GAPS.md).
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
visibility, pointer-events and paint order. Inert documents, retired frames and disconnected nodes
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
Source/collector hashes pin the executed inputs; font-specific controls also
record their measured font hashes. The final focused genuine-native155 cohort
passes all75 tests, zero failures/skips: abstract Range3, custom elements10, GC4,
lifetime7, frame4, host input11, iframe1, inline2, live geometry23 and shadow10.
The iframe test strictly compares20 independently captured Chrome scenes. Its
initial zero-width and computed-font differences are repaired. All1350 source
hashes are unchanged during the140.32s run on canonical e8152d3/public layout93fbdd3.
The final pinned producer2eac7f6 adds coverage tests with identical production
bytes. The final canonical3ed91fb separately passes74 focused tests on rusty_v8 149.3.0,
including the40-case Chrome font oracle and repaired mixed-case/scalar calc paths.
The75-test production cohort above precedes that last font correction. The later
source9028bf4 genuine-native155 workspace passes1,452 tests across134 suites, zero
failures and6 generated-doctest ignores, with1,446 selected source hashes unchanged.
That result precedes the shared-crypto iframe ownership repair. Source31bf6fe
then passes the complete genuine-native155 workspace:1,453 tests, zero failures
and6 generated-doctest ignores across134 suites, strict all-target Clippy and
formatting, with all1,447 selected source hashes unchanged. Subsequent Linux CI
passes all eight frame-lifetime tests but exposes a bundled-font inline advance
mismatch. The new production geometry candidate requires renewed complete gates
and rebuilt addons. Earlier22/49/52-control runs remain historical checkpoints.

The new authored probes are reusable through the paired localhost collector:

```sh
node harness/detection/api-shape.mjs --run --binary /absolute/path/to/detection_trace \
  --probe harness/detection/native-iframe-layout-probe.js --out /private/tmp/iframe-pair
```

Use a new output directory for each capture. The same command accepts
`inline-border-layout-probe.js`, `composed-layout-lifecycle-probe.js`,
`frame-layout-startup-probe.js` or `frame-viewport-lifecycle-probe.js` from this
directory. It records the actual Chrome/native versions, executable/probe hashes
and every observed difference. It does not overwrite the saved regression oracle;
review a fresh Chrome result before replacing any fixture. The production native
Rust controls retain their source-hash guards and original execution budgets.

Implemented source geometry now includes shaped original UTF-16 clusters,
mixed-node Range rectangles, caret ownership/hits, ordinary rotated div/img,
outer SVG viewport boxes, composed shadow slots, named/manual reassignment,
retained/unassigned Range behavior, scoped native declarations and real CSP-applied
styles. Session-specific layout cache identities prevent snapshots crossing live
isolates with equal local document IDs. Native font loading includes platform-only
families (for example actual Menlo), while preserving ordinary PDF font behavior.

Real source-owned inline boundaries now survive shaping and wrapping. Decorated,
empty and mixed-font boxes include their own font metrics, horizontal edges,
relative offsets and baseline shifts, including nested images/inline blocks.
Transparent ancestor geometry projections do not duplicate paint; offset bounds
include empty fragments where Chrome does. Child/nested viewports derive from the
actual iframe content box, track resize and ignore page-overridden dimensions.
Viewport accessors and media queries preserve measured receiver/origin/error
behavior. Cross-document shadow adoption clears constructed sheet lists before
callbacks; frame retirement empties retained Element/Range geometry.

The reusable same-font control now serves the exact pinned IBM Plex Mono and
Fira Code bytes to normal headed Chrome, recording font/source hashes and actual
selected platform fonts. Whole and split source nodes, spaces, partial ranges and
rotation remain separate observations. The native candidate fixes source-run
admission before wrapping and placement. Its original rotated Range control now
matches exactly, as do the admitted nested block-Element contexts. The22 focused
producer tests pass with exact assertions; full gates and public dependency
integration remain pending. The sibling retains eleven Chrome contexts, with
native fixed children under a
transformed containing block still explicitly rejected and tracked in issue28. A headless capture has different font metrics and is preserved
separately. See the collector command in [README.md](./README.md).

Remaining bounds:

- Computed font resolution has40 independent Chrome cases plus owner/lifecycle
  controls.
  Metric-dependent ch/ex, viewport units, unit-cancelling division and
  min/max/clamp/var expressions remain
  uncertified; the existing partial computed-style surface can retain unresolved
  authored text for those classes. This does not certify their resolved values or
  complete font shorthand/variant grammar or serialization. Per-element UA heading/form-control font
  defaults also need separate controls beyond the measured body/inheritance cases.
  Preserve unresolved state through
  inheritance instead of converting it to zero or an undefined value (issue28).

- Native glyph advances now have a source-audited, same-font control candidate.
  Broader GPOS adjustment ordering and macOS AAT/trak font backend behavior still
  need independent controls; matching IBM/Fira and existing Times/Menlo cases does
  not establish complete shaping/backend conformance (issue28).

- BackCompat/quirks inline strut geometry needs real document-mode input. The
  retained zero-viewport about:blank control measures native body height34px versus
  Chrome30px while child rectangles and the Times text Range18.5px agree. Standards
  srcdoc controls are separate; this mismatch remains tracked in issue28.

- Linux font/fallback geometry is not certified by the saved macOS system-font
  oracle. Full exact font comparisons run on macOS; original UTF-16 boundaries,
  private receiver access and retained caret lifecycle remain exact on every OS.
  A matching Linux Chrome/font capture is still required.

- Ordinary inline boxes still reject top/bottom/middle alignment and
  absolute/fixed/sticky positioning. RTL/vertical writing and block-in-inline
  splitting also remain unsupported.
- Complete stacking contexts, scroll methods/events, RTL/vertical overflow and
  scrollbars; existing metrics/clamping and ancestor projection are tested subsets.
- Downloaded fonts, image source-density/intrinsic loading and animation/pseudo epochs.
- Broader iframe viewport/layout migrations beyond the tested content-box resize,
  nested frame, cross-document adoption and retirement controls.
- Inner SVG graphical primitives; supporting outer SVG boxes does not certify SVG paint.
- Full fixed-position/containing-block and layout-unit arithmetic behavior. Returned
  rectangles are not quantized after layout to conceal differences.
- Remaining stylesheet at-rules and full selector/CSS parsing behavior. Property
  conditions use genuine private grammar; public CSS.supports now uses private native grammar and selector-capability checks.

Regression source is `rust/crates/turbo-surf-render/tests/live_geometry_contracts.rs`;
the `live_layout*` modules also test actual source provenance, rules, media,
transform metrics, overflow and inverse hit regions without V8.
