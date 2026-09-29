// Run a collector script in real Chrome and print its JSON result — the real-browser side of a
// turbo-surf-vs-Chrome differential. Pair with the turbo-surf side (`cargo run -p turbo-surf-mcp
// --example fp_snapshot -- <collector.js>`) and diff the two JSON blobs.
//
// Usage:
//   node run-probe.mjs <collector.js> [url] [--lang <tag>] [--tz <zone>] [--headless]
//   node run-probe.mjs probes/botguard-probe.js https://www.google.com/
//   node run-probe.mjs probes/botguard-probe.js            # defaults to about:blank
//
// The collector is a self-contained JS expression returning a JSON STRING (see probes/*.js).
// A fresh Chrome profile per run; auto-accepts a consent wall so the real page is what's probed.
import { chromium } from "patchright";
import { readFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const argv = process.argv.slice(2);
const positional = argv.filter((a) => !a.startsWith("--"));
const collectorPath = positional[0];
const url = positional[1] || "about:blank";
if (!collectorPath) {
  process.stderr.write(
    "usage: node run-probe.mjs <collector.js> [url] [--lang tag] [--tz zone] [--headless]\n",
  );
  process.exit(2);
}
const flag = (name, def) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : def;
};
const lang = flag("--lang", "en-US");
const tz = flag("--tz", "America/New_York");
const headless = argv.includes("--headless");
const COLLECT = readFileSync(collectorPath, "utf8");

const context = await chromium.launchPersistentContext(mkdtempSync(join(tmpdir(), "run-probe-")), {
  channel: "chrome",
  headless,
  viewport: null,
  locale: lang,
  timezoneId: tz,
  extraHTTPHeaders: { "Accept-Language": `${lang},${lang.split("-")[0]};q=0.9` },
});
const page = context.pages()[0] || (await context.newPage());
try {
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });
} catch (e) {
  process.stderr.write(`goto warning: ${e.message}\n`);
}
// Best-effort consent accept (multilingual), so a geo-localized wall doesn't shadow the real page.
const RE =
  /\b(accept all|accept|agree|i agree|allow all|got it|aceitar tudo|aceitar|aceptar todo|tout accepter|alle akzeptieren|accetta tutto)\b/i;
for (const frame of page.frames()) {
  try {
    for (const h of await frame.$$("button, [role=button], input[type=submit]")) {
      const label = (
        (await h.getAttribute("aria-label")) ||
        (await h.innerText().catch(() => "")) ||
        ""
      ).trim();
      if (label && RE.test(label)) {
        await h.click({ timeout: 2000 }).catch(() => {});
        await page.waitForTimeout(600);
        break;
      }
    }
  } catch (e) {}
}
await page.waitForTimeout(1200);

const raw = await page.evaluate(COLLECT);
// Collectors return a JSON string; pretty-print it (fall back to raw on non-JSON).
try {
  process.stdout.write(JSON.stringify(JSON.parse(raw), null, 2) + "\n");
} catch (e) {
  process.stdout.write(String(raw) + "\n");
}
await context.close();
process.exit(0);
