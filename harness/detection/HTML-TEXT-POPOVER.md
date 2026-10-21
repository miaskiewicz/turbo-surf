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

Canonical checkpoints: turbo-test `aa5f769` implements text and reflection,
`953a49d` adds mutation security, and `7afb6ad` adds native popover lifecycle.
The latest canonical library suite passes 79 tests, including eight popover
lifecycle/ownership tests. Surf must vendor that committed source and capture the
trusted scheduler after its environment is installed before running guest code.
The shared26-field Chrome155 lifecycle control passes in Surf, as does the
child scheduler/privacy/realm regression after canonical `f2c596c`. Fresh paired
comparisons of all collectors still require the final rebuilt executable.

| Contract | Implemented behavior | Remaining limits |
| --- | --- | --- |
| `innerText` reads | Native DOM and authorized CSS cascade; whitespace collapse; block/paragraph/BR/table separators; preformatted text; visibility overrides; hidden content; closed details; upper/lowercase transforms; disconnected/unrendered fallback | Full CSS layout, composed/shadow/slotted text, generated content, language-dependent transforms, editing/media/top-layer state and complete CSS Text behavior |
| `innerText` writes | Actual Text and BR nodes; CRLF/CR normalization; literal markup; nullable conversion; old children detached | Complete browser MutationObserver record grouping and live Range adjustment |
| `outerText` writes | Actual node replacement; adjoining Text nodes merged; detached element throws `NoModificationAllowedError` | Complete observer/Range/selection side effects |
| `popover` reflection | Native null-namespace attribute storage, nullable setter, enum normalization and invalid-value fallback; foreign namespace attributes remain independent | Complete HTML reflection beyond the covered popover attribute |
| Popover lifecycle | Actual native show/hide/toggle state, auto/hint/manual ancestry and source relationships, cancelable trusted opening, trusted closing, queued/coalesced trusted toggle events, ordinary removal/moves and state-preserving `moveBefore`; reentrant validity checks and inert-document rejection | Autofocus/restoration, external light dismissal, backdrop, fullscreen/modal integration, stateful CSS cascade and top-layer painting |
| `:popover-open` selectors | Native open state combined with live structural selectors within the actual DOM root; nested `:is`/`:not`/`:has`, shadow separation and escaped tokens; no DOM/style markers | Other unsupported selector grammar and stateful stylesheet-cascade integration |
| `ToggleEvent` | Genuine Event inheritance and private readonly fields; private popover event constructor/scheduler; queued event target identity survives adoption | Complete event-handler error reporting and browser lifecycle coverage beyond the measured contracts |

Layout/offset integration remains separate. The screenshot tier already borrows
native layout, and turbo-html2pdf exposes `layout_boxes` keyed by source `data-cid`.
That seam measures box-generating elements; ordinary inline elements lose their
own box during flattening. Live JS geometry needs a protected identity mapping
from native DOM nodes, mutation/style/resource invalidation, inline fragments,
scroll/transform/zoom coordinates and browser-compatible offset-parent behavior.
The current live `offsetWidth`/`offsetHeight` implementation measures text with the
host font service; it does not supply the element's border-box layout.

The expanded [`popover-lifecycle-probe.js`](./popover-lifecycle-probe.js) runs through
the same `api-shape.mjs --probe` command. Headed Chrome 155 controls are saved under
`artifacts/2026-10-13-popover-lifecycle-chrome-155-*`. They verify cancellation,
coalescing, source conversion, the nine auto/hint/manual ancestry combinations,
ordinary self-reference/removal versus `moveBefore`, foreign namespaces and the
already-queued toggle event surviving removal. Those are Chrome controls; a fresh
paired run with the rebuilt native executable is still required.
