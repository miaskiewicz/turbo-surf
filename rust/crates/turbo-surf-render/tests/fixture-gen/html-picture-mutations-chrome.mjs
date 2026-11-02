// Records the Chrome control for tests/html_image_srcset_selection.rs (turbo-dom#71, surf's
// load path): serves tests/fixtures/html_picture_mutations_cases.json's page at /page/ and
// /img/<W>x<H>/<name>.png as W x H PNGs, runs tests/fixtures/html_picture_mutations_probe.js
// after load at 1024x768, devicePixelRatio 1, and writes
// tests/fixtures/html_picture_mutations_chrome156.json. Regenerate (Chrome for Testing):
//   TURBO_SURF_CHROME_PATH=".../Google Chrome for Testing" \
//     node rust/crates/turbo-surf-render/tests/fixture-gen/html-picture-mutations-chrome.mjs
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { deflateSync } from "node:zlib";
import { chromium } from "playwright";

const here = (path) => new URL(path, import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const probe = readFileSync(here("../fixtures/html_picture_mutations_probe.js"), "utf8");
const casesText = readFileSync(here("../fixtures/html_picture_mutations_cases.json"), "utf8");
const executablePath = process.env.TURBO_SURF_CHROME_PATH;
if (!executablePath) throw new Error("set TURBO_SURF_CHROME_PATH to the Chrome binary");

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc = (bytes) => {
  let c = 0xffffffff;
  for (const byte of bytes) c = crcTable[(c ^ byte) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (kind, data) => {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(kind, 4, "latin1");
  data.copy(out, 8);
  out.writeUInt32BE(crc(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
};
const png = (width, height) => {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 0, 0, 0, 0], 8);
  const raw = Buffer.alloc((width + 1) * height, 0x80);
  for (let row = 0; row < height; row++) raw[row * (width + 1)] = 0;
  return Buffer.concat([
    Buffer.from("\x89PNG\r\n\x1a\n", "latin1"),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
};

const cases = JSON.parse(casesText);
const server = createServer((request, response) => {
  const path = new URL(request.url, "http://127.0.0.1").pathname;
  const size = path.match(/^\/img\/(\d+)x(\d+)\//);
  if (path === "/page/")
    response.writeHead(200, { "content-type": "text/html" }).end(cases[0].html);
  else if (size)
    response
      .writeHead(200, { "content-type": "image/png", "cache-control": "no-store" })
      .end(png(Number(size[1]), Number(size[2])));
  else response.writeHead(404).end();
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const errors = [];
  const result = [];
  for (const { id } of cases) {
    const page = await browser.newPage({
      viewport: { width: 1024, height: 768 },
      deviceScaleFactor: 1,
    });
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.goto(`http://127.0.0.1:${server.address().port}/page/`, { waitUntil: "load" });
    result.push({ id, result: await page.evaluate(probe) });
    await page.close();
  }
  writeFileSync(
    here("../fixtures/html_picture_mutations_chrome156.json"),
    `${JSON.stringify({ browser: `Chrome/${browser.version()}`, probeSha256: sha256(probe), casesSha256: sha256(casesText), collectorSha256: sha256(readFileSync(new URL(import.meta.url))), errors, result }, null, 2)}\n`,
  );
} finally {
  await browser.close();
  server.close();
}
