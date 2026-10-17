# Native Trusted Types controls

The native binding implements private `TrustedHTML`, `TrustedScript` and
`TrustedScriptURL` brands, document-owned factories and policies, exact UTF-16
payloads, cross-realm recognition, native descriptors and sink mappings. Policy
callbacks and values are reachable only through guest-GC-owned V8 private edges;
the host holds no strong creation-realm handles. A separate real-GC frame control
installs default-policy callbacks that capture each child document, retires the
frames, retains a policy/factory from two of them, checks continued use, then
releases those guest references and requires every weak realm/document/factory/
policy reference to clear.

The production controls cover factory/value/descriptor/conversion behavior,
policy allowlists and duplicates, enforced and report-only security events,
HTML/script/attribute/Range/DOMParser sinks, default callbacks and typed values
whose public coercion is poisoned. The code-generation control separately covers
direct lexical eval, ordinary-object pass-through, Function source, CSP
`trusted-types-eval`, actual timer execution, and default-policy null results,
rewrites and exceptions. These are authored localhost pages, not copied challenge
scripts. Chrome runs in a fresh temporary profile and receives no imported cookies.

The production fixtures record the installed Chrome155.0.8059.80/V815.5.35.23.
The separate canonical binding controls retain their originally measured
Chrome155.0.8059.79 with the same V8 version. All tests verify exact
case/probe/collector hashes. The code collector explicitly sets
`allowUnsafeEvalBlockedByCSP:false`; DevTools otherwise bypasses real eval CSP
checks. Native object results are normalized by the authored probe before CDP
serialization. The older DOM/value collector has no eval/code-generation cases.

The actual engine hook is V8's ModifyCodeGenerationFromStrings callback. Its
private native result can permit unchanged source, supply exact branded source,
or deny with a genuine V8 EvalError message. It preserves direct-eval lexical
scope. Neither eval nor Function is replaced with a guest JavaScript wrapper.
The callback's policy order and Function-report prefixes follow
[Chromium's initializer](https://chromium.googlesource.com/chromium/src.git/+/18f2e4c962e3e6fd894abdb712957ef9749ae23e/third_party/blink/renderer/bindings/core/v8/v8_initializer.cc)
and [Trusted Types utilities](https://chromium.googlesource.com/chromium/src/+/740fb050927871cbb0cd32f3e28edecdc3cefb27/third_party/blink/renderer/core/trustedtypes/trusted_types_util.cc).
The independent installed-browser controls decide the expected behavior.

String timers perform native Trusted Types conversion at registration and CSP
admission before queueing. Admitted code compiles as a native global script with
the document source URL; it uses no eval/Function wrapper. Repeating timers keep
an active record so clearInterval during a callback prevents reinsertion.

Published SDKs lack the private callback and cannot claim full eval/Function
parity. They fail closed on enforced raw eval; the complete code-generation
control is guarded to require the actual prepared engine. DOM sink and policy
controls run on both profiles. Broader Trusted Types surfaces (including dynamic
script text mutation execution, remaining unsafe parser/document-write APIs and
default-policy reentrancy) still need independent coverage and are not certified
by this cohort. This is not a claim of complete browser or security parity.

Recollect the code control from the repository root:

```sh
node harness/detection/trusted-types-code-chrome.mjs \
  harness/detection/trusted-types-code-cases.json \
  harness/detection/trusted-types-code-probe.js \
  harness/detection/trusted-types-code-chrome155.json
```

Use `trusted-types-chrome.mjs` with the value or sink case/probe/control paths for
those separate controls. Run Rust `--test trusted_types`; actual source admission
also requires the prepared native engine and updated code-generation archive.
