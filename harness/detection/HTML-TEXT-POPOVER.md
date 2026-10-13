# HTML text and popover contracts

The shared [`html-text-popover-probe.js`](./html-text-popover-probe.js) runs against
ordinary headed Chrome and the native page on localhost. It creates and removes
its own fixtures; it makes no Google request and transfers no cookies.

```sh
node harness/detection/api-shape.mjs --run \
  --probe harness/detection/html-text-popover-probe.js \
  --binary /absolute/path/to/detection_trace \
  --out /absolute/path/to/a/fresh/output-directory
```

The capture saves both traces, executable and collector hashes, browser/engine
versions, results and field differences. The original October 13 measurement used
Chrome 155.0.8059.40 and the frozen V8 154 executable; that version mismatch is
recorded in the output and remains independent of these DOM contracts.

Canonical implementation: turbo-test commit `aa5f769`. All 65 library tests pass,
including four new cases for native text flow, mutation, brands/coercion and
popover/ToggleEvent state.

| Contract | Implemented behavior | Remaining limits |
| --- | --- | --- |
| `innerText` reads | Native DOM and authorized CSS cascade; whitespace collapse; block/paragraph/BR/table separators; preformatted text; visibility overrides; hidden content; closed details; upper/lowercase transforms; disconnected/unrendered fallback | Full CSS layout, composed/shadow/slotted text, generated content, language-dependent transforms, editing/media/top-layer state and complete CSS Text behavior |
| `innerText` writes | Actual Text and BR nodes; CRLF/CR normalization; literal markup; nullable conversion; old children detached | Complete browser MutationObserver record grouping and live Range adjustment |
| `outerText` writes | Actual node replacement; adjoining Text nodes merged; detached element throws `NoModificationAllowedError` | Complete observer/Range/selection side effects |
| `popover` reflection | Native attribute storage, nullable setter, `auto`/`manual`/`hint` normalization and invalid-value fallback | Opening/closing policy, top layer, light dismissal, auto/hint stacks, focus, invokers and `:popover-open` selectors |
| `ToggleEvent` | Genuine Event inheritance and private readonly `oldState`/`newState`/`source`; native brands and constructors | Popover-generated beforetoggle/toggle lifecycle and task coalescing |

Layout/offset integration remains separate. The screenshot tier already borrows
native layout, and turbo-html2pdf exposes `layout_boxes` keyed by source `data-cid`.
That seam measures box-generating elements; ordinary inline elements lose their
own box during flattening. Live JS geometry needs a protected identity mapping
from native DOM nodes, mutation/style/resource invalidation, inline fragments,
scroll/transform/zoom coordinates and browser-compatible offset-parent behavior.
The current live `offsetWidth`/`offsetHeight` implementation measures text with the
host font service; it does not supply the element's border-box layout.
