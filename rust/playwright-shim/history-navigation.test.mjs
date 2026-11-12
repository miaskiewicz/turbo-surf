import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let newPage;
try {
  ({ newPage } = await import("./index.mjs"));
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const requests = [];
  const server = createServer((req, res) => {
    requests.push(req.url);
    res.setHeader("content-type", "text/html");
    res.end(
      `<button onclick="history.pushState({step:1},'', '/target');document.body.dataset.spa='yes'">push</button><script>window.marker={};window.pop=[];addEventListener('popstate',e=>pop.push(e.state));</script>`,
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
    await run(page, requests);
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("same-document push and shim traversal preserve the live Window and state (#319)", async () => {
  await fixture(async (page, requests) => {
    const frames = [];
    page.on("framenavigated", (frame) => frames.push(new URL(frame.url()).pathname));
    await page.locator("button").click();
    assert.equal(await page.evaluate(() => document.body.dataset.spa), "yes");
    assert.deepEqual(requests, ["/"]);
    assert.equal(await page.goBack(), null);
    assert.equal(new URL(page.url()).pathname, "/");
    assert.equal(await page.evaluate(() => document.body.dataset.spa), "yes");
    assert.equal(await page.evaluate(() => history.state), null);
    assert.equal(await page.goForward(), null);
    assert.deepEqual(await page.evaluate(() => history.state), { step: 1 });
    assert.deepEqual(frames, ["/target", "/", "/target"]);
    assert.deepEqual(requests, ["/"]);
  });
});

test("nested replacements and repeated URL entries retain identity (#319)", async () => {
  await fixture(async (page, requests) => {
    await page.evaluate(() => {
      window.savedMarker = marker;
      navigation.addEventListener("currententrychange", function nested() {
        navigation.removeEventListener("currententrychange", nested);
        history.replaceState({ step: 2 }, "", "/same");
        history.pushState({ step: 3 }, "", "/same");
      });
      history.pushState({ step: 1 }, "", "/same");
    });
    assert.deepEqual(await page.evaluate(() => history.state), { step: 3 });
    await page.goBack();
    assert.deepEqual(await page.evaluate(() => history.state), { step: 2 });
    await page.goBack();
    assert.equal(new URL(page.url()).pathname, "/");
    await page.goForward();
    assert.deepEqual(await page.evaluate(() => history.state), { step: 2 });
    await page.evaluate(() => history.forward());
    assert.deepEqual(await page.evaluate(() => history.state), { step: 3 });
    assert.equal(await page.evaluate(() => marker === savedMarker), true);
    assert.deepEqual(requests, ["/"]);
  });
});

test("real Location navigation and same-URL reload still fetch and execute (#319)", async () => {
  await fixture(async (page, requests) => {
    await page.evaluate(() => {
      history.pushState({ x: 1 }, "", "/target");
      location.assign("/real");
    });
    assert.equal(new URL(page.url()).pathname, "/real");
    assert.equal(await page.evaluate(() => document.body.dataset.spa), undefined);
    await page.evaluate(() => {
      window.beforeReload = true;
      location.reload();
    });
    assert.equal(await page.evaluate(() => window.beforeReload), undefined);
    assert.deepEqual(requests, ["/", "/real", "/real"]);
    await page.evaluate(() => location.assign(location.href));
    assert.deepEqual(requests, ["/", "/real", "/real", "/real"]);
    await page.evaluate(() => location.assign("/next"));
    await page.evaluate(() => location.replace("/replacement"));
    assert.equal((await page.goBack({ waitUntil: "networkidle" })).status(), 200);
    assert.equal(new URL(page.url()).pathname, "/real");
    assert.equal((await page.goForward({ waitUntil: "networkidle" })).status(), 200);
    assert.equal(new URL(page.url()).pathname, "/replacement");
  });
});
