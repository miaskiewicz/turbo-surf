// Records the Chrome control for tests/interceptor_mode_runtime.rs: runs
// tests/fixtures/interceptor_mode_probe.js in a fresh Chrome page and writes
// tests/fixtures/interceptor_mode_chrome156.json. Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/interceptor-mode-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/interceptor_mode_probe.js"), "utf8");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.route("https://interceptor-mode.test/", (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><body>" }),
  );
  await page.goto("https://interceptor-mode.test/");
  const result = await page.evaluate(probe);
  const control = {
    browser: `Chrome/${browser.version()}`,
    probeSha256: sha256(probe),
    collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
    errors,
    result,
  };
  writeFileSync(
    here("../fixtures/interceptor_mode_chrome156.json"),
    `${JSON.stringify(control, null, 2)}\n`,
  );
} finally {
  await browser.close();
}
