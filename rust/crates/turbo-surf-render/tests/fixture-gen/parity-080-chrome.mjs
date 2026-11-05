// Records the Chrome control for tests/parity_080.rs (turbo-surf#156, #158, #159, #160,
// #161, #162, #163). Each case of fixtures/parity_080_cases.json is served at
// http://127.0.0.1/ via route interception (nothing leaves the machine) in a fresh 800x600
// page of a browser launched at the case's device scale factor (`--force-device-scale-factor`:
// a real screen scale, which unlike CDP's emulated one also moves text metrics — the default
// line box is 18px at 1 and 18.5px at 2); its `probe` expression's value is the result.
// Regenerate (frozen headless Chrome 156):
//   TURBO_SURF_CHROME_PATH=".../chrome-headless-shell" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/parity-080-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const url = "http://127.0.0.1/";
const casesText = readFileSync(here("../fixtures/parity_080_cases.json"), "utf8");
const launch = (dpr) =>
  chromium.launch({
    executablePath,
    headless: true,
    args: ["--use-mock-keychain", "--password-store=basic", `--force-device-scale-factor=${dpr}`],
  });
const errors = [];
const result = [];
let version;
for (const { id, html, dpr, probe } of JSON.parse(casesText)) {
  const browser = await launch(dpr);
  version = browser.version();
  try {
    const page = await browser.newPage({
      viewport: { width: 800, height: 600 },
      deviceScaleFactor: dpr,
    });
    page.on("pageerror", (error) => errors.push(`${id}: ${error.message}`));
    await page.route(url, (route) => route.fulfill({ contentType: "text/html", body: html }));
    await page.goto(url, { waitUntil: "load" });
    result.push({ id, result: await page.evaluate(probe) });
  } finally {
    await browser.close();
  }
}
writeFileSync(
  here("../fixtures/parity_080_chrome156.json"),
  `${JSON.stringify(
    {
      browser: `Chrome/${version}`,
      viewport: [800, 600],
      casesSha256: sha256(casesText),
      collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
      errors,
      result,
    },
    null,
    2,
  )}\n`,
);
