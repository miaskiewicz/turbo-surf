// Records the Chrome control for tests/screenshot_web_fonts.rs (turbo-surf#142): the
// fixtures/web_fonts page, its linked sheet (an `@font-face` whose first source 404s and
// whose second resolves against the sheet's URL) and an inline `<style>` face, with IBM Plex
// Mono from harness/detection/fixtures/fonts, served at http://127.0.0.1/ via route
// interception (nothing leaves the machine) in an 800x600 page at device scale 1. Records
// each span's box and each bar's top, which the static screenshot must paint alike.
// Regenerate (frozen headless Chrome 156):
//   TURBO_SURF_CHROME_PATH=".../chrome-headless-shell" \
//     node rust/crates/turbo-surf-mcp/tests/fixture-gen/screenshot-web-fonts-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");
const fonts = "../../../../../harness/detection/fixtures/fonts/ibm-plex-mono/IBMPlexMono-Regular";
const files = {
  "/": ["text/html", readFileSync(here("../fixtures/web_fonts/page.html"))],
  "/css/fonts.css": ["text/css", readFileSync(here("../fixtures/web_fonts/fonts.css"))],
  "/fonts/plex.woff2": ["font/woff2", readFileSync(here(`${fonts}.woff2`))],
  "/fonts/plex.ttf": ["font/ttf", readFileSync(here(`${fonts}.ttf`))],
};
const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--use-mock-keychain", "--password-store=basic", "--force-device-scale-factor=1"],
});
try {
  const errors = [];
  const page = await browser.newPage({ viewport: { width: 800, height: 600 } });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("http://127.0.0.1/**", (route) => {
    const file = files[new URL(route.request().url()).pathname];
    if (!file) return route.fulfill({ status: 404, body: "" });
    return route.fulfill({ contentType: file[0], body: file[1] });
  });
  await page.goto("http://127.0.0.1/", { waitUntil: "load" });
  const result = await page.evaluate(async () => {
    await document.fonts.ready;
    const box = (id) => {
      const r = document.getElementById(id).getBoundingClientRect();
      return [r.x, r.y, r.width, r.height];
    };
    return {
      fonts: [...document.fonts].map((face) => [face.family, face.status]),
      m: box("m"),
      n: box("n"),
      bar: box("bar")[1],
      bar2: box("bar2")[1],
    };
  });
  writeFileSync(
    here("../fixtures/screenshot_web_fonts_chrome156.json"),
    `${JSON.stringify(
      {
        browser: `Chrome/${browser.version()}`,
        viewport: [800, 600],
        filesSha256: Object.fromEntries(
          Object.entries(files).map(([path, [, bytes]]) => [path, sha256(bytes)]),
        ),
        collectorSha256: sha256(readFileSync(new URL(import.meta.url))),
        errors,
        result,
      },
      null,
      2,
    )}\n`,
  );
} finally {
  await browser.close();
}
