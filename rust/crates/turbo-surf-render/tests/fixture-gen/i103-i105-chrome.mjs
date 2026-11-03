// Records the Chrome controls for tests/unhandled_rejection.rs (turbo-surf#103) and
// tests/container_queries.rs (turbo-surf#105). Pages are served at http://127.0.0.1/ via
// route interception (nothing leaves the machine) in fresh 1280x800 pages.
// Regenerate (frozen headless Chrome 156):
//   TURBO_SURF_CHROME_PATH=".../chrome-headless-shell" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/i103-i105-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const read = (name) => readFileSync(here(`../fixtures/${name}`), "utf8");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const url = "http://127.0.0.1/";
const collector = sha256(readFileSync(new URL(import.meta.url)));
const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--use-mock-keychain", "--password-store=basic"],
});
const open = async (html) => {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.route(url, (route) => route.fulfill({ contentType: "text/html", body: html }));
  await page.goto(url, { waitUntil: "load" });
  return { page, pageErrors };
};
const write = (name, value) =>
  writeFileSync(here(`../fixtures/${name}`), `${JSON.stringify(value, null, 2)}\n`);
try {
  const rejection = read("unhandled_rejection_probe.js");
  {
    const { page, pageErrors } = await open(
      "<!doctype html><html><head></head><body></body></html>",
    );
    const result = await page.evaluate(rejection);
    await page.waitForTimeout(100);
    write("unhandled_rejection_chrome156.json", {
      browser: `Chrome/${browser.version()}`,
      probeSha256: sha256(rejection),
      collectorSha256: collector,
      // Console reports ("Uncaught (in promise)"): the prevented rejection is absent.
      pageErrors,
      result,
    });
    await page.close();
  }
  const probe = read("container_queries_probe.js");
  const casesText = read("container_queries_cases.json");
  const errors = [];
  const result = [];
  for (const { id, html } of JSON.parse(casesText)) {
    const { page, pageErrors } = await open(html);
    errors.push(...pageErrors);
    result.push({ id, result: await page.evaluate(probe) });
    await page.close();
  }
  write("container_queries_chrome156.json", {
    browser: `Chrome/${browser.version()}`,
    viewport: [1280, 800],
    probeSha256: sha256(probe),
    casesSha256: sha256(casesText),
    collectorSha256: collector,
    errors,
    result,
  });
} finally {
  await browser.close();
}
