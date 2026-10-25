# Document response hooks control

The authored localhost control measures the Document state Chrome derives from the navigation response and the embedding frame: `document.lastModified`, `document.domain` assignment (CSP `sandbox`, iframe `sandbox`, origin-keyed agent clusters), `window.originAgentCluster`, and the permissions-policy-backed `fullscreenEnabled`, `webkitFullscreenEnabled` and `pictureInPictureEnabled`. Fresh Chrome 156.0.8078.47 / V8 15.6.75.19, headful and `--headless=new`, produce identical results; `pictureInPictureEnabled` follows the `picture-in-picture` policy in both modes rather than window availability.

Run the collector (set `TURBO_SURF_CHROME_HEADLESS=1` for the headless receipt):

```sh
node harness/detection/document-hooks-chrome.mjs harness/detection/document-hooks-cases.json harness/detection/document-hooks-probe.js harness/detection/fixtures/document_hooks_chrome156.json
```

Measured rules the engine reproduces (`rust/crates/turbo-surf-render/tests/document_hooks.rs`):

- `lastModified` is the response `Last-Modified` instant in local time; absent or unparsable headers, srcdoc and about:blank documents report the current time.
- Every sandbox (CSP or iframe attribute, any tokens) forbids `document.domain` assignment, and nested srcdoc and response documents inherit the flag. Opaque origins report an empty domain.
- Agent clustering follows HTML's historical key map per page: the first document of an origin fixes its keying (`Origin-Agent-Cluster`, default keyed for potentially trustworthy origins), later same-origin documents keep it whatever their header says, srcdoc/about:blank join their creator's cluster, opaque origins are keyed. A keyed cluster ignores `document.domain` assignment.
- Fullscreen is enabled for same-origin children without `allow`; cross-origin children need `allow="fullscreen"` or `allowfullscreen`; an `allow` declaration of the feature wins over `allowfullscreen`; `Permissions-Policy` on either document disables it. Picture-in-picture defaults to every origin. Inert and detached documents report false.

Remaining scope: srcdoc and initial about:blank documents share their creator's origin in Chrome, so a `document.domain` assignment in one relaxes all of them; the vendored binding keeps the relaxed domain per Document (pinned as a known gap in the test). Cross-document access checks based on relaxed domains, `requestFullscreen`/`requestPictureInPicture` behavior and non-RFC `Last-Modified` formats Chrome's lenient date parser accepts are not established by this control.
