# Browser sidecar (hardened Chrome) for `web_search { browser:true }`

Some search engines (google) gate their results behind a **browser-integrity wall**
(BotGuard / `enablejs`): no headless HTTP fetch and no JS-render tier clears it, because
the results are served only after the client runs google's attestation VM in a *real*
browser. This sidecar is that browser — kept **out of the turbo-surf engine binary** and
driven over a tiny JSON contract.

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

Then `web_search { query:"…", engine:"google" }` (google is `mode:"browser"`, so it uses the
sidecar automatically), or `browser:true` to force any engine through it.

Or from an agent: call the MCP tool **`web_search_setup_browser`**, which runs the script.

## Contract

`fetch-serp.mjs` reads `{"url","userAgent"?,"proxy"?}` on **stdin**, drives the browser, and
writes `{"html","finalUrl","status","blocked"}` on **stdout**. `blocked:true` = an anti-abuse
captcha (`/sorry`) — surfaced by the mcp as a clear error, never parsed as empty results.

## Committed vs local

Committed: `fetch-serp.mjs`, `setup.sh`, `package.json`, this README.
Gitignored (per-machine runtime): `node_modules/`, `.chrome-profile/` (the persistent profile
that warms NID/consent cookies across runs), `package-lock.json`.

## Notes

- **Headless works** — patchright + persistent context passes google headless too (no window);
  a server deploy runs it under a virtual display if it wants headed.
- **Google needs the browser per search** — its gate is a per-request JS attestation, not a
  durable replayable cookie (unlike Cloudflare's `cf_clearance`), so there's no token to cache.
- **Rate limits** — hammering google from one IP still trips `/sorry` regardless of stealth;
  the persistent profile + human-paced use reduce it.
