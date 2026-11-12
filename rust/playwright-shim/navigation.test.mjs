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
      response.end(
        '<body data-keep="yes"><a id="download" href="/attachment">Download</a><a id="hidden" style="display:none" href="/attachment">Hidden</a><button id="disabled" disabled>Disabled</button></body>',
      );
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
    await assert.rejects(
      page.locator("#hidden").click({ timeout: 30 }),
      (error) => error.name === "TimeoutError",
    );
    await assert.rejects(
      page.locator("#disabled").click({ timeout: 30 }),
      (error) => error.name === "TimeoutError",
    );
    assert.equal(page._live, false);
    assert.equal(attachmentRequests, 1);
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

test("waitForURL keeps waiting for a later driver navigation within its timeout", async () => {
  const page = shim.newPage();
  let change;
  try {
    await page.setContent("<body>protected</body>");
    const navigated = new Promise((resolve, reject) => {
      change = setTimeout(() => {
        page.evaluate(() => history.pushState({}, "", "#login")).then(resolve, reject);
      }, 300);
    });
    await page.waitForURL("**#login", { timeout: 2000 });
    await navigated;
    assert.equal(page.url(), "about:blank#login");
  } finally {
    clearTimeout(change);
    await page.close();
  }
});

test("waitForURL honors zero timeout and URL predicate/RegExp overloads", async () => {
  const page = shim.newPage();
  let change;
  try {
    await page.setContent("<body>protected</body>");
    const navigated = new Promise((resolve, reject) => {
      change = setTimeout(
        () => page.evaluate(() => history.pushState({}, "", "#ready")).then(resolve, reject),
        100,
      );
    });
    await page.waitForURL((url) => url.hash === "#ready", { timeout: 0, waitUntil: "commit" });
    await navigated;
    await page.waitForURL(/#ready$/, { timeout: 50 });
  } finally {
    clearTimeout(change);
    await page.close();
  }
});

test("waitForURL expires at its deadline and stops when the page closes", async () => {
  const page = shim.newPage();
  try {
    await page.setContent("<body>protected</body>");
    const start = Date.now();
    await assert.rejects(page.waitForURL("**#missing", { timeout: 120 }), { name: "TimeoutError" });
    assert.ok(Date.now() - start >= 100);
    const wait = page.waitForURL("**#missing", { timeout: 0 });
    await page.close();
    await assert.rejects(wait, /Page closed/);
  } finally {
    await page.close();
  }
});

test("a URL wait timeout leaves a pending host callback and sibling URL wait alive", async () => {
  const page = shim.newPage();
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  let timer;
  try {
    await page.setContent("<body>protected</body>");
    await page.exposeFunction("finishLogin", async () => {
      await gate;
      return "success";
    });
    await page._evalEngine(
      "finishLogin().then(value => { globalThis.loginResult = value; history.pushState({}, '', '#login'); }); globalThis.__RESULT = ''; ",
    );
    const loser = page.waitForURL("**#missing", { timeout: 80 });
    const winner = page.waitForURL("**#login", { timeout: 2000 });
    timer = setTimeout(release, 200);
    await assert.rejects(loser, { name: "TimeoutError" });
    await winner;
    assert.equal(await page.evaluate(() => globalThis.loginResult), "success");
  } finally {
    clearTimeout(timer);
    release();
    await page.close();
  }
});

test("a URL timeout does not abort an unrelated routed request", async () => {
  const server = createServer((request, response) =>
    response.end(request.url === "/slow" ? "success" : "<body>protected</body>"),
  );
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  let timer;
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page._ensureLive();
    await page.route("**/slow", async (route) => {
      await gate;
      await route.continue();
    });
    await page._evalEngine(
      "fetch('/slow').then(r => r.text()).then(value => { globalThis.loginResult = value; history.pushState({}, '', '#login'); }); globalThis.__RESULT = ''; ",
    );
    const loser = page.waitForURL("**#missing", { timeout: 80 });
    const winner = page.waitForURL("**#login", { timeout: 2000 });
    timer = setTimeout(release, 200);
    await assert.rejects(loser, { name: "TimeoutError" });
    await winner;
    assert.equal(await page.evaluate(() => globalThis.loginResult), "success");
  } finally {
    clearTimeout(timer);
    release();
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("waitForURL can start before a static driver navigation", async () => {
  const server = createServer((_request, response) => response.end("<body>destination</body>"));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  let change;
  try {
    const url = `http://127.0.0.1:${server.address().port}/destination`;
    const navigated = new Promise((resolve, reject) => {
      change = setTimeout(() => page.goto(url, { waitUntil: "commit" }).then(resolve, reject), 100);
    });
    await page.waitForURL(url, { timeout: 2000, waitUntil: "commit" });
    await navigated;
    assert.equal(page.url(), url);
    assert.equal(page._live, false);
  } finally {
    clearTimeout(change);
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
