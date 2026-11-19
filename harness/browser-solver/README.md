# Browser challenge-solver harness

`solve.mjs` is an optional Playwright/Chromium sidecar for the core
`BrowserSolver`. It visits a URL in headless Chromium, waits for network idle and
three additional seconds, then returns cookies. It is development tooling, not
part of the shipped npm launcher, and does not establish that a challenge was
cleared. It does not automate visual CAPTCHA interaction.

The native engine remains browserless. See [solver selection, provider keys,
status and limitations](../../docs/SOLVERS.md) before enabling a solver. This
command is distinct from the [search browser sidecar](../../scripts/browser-sidecar/README.md).

## Setup and launch

From the repository root, install the development dependencies and a matching
Playwright Chromium binary:

```bash
npm ci
npx playwright install chromium
export TURBO_SURF_SOLVER=browser
export TURBO_SURF_BROWSER_CMD="node $PWD/harness/browser-solver/solve.mjs"
node cli.js
```

`TURBO_SURF_BROWSER_CMD` is executed with `sh -c`; quote paths containing spaces
inside the command value. A browser command alone is not auto-selected. To try
an in-house solver before this sidecar, set `TURBO_SURF_SOLVER=cloudflare`,
`awswaf`, `akamai`, or `recaptcha` while retaining the command. Fallback occurs
on a primary solve error, not on later server rejection of a returned token.

## JSON protocol

One JSON request is written to stdin, followed by EOF:

```json
{"url":"https://example.test/","vendor":"cloudflare","userAgent":"...","proxy":null}
```

Vendor labels emitted by the core include `akamai`, `datadome`, `kasada`,
`cloudflare`, `awswaf`, and `recaptcha`. The reference script does not dispatch
by vendor. It uses the requested URL, optional user agent, and optional proxy.

Write only the response JSON to stdout; diagnostics belong on stderr:

```json
{"cookies":{"cookie_name":"cookie_value"},"headers":{}}
```

Both maps contain string values. At least one cookie or header is required by
the Rust adapter. Exit zero on success; nonzero exit plus stderr is a failure.
A successful response's TTL is represented as 30 minutes internally, not proof
of the target's cookie lifetime. The MCP replay path sets cookies at the target
URL with `Path=/` and refetches once; original domain/path/expiry metadata is
not part of this protocol.

## Limits

The script adds a few automation-surface overrides; it is not a guarantee of
undetectable browsing. It launches headless Chromium with a fresh context, a
1920×1080 viewport, and `en-US` locale. It does not select system Chrome, use a
persistent profile, or expose a headful toggle. Other implementations can use
the same protocol.

`TURBO_SURF_PROXY` reaches this sidecar as `proxy`, but does not configure native
replay routing. Do not claim matching egress from that variable alone. Tokens
may also depend on TLS and browser fingerprints; no fixed Chrome version or
30-minute clearance guarantee is implied.

Rust's deterministic `e2e_solver_clears_wall_and_replays` test uses a stub
sidecar to verify the integration without a browser or live service. Running the
reference script against a real target is a separate, opt-in live check.
