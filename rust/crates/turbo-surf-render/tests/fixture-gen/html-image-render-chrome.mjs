// Records the Chrome control for tests/html_image.rs's render() case (turbo-surf#32):
// serves tests/fixtures/html_image_render_page.html with
// tests/fixtures/html_image_render_probe.js as an inline script at the end of the body
// (where render()'s script runs) and writes the document title it sets to
// tests/fixtures/html_image_render_chrome156.json. Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/html-image-render-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/html_image_render_probe.js"), "utf8");
const html = readFileSync(here("../fixtures/html_image_render_page.html"), "utf8");
const url = "https://html-image.test/";
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  const body = `${html.trimEnd()}<script>${probe}</script>`;
  await page.route(url, (route) => route.fulfill({ contentType: "text/html", body }));
  await page.goto(url, { waitUntil: "load" });
  await page.waitForFunction(() => document.title !== "", null, { timeout: 10000 });
  const control = {
    browser: `Chrome/${browser.version()}`,
    probeSha256: sha256(probe),
    pageSha256: sha256(html),
    collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
    errors,
    result: JSON.parse(await page.title()),
  };
  writeFileSync(
    here("../fixtures/html_image_render_chrome156.json"),
    `${JSON.stringify(control, null, 2)}\n`,
  );
} finally {
  await browser.close();
}
