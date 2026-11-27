# Observer CSS reads use the captured native method

ResizeObserver and IntersectionObserver internal style reads now call the native
CSSStyleDeclaration.getPropertyValue captured during bootstrap. Author prototype
replacement must not intercept browser-internal padding, border or overflow reads.

Independent Chromium159 invoked the author method zero times; the unmodified
native observer invoked it 32 times on the same two-observer fixture. Native
regression first failed with eight author calls on one observer, then passed with
zero. The callback mutation inverse preserves a100,b100,a140,b140 and border
widths110,110,150,150: mutation affects the following delivery.

The exact Chromium fixture is
`harness/detection/fixtures/observer-intrinsics-chrome159.html`
(SHA256 6f1949ed58840ace5cda34d98f9448002931354bcd63994353c5215003e031bc).
The private raw oracle is `.recovery/modal-repro/resize-intrinsic-oracle`; native
RED/GREEN and broader checks are `.recovery/resize-observer-preparation`.

This is a separate correctness fix. It does not claim to resolve the ongoing
Flux Super 120-second timeout. Record-local geometry batching is still pending.
