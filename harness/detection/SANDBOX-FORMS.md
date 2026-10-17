# Sandboxed form default actions

Nine independently authored fresh Chrome155.0.8059.80/V8 15.5.35.23 localhost controls measure real button.click() and trusted submit events. Sandboxing suppresses the submit default action before any submit event, while preserving the click. Header policies, inherited restrictions and iframe attributes intersect; allow-forms is ASCII case insensitive, and opaque children may submit if allowed. The collector and every source byte are pinned in the saved control.

The native consumer supplies its intersected sandbox state to canonical per-document admission. Guest attributes and another realm cannot change the installed document policy. This gate runs after click handlers and preserves cancellation/trusted event behavior.

Run the normal Chrome collector with sandbox-form-cases.json and sandbox-form-probe.js, then cargo test -p turbo-surf-render --test sandbox_forms through the pinned native SDK helper. The regression checks actual V8 version and fixture lengths and hashes.

Form validation, requestSubmit/submit admission and real top-document text GET/POST
navigation are now integrated. Actual native155+wreq controls cover validation,
retained form realms, HTTP response installation, redirects/consent cookies,
fragments, cancellation/CSP and UTF-8/Windows-1252 encoding. This initial nine-case
control remains a focused sandbox admission check.

Remaining: genuine FormData event/ElementInternals integration, complete successful
controls, multipart/file-input submission, child/other browsing targets,
meta-only/default legacy encoding provenance, broader form-action/reporting,
activation and popup behavior. Track these in [PARITY-GAPS.md](./PARITY-GAPS.md)
and [issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28).
