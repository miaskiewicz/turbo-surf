# Browser sidecar (hardened Chrome) for `web_search`

Some search engines (google) gate their results behind a **browser-integrity wall**
(BotGuard / `enablejs`): a plain HTTP fetch gets the `enablejs` JS shell, not the SERP.
This sidecar is a **real** browser — kept **out of the turbo-surf engine binary** and
driven over a tiny JSON contract — used two ways:

- **MINT (google's default path).** A trusted `__Secure-ENID` cookie is minted *only* by a
  real browser's homepage load, but it is **long-lived (~2027) and client-agnostic**: once
  minted, google's `/search` serves the real SERP to a plain **native (no-browser)** wreq
  request that replays it. So the sidecar mints the token **rarely** (headed homepage load)
  and the native engine does the actual searches — "browser every query" becomes "native
  every query + a rare token refresh". A stale/rotated token (google falls back to the
  `enablejs` shell) auto-triggers exactly one re-mint + native retry.
- **FETCH (`browser:true`).** The whole SERP is fetched through the browser — an escape
  hatch for an engine even ENID reuse can't clear.

## Why it works (and naive stealth doesn't)

Learned from `../botto` (our own headless/automation detector):

- **The tell is the CDP connection**, not the IP/profile/mouse. Plain playwright/puppeteer
  enable the CDP `Runtime.enable` domain → a console-serialization side-channel google (and
  botto's `cdp-inspector-attached`) detect *even with `navigator.webdriver` spoofed and real
  Chrome + human input*. Verified: real headed Chrome still got `/sorry` until this was fixed.
- **Fix: [patchright](https://www.npmjs.com/package/patchright)** — a drop-in, CDP-hardened
  playwright that suppresses the `Runtime.enable` leak.
- **Less faking beats more faking.** botto flags any non-native override (`webdriver-getter-tampered`,
  non-native functions, canvas/WebGL render-gap). So we run **real Google Chrome** (genuine
  surface), use a **persistent context** (isolated `newContext` is itself a tell), and do
  **zero JS tampering** — `patchright` handles the automation hiding.

## Setup (one command)

```bash
bash scripts/browser-sidecar/setup.sh
export TURBO_SURF_BROWSER_FETCH_CMD="node $PWD/scripts/browser-sidecar/fetch-serp.mjs"
```

Then `web_search { query:"…", engine:"google" }` (google is `mode:"enid"`, so it mints via
the sidecar on first use and then searches natively), or `browser:true` to route any engine's
whole SERP through the browser. `remint:true` (or `TURBO_SURF_ENID_REMINT=1`) forces a fresh
mint.

Or from an agent: call the MCP tool **`web_search_setup_browser`**, which runs the script.

## Contract

Two stdin modes, both write to **stdout**:

- **FETCH:** `{"url","headless"?,"userAgent"?,"proxy"?}` → `{"html","finalUrl","status","blocked"}`.
  `blocked:true` = an anti-abuse captcha (`/sorry`) — surfaced by the mcp as a clear error.
- **MINT:** `{"mint":true,"headless"?,"proxy"?}` → `{"cookies":[{"name","value","domain","path","expires"}]}`
  from a real google homepage load (must include a trusted `__Secure-ENID`). The engine caches
  it to `.enid-cache.json` and replays it on native `/search` fetches.

`headless` defaults to **headed** (a trusted ENID is only earned headed; headless trips google's
checks). Set `TURBO_SURF_SIDECAR_HEADLESS=1` only where there's no display (wrap in xvfb).

## Committed vs local

Committed: `fetch-serp.mjs`, `setup.sh`, `package.json`, this README.
Gitignored (per-machine runtime): `node_modules/`, `.chrome-profile/` (the persistent profile
that warms NID/consent cookies across runs), `package-lock.json`, `.enid-cache.json` (the
minted-token cache).

## Notes

- **The trusted ENID is the cacheable token.** Unlike a per-request JS attestation, google's
  `__Secure-ENID` is durable + replayable from any client — that's what makes native search
  work. Minting is the only step that needs the browser.
- **Reuse depends on two things staying good:** the ENID staying *trusted* (google may rotate/
  invalidate it — the `enablejs`-triggered re-mint handles that) **and** a non-flagged exit IP.
  A `/sorry`'d IP won't serve the SERP to *any* token — that's IP reputation, orthogonal to the
  ENID, and handled by the existing `/sorry` clearance path. Reuse does **not** defeat IP-based
  blocking.
- **Headless mint works only with a display** — mint headed (default). Headless real Chrome
  zeroes plugins + breaks coherence checks and won't earn a *trusted* token.
