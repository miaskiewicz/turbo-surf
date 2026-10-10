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
//     Visits the homepage and interacts with its box, then requires ENID in the
//     returned cookies. Presence does not attest trust; current Chrome can search
//     without ENID. See harness/detection/FINDINGS.md for measured outcomes.
//   headless defaults to false (headed) — see launchContext.
//   nonzero exit + stderr on failure.
//
// Wire it (opt-in):
//   TURBO_SURF_BROWSER_FETCH_CMD="node .sidecar/fetch-serp.mjs" turbo-surf-mcp
//
// patchright reduces known CDP automation signals. Headed Chrome + a persistent
// profile is the chosen oracle configuration, not a guarantee of Google clearance.
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
  // Headed by default; callers can override it. Google acceptance depends on the
  // current challenge/profile/network, so measure either mode rather than assume.
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
// cookies (the trusted __Secure-ENID, plus AEC/SOCS/NID when present). The engine
// caches + replays __Secure-ENID on native /search fetches.
//
// This mint flow engages the box but does not submit. It was designed around an
// earlier interaction-gated ENID grant; modern responses may not issue ENID at all.
async function mintEnid(context) {
  await context.addCookies([{ name: "SOCS", value: "CAI", domain: ".google.com", path: "/" }]);
  const page = context.pages()[0] || (await context.newPage());
  await page.goto("https://www.google.com/", {
    waitUntil: "domcontentloaded",
    timeout: 30000,
  });
  await page.waitForTimeout(600);
  // Engage the search box to trigger BotGuard (real Chrome 153 uses a <textarea name=q>;
  // older layouts use <input name=q>). Best-effort — a layout miss must not fail the mint.
  try {
    const box = page.locator('textarea[name="q"], input[name="q"]').first();
    await box.click({ timeout: 5000 });
    for (const ch of "weather today") {
      await page.keyboard.type(ch, { delay: 60 + Math.floor(Math.random() * 90) });
    }
    // Let the interaction-gated VM load /js/bg/…, run, and the ENID grant land.
    await page.waitForTimeout(2500);
  } catch (e) {
    process.stderr.write("mint: search-box interaction skipped (" + (e && e.message) + ")\n");
    await page.waitForTimeout(1500);
  }
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
