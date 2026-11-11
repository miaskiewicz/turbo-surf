import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("Blob downloads preserve every byte in path, saveAs and streams", async () => {
  const page = shim.newPage();
  const directory = await mkdtemp(join(tmpdir(), "surf-download-test-"));
  let downloadPath;
  try {
    await page.setContent("<body></body>");
    await page._ensureLive();
    await page.evaluate(() => {
      const url = URL.createObjectURL(new Blob([new Uint8Array([0, 255, 128, 10])]));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "fixture.bin";
      anchor.click();
      URL.revokeObjectURL(url);
    });
    const download = await page.waitForEvent("download", { timeout: 1000 });
    downloadPath = await download.path();
    const expected = Buffer.from([0, 255, 128, 10]);
    assert.deepEqual(await readFile(downloadPath), expected);
    await download.saveAs(join(directory, "saved.bin"));
    assert.deepEqual(await readFile(join(directory, "saved.bin")), expected);
    const chunks = [];
    for await (const chunk of await download.createReadStream()) chunks.push(chunk);
    assert.deepEqual(Buffer.concat(chunks), expected);
  } finally {
    await page.close();
    await rm(directory, { recursive: true, force: true });
    if (downloadPath) await rm(dirname(downloadPath), { recursive: true, force: true });
  }
});

test("HTTP attachments emit binary downloads without replacing the live document", async () => {
  let attachmentRequests = 0;
  const bytes = Buffer.from([0, 255, 128, 10]);
  const server = createServer((request, response) => {
    if (request.url === "/attachment") {
      attachmentRequests++;
      response.setHeader(
        "content-disposition",
        "attachment; filename=fallback.bin; filename*=UTF-8''f%C3%ADxture.bin",
      );
      response.setHeader("content-type", "application/octet-stream");
      response.end(bytes);
    } else
      response.end('<body data-keep="yes"><a id="download" href="/attachment">Download</a></body>');
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}/`;
  const page = shim.newPage();
  let downloadPath;
  try {
    await page.goto(origin, { waitUntil: "networkidle" });
    const events = [];
    page.on("download", (download) => events.push(download));
    const waiting = page.waitForEvent("download", { timeout: 300 });
    await page.locator("#download").click();
    const download = await waiting;
    assert.ok(download, "an attachment must produce a real Download");
    assert.equal(download.suggestedFilename(), "fíxture.bin");
    downloadPath = await download.path();
    assert.deepEqual(await readFile(downloadPath), bytes);
    assert.equal(events.length, 1);
    assert.equal(attachmentRequests, 1);
    assert.equal(page.url(), origin);
    assert.equal(await page.evaluate(() => document.body.dataset.keep), "yes");
    await assert.rejects(
      page.waitForEvent("download", { timeout: 30 }),
      (error) => error.name === "TimeoutError",
    );
    await page.goto(origin, { waitUntil: "commit" });
    const staticWaiting = page.waitForEvent("download", { timeout: 300 });
    await page.locator("#download").click();
    assert.equal((await staticWaiting).suggestedFilename(), "fíxture.bin");
    assert.equal(page._live, false);
    assert.equal(attachmentRequests, 2);
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
    if (downloadPath) await rm(dirname(downloadPath), { recursive: true, force: true });
  }
});
