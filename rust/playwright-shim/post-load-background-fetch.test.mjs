import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { BrowserContext } from "./index.mjs";

async function postLoadFixture(mode = "fetch", { binding = false } = {}) {
  let release;
  const held = new Promise((resolve) => (release = resolve));
  let requested;
  const requestSeen = new Promise((resolve) => (requested = resolve));
  let loaded;
  const loadSeen = new Promise((resolve) => (loaded = resolve));
  let initialized;
  const initializedSeen = new Promise((resolve) => (initialized = resolve));
  let closing;
  const state = { requests: 0, released: false, loadDispatched: false, initialized: false };
  const server = createServer(async (request, response) => {
    if (request.url === "/held") {
      state.requests++;
      requested();
      if (mode === "fetch") {
        response.writeHead(200, { "content-type": "text/plain" });
        response.flushHeaders();
      }
      await held;
      if (mode === "image") {
        response.writeHead(200, { "content-type": "image/svg+xml" });
        response.end('<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>');
      } else {
        response.end(mode === "fetch" ? "Background delivered" : "window.requiredScriptRan=true;");
      }
      return;
    }
    response.writeHead(200, { "content-type": "text/html" });
    response.end(
      '<!doctype html><form><input name="name"><button>Save</button></form><h1>Before</h1>' +
        (binding
          ? '<script type="module">const reply=await actualHostReady("before-load");document.querySelector("h1").textContent=reply;</script>'
          : "") +
        '<script>console.log("parser initialized");window.addEventListener("load",()=>{console.log("load dispatched");' +
        (mode === "fetch"
          ? 'fetch("/held").then(response=>response.text()).then(text=>document.querySelector("h1").textContent=text);'
          : "") +
        "});</script>" +
        (mode === "script"
          ? '<script src="/held"></script>'
          : mode === "image"
            ? '<img src="/held" alt="Required logo">'
            : ""),
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const context = new BrowserContext();
  const page = await context.newPage();
  page.on("console", (message) => {
    if (message.text() === "parser initialized") {
      state.initialized = true;
      initialized();
    }
    if (message.text() === "load dispatched") {
      state.loadDispatched = true;
      loaded();
    }
  });
  return {
    page,
    state,
    requestSeen,
    loadSeen,
    initializedSeen,
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

for (const waitUntil of ["default", "load", "networkidle"])
  test(
    `post-load held fetch preserves the ${waitUntil} navigation milestone`,
    { timeout: 10000 },
    async () => {
      const f = await postLoadFixture();
      try {
        const navigation = f.page
          .goto(f.origin, { ...(waitUntil === "default" ? {} : { waitUntil }), timeout: 1500 })
          .then(
            () => ({ completed: true }),
            (error) => ({ completed: false, name: error.name, message: error.message }),
          );
        await Promise.all([f.loadSeen, f.requestSeen]);
        assert.deepEqual(f.state, {
          requests: 1,
          released: false,
          loadDispatched: true,
          initialized: true,
        });
        const result = await navigation;
        if (waitUntil === "networkidle") {
          assert.equal(result.completed, false);
          assert.equal(result.name, "TimeoutError");
        } else {
          assert.equal(result.completed, true, JSON.stringify({ result, state: f.state }));
          assert.match(await f.page.content(), /Before/);
          f.release();
          await f.page.waitForFunction(
            () => document.querySelector("h1").textContent === "Background delivered",
            undefined,
            { timeout: 1500 },
          );
          assert.equal(await f.page.locator("h1").innerText(), "Background delivered");
        }
      } finally {
        console.log(JSON.stringify({ waitUntil, state: f.state }));
        await f.close();
      }
    },
  );

for (const mode of ["script", "image"])
  test(`a held required ${mode} still blocks load`, { timeout: 10000 }, async () => {
    const f = await postLoadFixture(mode);
    try {
      const navigation = f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 }).then(
        () => ({ completed: true }),
        (error) => ({ completed: false, name: error.name, message: error.message }),
      );
      await Promise.all([f.initializedSeen, f.requestSeen]);
      assert.equal(f.state.loadDispatched, false);
      const result = await navigation;
      assert.equal(result.completed, false);
      assert.equal(result.name, "TimeoutError");
      assert.equal(f.state.released, false);
    } finally {
      console.log(JSON.stringify({ mode, state: f.state }));
      await f.close();
    }
  });

test(
  "a required pre-load host binding finishes before load while later fetch remains live",
  { timeout: 10000 },
  async (t) => {
    const f = await postLoadFixture("fetch", { binding: true });
    t.after(() => f.close());
    const calls = [];
    try {
      await f.page.exposeFunction("actualHostReady", (message) => {
        calls.push(message);
        return "Host binding delivered";
      });
      const navigation = f.page.goto(f.origin, { waitUntil: "load", timeout: 1500 }).then(
        () => ({ completed: true }),
        (error) => ({ completed: false, name: error.name, message: error.message }),
      );
      await Promise.all([f.loadSeen, f.requestSeen]);
      assert.deepEqual(calls, ["before-load"]);
      assert.equal(f.state.released, false);
      const result = await navigation;
      assert.equal(result.completed, true, JSON.stringify({ result, state: f.state, calls }));
      assert.match(await f.page.content(), /Host binding delivered/);
      f.release();
      await f.page.waitForFunction(
        () => document.querySelector("h1").textContent === "Background delivered",
        undefined,
        { timeout: 1500 },
      );
      assert.equal(await f.page.locator("h1").innerText(), "Background delivered");
    } finally {
      console.log(JSON.stringify({ binding: true, calls, state: f.state }));
      await f.close();
    }
  },
);
