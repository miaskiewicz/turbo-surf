// Records the Chrome control for tests/image_formats.rs (turbo-surf#30): serves the page of
// tests/fixtures/image_formats_cases.json (and its http cases at /<id>) to a fresh Chrome
// page, runs tests/fixtures/image_formats_probe.js and writes
// tests/fixtures/image_formats_chrome156.json. Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/html-image-formats-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/image_formats_probe.js"), "utf8");
const casesText = readFileSync(here("../fixtures/image_formats_cases.json"), "utf8");
const cases = JSON.parse(casesText);
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const origin = "https://image-formats.test";
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const errors = [];
  const page = await browser.newPage();
  page.on("pageerror", (error) => errors.push(String(error)));
  const served = new Map(cases.http.map((c) => [`/${c.id}`, c]));
  await page.route(`${origin}/**`, (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/") return route.fulfill({ contentType: "text/html", body: cases.html });
    const c = served.get(path);
    if (!c) return route.fulfill({ status: 404, body: "" });
    const headers = c.contentType ? { "content-type": c.contentType } : {};
    return route.fulfill({ status: c.status, headers, body: Buffer.from(c.body, "base64") });
  });
  await page.goto(`${origin}/`, { waitUntil: "load" });
  const result = await page.evaluate(probe);
  const control = {
    browser: `Chrome/${browser.version()}`,
    probeSha256: sha256(probe),
    casesSha256: sha256(casesText),
    collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
    errors,
    result,
  };
  writeFileSync(
    here("../fixtures/image_formats_chrome156.json"),
    `${JSON.stringify(control, null, 2)}\n`,
  );
} finally {
  await browser.close();
}
