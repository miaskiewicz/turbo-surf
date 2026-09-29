// TEMP net-differential harness (not committed): load google.com in real headed Chrome and
// record EVERY network request it fires (method, url, resourceType) + which response sets a
// __Secure-ENID cookie. Compared against turbo-surf's in-isolate op_fetch log to find the
// request(s) real Chrome fires that we don't — the mint/scoring exchange we may be missing.
import { chromium } from "patchright";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const context = await chromium.launchPersistentContext(join(HERE, ".chrome-profile-net"), {
  channel: "chrome",
  headless: false,
  viewport: null,
  locale: "en-US",
  timezoneId: "America/New_York",
});
await context.addCookies([{ name: "SOCS", value: "CAI", domain: ".google.com", path: "/" }]);
const page = context.pages()[0] || (await context.newPage());

const reqs = [];
page.on("request", (r) => reqs.push({ method: r.method(), url: r.url(), type: r.resourceType() }));
const enidSetters = [];
page.on("response", async (r) => {
  try {
    const h = await r.allHeaders();
    const sc = h["set-cookie"] || "";
    if (sc.includes("__Secure-ENID")) enidSetters.push({ url: r.url(), status: r.status() });
  } catch (e) {}
});

await page.goto("https://www.google.com/", { waitUntil: "networkidle", timeout: 30000 });
// A small interaction to trigger any engagement-gated requests (BotGuard fires on engagement).
try {
  await page.fill("textarea[name=q], input[name=q]", "rust programming language", {
    timeout: 4000,
  });
  await page.waitForTimeout(600);
} catch (e) {}
await page.waitForTimeout(1500);

// Group by URL path (strip query) + method, count, so the cascade is legible.
const norm = (u) => {
  try {
    const x = new URL(u);
    return x.origin + x.pathname;
  } catch (e) {
    return u;
  }
};
const byKey = new Map();
for (const r of reqs) {
  const key = `${r.method} ${norm(r.url)} [${r.type}]`;
  byKey.set(key, (byKey.get(key) || 0) + 1);
}
const rows = [...byKey.entries()].sort((a, b) => a[0].localeCompare(b[0]));

process.stdout.write("=== REAL CHROME network cascade on google.com (grouped) ===\n");
for (const [k, n] of rows) process.stdout.write(`${String(n).padStart(3)}x  ${k}\n`);
process.stdout.write(`\n=== responses that SET __Secure-ENID ===\n`);
for (const e of enidSetters) process.stdout.write(`  ${e.status}  ${e.url.slice(0, 120)}\n`);
process.stdout.write(`\ntotal requests: ${reqs.length}\n`);
// Also dump the raw POST list (the interesting ones for token exchange).
process.stdout.write(`\n=== POSTs (token/beacon exchanges) ===\n`);
for (const r of reqs.filter((r) => r.method === "POST")) process.stdout.write(`  ${norm(r.url)}\n`);

await context.close();
process.exit(0);
