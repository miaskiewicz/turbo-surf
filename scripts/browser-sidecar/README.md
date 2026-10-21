# Browser sidecar for `web_search`

The sidecar runs real Chrome outside the native engine over a JSON stdin/stdout
contract. The built-in Google strategy currently uses `mode:"enid"`: mint/cache
cookies through this sidecar, then try the native request, with one remint retry.
`browser:true` fetches the whole SERP through Chrome instead.

The ENID policy reflects earlier experiments. Presence of `__Secure-ENID` is not
proof of trust, and ENID is not universally required for results. On 2026-10-09 a
fresh Chrome 154 control got results without it by executing the search shell and
following a `sei` retry. The fully equipped native runtime's retry was rejected;
the cause remains unresolved. See the
[current findings](../../harness/detection/FINDINGS.md).

## Setup

```bash
bash scripts/browser-sidecar/setup.sh
export TURBO_SURF_BROWSER_FETCH_CMD="node $PWD/scripts/browser-sidecar/fetch-serp.mjs"
```

`web_search {query:"…",engine:"google"}` uses the ENID strategy;
`browser:true` uses browser fetching. `remint:true` or `TURBO_SURF_ENID_REMINT=1`
forces a mint. `web_search_setup_browser` runs setup from MCP.

The sidecar uses patchright, headed Chrome and a persistent profile. Patchright
reduces known CDP automation signals; it does not guarantee Google acceptance.
The mint flow seeds SOCS, visits the homepage, clicks/types in the search box,
waits, then checks for ENID without submitting a search. Modern responses may
omit ENID, which makes the mint fail even when Chrome itself could search.

## Contract

- FETCH: `{"url","headless"?,"userAgent"?,"proxy"?}` →
  `{"html","finalUrl","status","blocked"}`. `blocked:true` identifies `/sorry`;
  it does not identify the rejection's cause.
- MINT: `{"mint":true,"headless"?,"proxy"?}` →
  `{"cookies":[{"name","value","domain","path","expires"}]}`.
  The implementation requires an ENID cookie and returns an error if absent.
  The engine caches returned cookies in `.enid-cache.json`.

Headed is the default; `TURBO_SURF_SIDECAR_HEADLESS=1` or per-call `headless`
changes it. Headed/headless outcomes must be measured on the deployed version,
network and profile; neither mode guarantees a token or successful SERP.

## Diagnostics

For the reusable Chrome/native comparison, interface observations, payloads,
response bodies, immutable build provenance and failure artifacts, use
[`harness/detection`](../../harness/detection/README.md). It uses separate fresh
profiles/jars and never imports the Chrome cookies into the native attempt.

The smaller `net-trace.mjs` loads a page and records Chrome requests, POST bodies
and cookie-setting responses. `--fill` engages the field but does **not** submit a
search, so that command alone cannot establish a SERP outcome:

```bash
node scripts/browser-sidecar/net-trace.mjs https://www.google.com/ --fill "textarea[name=q]=rust lang" --wait 1500 --json
node scripts/browser-sidecar/run-probe.mjs scripts/browser-sidecar/probes/webgl-fp.js
```

Committed dev dependencies are declared in `package.json`. `node_modules/`,
`.chrome-profile/`, `package-lock.json` and `.enid-cache.json` are local/gitignored.
Cookie caches and raw diagnostics can contain session secrets; keep them local.
