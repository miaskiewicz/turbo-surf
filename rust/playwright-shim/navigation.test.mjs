import assert from "node:assert/strict";
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
