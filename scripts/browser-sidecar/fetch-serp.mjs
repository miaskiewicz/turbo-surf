#!/usr/bin/env node
// Stealth browser SERP sidecar for turbo-surf's `web_search`.
// Chromium stays OUT of the engine binary; the mcp server shells out over a tiny
// JSON contract, two modes on stdin:
//   FETCH (browser:true / a mode:"browser" strategy):
//     stdin  : {"url":"…","headless"?:bool,"userAgent"?:"…","proxy"?:"…"}
//     stdout : {"html":"…","finalUrl":"…","status":200,"blocked":bool}
//   MINT (the native-google ENID path — mint a trusted __Secure-ENID rarely):
//     stdin  : {"mint":true,"headless"?:bool,"proxy"?:"…"}
//     stdout : {"cookies":[{"name","value","domain","path","expires"}, …]}
//     Loads the google homepage in REAL headed Chrome so google issues a TRUSTED
//     __Secure-ENID; the engine then reuses that cookie in fast native (no-browser)
//     /search fetches. A native/raw homepage GET earns only an UNtrusted ENID, so
//     minting MUST go through the real browser here.
//   headless defaults to false (headed) — see launchContext.
//   nonzero exit + stderr on failure.
//
// Wire it (opt-in):
//   TURBO_SURF_BROWSER_FETCH_CMD="node .sidecar/fetch-serp.mjs" turbo-surf-mcp
//
// DESIGN (informed by ../botto, our own headless/automation detector):
//   botto flags naive stealth as MORE detectable, not less —
//   `webdriver-getter-tampered` catches `Object.defineProperty(navigator,'webdriver',…)`,
//   and the surface/deep-probe scan flags any non-native (JS-getter) override of
//   plugins/WebGL/Notification/etc. via Function.prototype.toString + descriptor checks.
//   So we do the OPPOSITE of patching: launch REAL Chrome (channel:'chrome', not
//   chromium — genuine plugins/WebGL/audio/fonts/toString), HEADED (headless zeroes
//   plugins + breaks Notification↔Permissions coherence), and hide the one remaining
//   tell (navigator.webdriver) NATIVELY via --disable-blink-features=AutomationControlled
//   rather than a tamperable JS getter. Minimal cookies only; no surface tampering.
//   Lives in the gitignored .sidecar/ (its node_modules + any browser are never committed).

// patchright: a drop-in patched playwright that suppresses the CDP tells a real
// playwright leaks — chiefly the `Runtime.enable` console-serialization side-channel
// that lets google (and our own botto's `cdp-inspector-attached`) flag a CDP-driven
// browser even with navigator.webdriver spoofed and real Chrome + human input. This
// is THE fix for "real headed Chrome still gets /sorry": it's the automation
// CONNECTION being detected, not the IP/profile/mouse.
import { chromium } from "patchright";

function readStdin() {
  return new Promise((resolve) => {
    let buf = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (d) => (buf += d));
    process.stdin.on("end", () => resolve(buf));
    if (process.stdin.isTTY) resolve("");
  });
}

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const HERE = dirname(fileURLToPath(import.meta.url));

// patchright best-practice launch: a PERSISTENT context on REAL Chrome, HEADED,
// viewport:null, and NO manual stealth args — patchright warns that (a) an isolated
// `newContext` after `launch` is itself a tell, (b) real Chrome (channel:'chrome')
// beats bundled chromium, (c) headed beats headless, (d) it applies its own CDP/args
// hardening so extra `--disable-*` flags fight it. The persistent profile also warms
// across runs (NID/consent cookies accumulate). Returns a BrowserContext (not a
// Browser) — the caller uses it directly.
async function launchContext(proxy, headless) {
  const userDataDir = join(HERE, ".chrome-profile"); // gitignored, persists between runs
  // HEADED by default. Measured: headless real Chrome trips google's /sorry "unusual
  // traffic" wall (headless zeroes plugins + breaks Notification↔Permissions coherence
  // — an automation tell) while HEADED Chrome on the same IP returns the real SERP.
  // `headless` is caller-controlled (see main): per-call `headless` on stdin, else the
  // TURBO_SURF_SIDECAR_HEADLESS env, else headed. Run headless only where there's no
  // display (CI/servers — wrap in xvfb) and accept the /sorry risk.
  const opts = {
    channel: "chrome",
    headless,
    viewport: null,
    locale: "en-US",
    timezoneId: "America/New_York",
    proxy: proxy ? { server: proxy } : undefined,
  };
  return await chromium.launchPersistentContext(userDataDir, opts);
}

// MINT: load the google homepage in real headed Chrome and return the earned
// cookies (at least __Secure-ENID, plus AEC/SOCS/NID when present). The engine
// caches + replays __Secure-ENID on native /search fetches. A brief human-ish
// pause after load lets google set its full cookie set.
async function mintEnid(context) {
  await context.addCookies([{ name: "SOCS", value: "CAI", domain: ".google.com", path: "/" }]);
  const page = context.pages()[0] || (await context.newPage());
  await page.goto("https://www.google.com/", {
    waitUntil: "domcontentloaded",
    timeout: 30000,
  });
  await page.waitForTimeout(1200);
  // Only google.com cookies are relevant; map to the engine's cookie record.
  const cookies = (await context.cookies("https://www.google.com/")).map((c) => ({
    name: c.name,
    value: c.value,
    domain: c.domain,
    path: c.path,
    expires: c.expires, // epoch seconds; -1 == session
  }));
  process.stdout.write(JSON.stringify({ cookies }));
}

async function main() {
  const raw = await readStdin();
  let req = {};
  try {
    req = JSON.parse(raw || "{}");
  } catch {
    /* empty → error below */
  }
  if (!req.mint && !req.url) {
    process.stderr.write("fetch-serp: missing 'url' (or 'mint') on stdin\n");
    process.exit(2);
  }

  // Headless precedence: per-call `headless` on stdin > TURBO_SURF_SIDECAR_HEADLESS env
  // > headed (false). Headed is the reliable default (see launchContext) — and a
  // trusted __Secure-ENID is only earned headed (headless trips google's checks).
  const headless =
    typeof req.headless === "boolean"
      ? req.headless
      : process.env.TURBO_SURF_SIDECAR_HEADLESS === "1";
  const context = await launchContext(req.proxy, headless);
  if (req.mint) {
    try {
      await mintEnid(context);
    } finally {
      await context.close();
    }
    return;
  }
  try {
    // Google consent so the SERP isn't gated by the "before you continue" wall.
    await context.addCookies([
      { name: "CONSENT", value: "YES+", domain: ".google.com", path: "/" },
      { name: "SOCS", value: "CAI", domain: ".google.com", path: "/" },
    ]);
    const page = context.pages()[0] || (await context.newPage());

    // A direct SERP hit reads as a bot and trips google's /sorry "unusual traffic"
    // captcha. Warm the session like a human: land on the homepage first (seeds NID +
    // a real Referer), a brief human-ish pause, THEN navigate to the results.
    const isGoogle = /(^|\.)google\./i.test(new URL(req.url).hostname);
    if (isGoogle) {
      try {
        await page.goto("https://www.google.com/", {
          waitUntil: "domcontentloaded",
          timeout: 20000,
        });
        await page.waitForTimeout(800);
      } catch {}
    }

    const resp = await page.goto(req.url, { waitUntil: "domcontentloaded", timeout: 30000 });
    await page.waitForSelector("#search a h3, #rso a h3, div.g", { timeout: 8000 }).catch(() => {});
    const html = await page.content();
    const finalUrl = page.url();
    const blocked = /\/sorry\//.test(finalUrl) || /unusual traffic/i.test(html);
    process.stdout.write(
      JSON.stringify({ html, finalUrl, status: resp ? resp.status() : 0, blocked }),
    );
  } finally {
    await context.close();
  }
}

main().catch((e) => {
  process.stderr.write("fetch-serp: " + (e && e.stack ? e.stack : e) + "\n");
  process.exit(1);
});
