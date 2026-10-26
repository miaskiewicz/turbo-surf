// Records the Chrome control for tests/html_image.rs (turbo-surf#40): serves
// tests/fixtures/html_image_srcset_cases.json's page and its candidate images (/c/<case>/<x>.png,
// no-store) to a fresh Chrome page at surf's devicePixelRatio (2)
// and viewport width (1280), runs tests/fixtures/html_image_srcset_probe.js and writes
// tests/fixtures/html_image_srcset_chrome156.json. Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/html-image-srcset-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/html_image_srcset_probe.js"), "utf8");
const casesText = readFileSync(here("../fixtures/html_image_srcset_cases.json"), "utf8");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const origin = "https://html-image.test";
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const errors = [];
  const result = [];
  for (const { id, path, html, images } of JSON.parse(casesText)) {
    const page = await browser.newPage({
      viewport: { width: 1280, height: 720 },
      deviceScaleFactor: 2,
    });
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.route(`${origin}/**`, (route) => {
      const url = new URL(route.request().url());
      if (url.pathname === path) return route.fulfill({ contentType: "text/html", body: html });
      const image = images[(url.pathname.match(/\/(\w)\.png$/) || [])[1]];
      if (!image) return route.fulfill({ status: 404, body: "" });
      return route.fulfill({
        body: Buffer.from(image, "base64"),
        headers: { "content-type": "image/png", "cache-control": "no-store" },
      });
    });
    await page.goto(origin + path, { waitUntil: "load" });
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
    here("../fixtures/html_image_srcset_chrome156.json"),
    `${JSON.stringify(control, null, 2)}\n`,
  );
} finally {
  await browser.close();
}
