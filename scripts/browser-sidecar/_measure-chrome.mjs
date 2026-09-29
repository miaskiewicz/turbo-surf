// TEMP measurement harness (not committed): load google's homepage in real headed
// Chrome (same hardened launch as fetch-serp), then evaluate the shared collector in
// the page and print the fingerprint snapshot + whether a trusted __Secure-ENID was
// earned. Compared against the turbo-surf isolate's snapshot to find the value tells.
import { chromium } from "patchright";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const COLLECT = readFileSync(process.argv[2], "utf8");

const context = await chromium.launchPersistentContext(join(HERE, ".chrome-profile"), {
  channel: "chrome",
  headless: false,
  viewport: null,
  locale: "en-US",
  timezoneId: "America/New_York",
});
await context.addCookies([{ name: "SOCS", value: "CAI", domain: ".google.com", path: "/" }]);
const page = context.pages()[0] || (await context.newPage());
await page.goto("https://www.google.com/", { waitUntil: "domcontentloaded", timeout: 30000 });
await page.waitForTimeout(1500);

const snapJson = await page.evaluate(COLLECT);
const cookies = await context.cookies("https://www.google.com/");
const enid = cookies.find((c) => c.name === "__Secure-ENID");
process.stdout.write(
  JSON.stringify(
    {
      snapshot: JSON.parse(snapJson),
      enid: enid ? enid.value.slice(0, 16) + "…" : null,
      cookieNames: cookies.map((c) => c.name),
    },
    null,
    2,
  ),
);
await context.close();
process.exit(0);
