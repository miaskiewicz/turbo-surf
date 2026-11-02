// Records the Chrome control for tests/html_image.rs (turbo-dom#23, surf side): runs
// tests/fixtures/html_image_loads_probe.js over each page of
// tests/fixtures/html_image_loads_cases.json in a fresh Chrome page and writes
// tests/fixtures/html_image_loads_chrome156.json. Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/html-image-loads-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/html_image_loads_probe.js"), "utf8");
const casesText = readFileSync(here("../fixtures/html_image_loads_cases.json"), "utf8");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const errors = [];
  const result = [];
  for (const { id, url, html } of JSON.parse(casesText)) {
    const page = await browser.newPage();
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.route(url, (route) => route.fulfill({ contentType: "text/html", body: html }));
    await page.goto(url, { waitUntil: "load" });
    result.push({ id, result: await page.evaluate(probe) });
    await page.close();
  }
  const control = {
    browser: `Chrome/${browser.version()}`,
    probeSha256: sha256(probe),
    casesSha256: sha256(casesText),
    collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
    errors,
    result,
  };
  writeFileSync(
    here("../fixtures/html_image_loads_chrome156.json"),
    `${JSON.stringify(control, null, 2)}\n`,
  );
} finally {
  await browser.close();
}
