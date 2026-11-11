import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { newPage } from "./index.mjs";

const pixel = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

async function withImages(run) {
  let active = 0;
  let peak = 0;
  let requests = 0;
  const server = createServer((req, res) => {
    if (!req.url.startsWith("/img/")) return res.end("<!doctype html><body></body>");
    requests++;
    peak = Math.max(peak, ++active);
    res.writeHead(200, { "content-type": "image/png", "cache-control": "no-store" });
    setTimeout(() => {
      active--;
      res.end(pixel);
    }, 20);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "load" });
    // Install a static snapshot without initiating separate live image requests.
    page._html =
      "<!doctype html><body>" +
      Array.from({ length: 12 }, (_, i) => `<img src="/img/${i}.png" width="8" height="8">`).join(
        "",
      ) +
      "</body>";
    await run(page, () => ({ requests, peak }));
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("screenshot image preparation overlaps independent fetches within its bound", async () => {
  await withImages(async (page, stats) => {
    const png = await page.screenshot({ externalCss: false });
    assert.equal(png.readUInt32BE(16), 1280);
    assert.equal(stats().requests, 12);
    assert.ok(stats().peak > 1, "independent image fetches must overlap");
    assert.ok(stats().peak <= 8, "image preparation must remain bounded");
  });
});
