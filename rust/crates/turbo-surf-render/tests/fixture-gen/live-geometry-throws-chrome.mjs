// Records the Chrome control for tests/live_geometry_throws.rs (turbo-surf#31): runs
// tests/fixtures/live_geometry_throws_probe.js over each page of
// tests/fixtures/live_geometry_throws_cases.json (served at https://live-geometry.test/) in
// a fresh 1280x800 Chrome page and writes tests/fixtures/live_geometry_throws_chrome156.json.
// Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/live-geometry-throws-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/live_geometry_throws_probe.js"), "utf8");
const casesText = readFileSync(here("../fixtures/live_geometry_throws_cases.json"), "utf8");
const url = "https://live-geometry.test/";
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const errors = [];
  const result = [];
  for (const { id, html } of JSON.parse(casesText)) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.route(url, (route) => route.fulfill({ contentType: "text/html", body: html }));
    await page.goto(url, { waitUntil: "load" });
    result.push({ id, result: await page.evaluate(probe) });
    await page.close();
  }
  const control = {
    browser: `Chrome/${browser.version()}`,
    viewport: [1280, 800],
    probeSha256: sha256(probe),
    casesSha256: sha256(casesText),
    collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
    errors,
    result,
  };
  writeFileSync(
    here("../fixtures/live_geometry_throws_chrome156.json"),
    `${JSON.stringify(control, null, 2)}\n`,
  );
} finally {
  await browser.close();
}
