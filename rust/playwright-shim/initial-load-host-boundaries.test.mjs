import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { BrowserContext } from "./index.mjs";

async function boundaryFixture(binding = true) {
  let release;
  const held = new Promise((resolve) => (release = resolve));
  let load;
  const loaded = new Promise((resolve) => (load = resolve));
  let request;
  const requested = new Promise((resolve) => (request = resolve));
  let closing;
  const state = { released: false, requests: 0 };
  const server = createServer(async (req, response) => {
    if (req.url === "/replacement") {
      response.end("<!doctype html><h1>Replacement document</h1>");
    } else if (req.url === "/held") {
      state.requests++;
      request();
      response.writeHead(200, { "content-type": "text/plain" });
      response.flushHeaders();
      await held;
      response.end("Background delivered");
    } else {
      response.end(
        '<!doctype html><h1>Before</h1><script>window.addEventListener("load",()=>{console.log("load reached");fetch("/held").then(r=>r.text()).then(text=>document.querySelector("h1").textContent=text)});</script>' +
          (binding
            ? '<script type="module">try{const value=await actualHostReady();document.querySelector("h1").textContent=value;}catch(error){document.querySelector("h1").textContent="Rejected:"+error.message;}</script>'
            : ""),
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const context = new BrowserContext();
  const page = await context.newPage();
  page.on("console", (message) => {
    if (message.text() === "load reached") load();
  });
  return {
    page,
    loaded,
    requested,
    state,
    origin: `http://127.0.0.1:${server.address().port}`,
    release() {
      state.released = true;
      release();
    },
    close() {
      closing ??= (async () => {
        release();
        await context.close();
        server.closeAllConnections();
        await new Promise((resolve) => server.close(resolve));
      })();
      return closing;
    },
  };
}

async function delivered(f) {
  f.release();
  await f.page.waitForFunction(
    () => document.querySelector("h1").textContent === "Background delivered",
    undefined,
    { timeout: 1500 },
  );
  assert.equal(await f.page.locator("h1").innerText(), "Background delivered");
}

test(
  "load on a reused Page retains later background response delivery",
  { timeout: 10000 },
  async (t) => {
    const f = await boundaryFixture(false);
    t.after(() => f.close());
    await f.page.setContent("<h1>Existing actor</h1>");
    const navigation = f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 });
    navigation.catch(() => {});
    await Promise.all([f.loaded, f.requested]);
    assert.deepEqual(f.state, { released: false, requests: 1 });
    await navigation;
    assert.match(await f.page.content(), /Before/);
    await delivered(f);
  },
);

test(
  "live element reads observe loaded state while the same page retains a held response",
  { timeout: 10000 },
  async (t) => {
    const f = await boundaryFixture(false);
    t.after(() => f.close());
    const navigation = f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 });
    navigation.catch(() => {});
    await Promise.all([f.loaded, f.requested]);
    await navigation;
    const heading = f.page.locator("h1");
    assert.equal(await heading.innerText(), "Before");
    assert.equal(await heading.innerHTML(), "Before");
    assert.equal(await heading.isEnabled(), true);
    assert.equal(await heading.isEmpty(), false);
    assert.deepEqual(f.state, { released: false, requests: 1 });
    await delivered(f);
  },
);

test(
  "a genuine exposed-function rejection reaches the authored module before load",
  { timeout: 10000 },
  async (t) => {
    const f = await boundaryFixture();
    t.after(() => f.close());
    let calls = 0;
    await f.page.exposeFunction("actualHostReady", () => {
      calls++;
      throw new Error("Actual host callback failure");
    });
    const navigation = f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 });
    navigation.catch(() => {});
    await Promise.all([f.loaded, f.requested]);
    await navigation;
    assert.equal(calls, 1);
    assert.equal(f.state.released, false);
    assert.equal(await f.page.locator("h1").innerText(), "Rejected:Actual host callback failure");
    await delivered(f);
  },
);

test(
  "closing the actual Page from a pre-load host callback rejects its opening",
  { timeout: 10000 },
  async (t) => {
    const f = await boundaryFixture();
    t.after(() => f.close());
    let calls = 0;
    await f.page.exposeFunction("actualHostReady", async () => {
      calls++;
      await f.page.close();
      return "Closed document";
    });
    await assert.rejects(
      f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 }),
      /closed|cancel/i,
    );
    assert.equal(calls, 1);
    assert.equal(f.page.isClosed(), true);
    assert.equal(f.state.requests, 0);
  },
);

test(
  "a real replacement navigation supersedes the old pre-load host continuation",
  { timeout: 10000 },
  async (t) => {
    const f = await boundaryFixture();
    t.after(() => f.close());
    let calls = 0;
    let replacementCompleted = false;
    await f.page.exposeFunction("actualHostReady", async () => {
      calls++;
      await f.page.goto(`${f.origin}/replacement`, { waitUntil: "load", timeout: 1500 });
      replacementCompleted = true;
      throw new Error("Actual host callback failure after navigation");
    });
    await assert.rejects(
      f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 }),
      /Page session changed during initialization/,
    );
    assert.equal(calls, 1);
    assert.equal(replacementCompleted, true);
    assert.equal(f.page.isClosed(), false);
    assert.match(await f.page.content(), /Replacement document/);
    assert.equal(await f.page.evaluate(() => 42), 42);
    assert.equal(f.state.requests, 0);
  },
);
