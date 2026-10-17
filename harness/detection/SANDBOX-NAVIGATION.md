# Sandbox top navigation control

The authored localhost control measures top-level hash navigation from native iframe scripts. Fresh normal Chrome155.0.8059.80/V8 15.5.35.23 denies same-origin and opaque sandboxed children without `allow-top-navigation`, including `allow-top-navigation-by-user-activation` when there is no activation. An explicit allowance works for both principals. Response CSP intersects the iframe attribute, including inherited restrictions. The collector disables the DevTools unsafe-eval CSP bypass and pins case/probe/collector bytes in the receipt.

Run the collector:

```sh
node harness/detection/sandbox-navigation-chrome.mjs harness/detection/sandbox-navigation-cases.json harness/detection/sandbox-navigation-probe.js harness/detection/sandbox-navigation-chrome155.json
```

The native regression asserts the actual engine and compares every source-pinned control case. The implementation keeps policy restrictions in native frame state: guest edits to `navigator.userActivation` cannot grant authority.

Remaining scope: genuine host user activation and its expiry/consumption, sibling/auxiliary navigation, popup creation, form submission, downloads, non-hash top-document loading, full navigation/session-history events. These controls do not establish those behaviors or full sandbox parity.

The guard follows [HTML allowed by sandboxing to navigate](https://html.spec.whatwg.org/multipage/browsing-the-web.html#allowed-by-sandboxing-to-navigate). The targeted no-activation behavior is independently measured in Chrome rather than inferred solely from the standard.
