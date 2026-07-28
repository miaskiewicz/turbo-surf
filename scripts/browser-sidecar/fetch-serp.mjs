#!/usr/bin/env node
// Stealth browser SERP sidecar for turbo-surf's `web_search { browser:true }`.
// Chromium stays OUT of the engine binary; the mcp server shells out over a tiny
// JSON contract:
//   stdin  : {"url":"…","userAgent"?:"…","proxy"?:"…"}
//   stdout : {"html":"…","finalUrl":"…","status":200,"blocked":bool}
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
async function launchContext(proxy) {
  const userDataDir = join(HERE, ".chrome-profile"); // gitignored, persists between runs
  const opts = {
    channel: "chrome",
    headless: true,
    viewport: null,
    locale: "en-US",
    timezoneId: "America/New_York",
    proxy: proxy ? { server: proxy } : undefined,
  };
  return await chromium.launchPersistentContext(userDataDir, opts);
}

async function main() {
  const raw = await readStdin();
  let req = {};
  try { req = JSON.parse(raw || "{}"); } catch { /* empty → error below */ }
  if (!req.url) {
    process.stderr.write("fetch-serp: missing 'url' on stdin\n");
    process.exit(2);
  }

  const context = await launchContext(req.proxy);
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
        await page.goto("https://www.google.com/", { waitUntil: "domcontentloaded", timeout: 20000 });
        await page.waitForTimeout(800);
      } catch (e) {}
    }

    const resp = await page.goto(req.url, { waitUntil: "domcontentloaded", timeout: 30000 });
    await page.waitForSelector("#search a h3, #rso a h3, div.g", { timeout: 8000 }).catch(() => {});
    const html = await page.content();
    const finalUrl = page.url();
    const blocked = /\/sorry\//.test(finalUrl) || /unusual traffic/i.test(html);
    process.stdout.write(JSON.stringify({ html, finalUrl, status: resp ? resp.status() : 0, blocked }));
  } finally {
    await context.close();
  }
}

main().catch((e) => {
  process.stderr.write("fetch-serp: " + (e && e.stack ? e.stack : e) + "\n");
  process.exit(1);
});
