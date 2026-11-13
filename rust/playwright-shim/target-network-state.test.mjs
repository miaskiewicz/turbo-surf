import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

let native;
try {
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const pending = new Map();
  const arrivals = new Map();
  const actors = [];
  const arrived = (path) => {
    const existing = arrivals.get(path);
    if (existing) return existing.promise;
    let resolve;
    const promise = new Promise((done) => {
      resolve = done;
    });
    arrivals.set(path, { promise, resolve });
    return promise;
  };
  const server = createServer((request, response) => {
    if (request.url.startsWith("/hold")) {
      arrived(request.url);
      pending.set(request.url, response);
      arrivals.get(request.url).resolve();
      return;
    }
    response.setHeader("content-type", "text/html");
    response.end("<body>target</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}/`;
  const open = async () => {
    const root = await native.liveOpen("<body>root</body>", base, "[]", "");
    actors.push(root);
    return root;
  };
  const state = (id) => JSON.parse(native.liveTargetNetworkState(id));
  try {
    await run({ open, state, base, arrived, pending });
  } finally {
    for (const response of pending.values()) response.end("released");
    for (const actor of actors) native.liveActorClose(actor);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("target activity starts before headers and lasts through body EOF", { timeout: 10000 }, () =>
  fixture(async ({ open, state, arrived, pending }) => {
    const root = await open();
    const initial = state(root);
    const received = arrived("/hold-body");
    const request = native.liveEvalEngineExpression(
      root,
      "fetch('/hold-body').then(response=>response.text())",
    );
    request.catch(() => {});
    await received;
    assert.equal(state(root).pendingRequests, 1);
    assert.notEqual(state(root).revision, initial.revision);
    assert.equal(state(root).error, null);
    const response = pending.get("/hold-body");
    response.writeHead(200, { "content-type": "text/plain" });
    response.write("first");
    assert.equal(state(root).pendingRequests, 1);
    response.end("last");
    assert.equal(await request, "firstlast");
    assert.equal(state(root).pendingRequests, 0);
    assert.equal(state(root).closed, false);
  }),
);

test("a held child request does not change its opener network revision", { timeout: 10000 }, () =>
  fixture(async ({ open, state, arrived, pending }) => {
    const root = await open();
    await native.liveEvalEngineExpression(
      root,
      "!!(globalThis.child=window.open('about:blank','idle-child'))",
    );
    const info = JSON.parse(native.liveAuxiliaryState(root));
    const child = native.liveAuxiliaryAdopt(
      root,
      info.events.find((event) => event.kind === "created").target,
    );
    const rootQuiet = state(root);
    const received = arrived("/hold-child");
    const request = native.liveEvalEngineExpression(
      child,
      "fetch('/hold-child').then(response=>response.text())",
    );
    request.catch(() => {});
    await received;
    assert.equal(state(child).pendingRequests, 1);
    assert.equal(state(child).error, null);
    assert.deepEqual(state(root), rootQuiet);
    pending.get("/hold-child").end("child");
    assert.equal(await request, "child");
    assert.equal(state(child).pendingRequests, 0);
    assert.deepEqual(state(root), rootQuiet);
  }),
);

test(
  "a queued and loading navigation remains busy until real load completion",
  { timeout: 10000 },
  () =>
    fixture(async ({ open, state, base, arrived, pending }) => {
      const root = await open();
      const received = arrived("/hold-navigation");
      const navigation = native.liveNavigateTarget(root, `${base}hold-navigation`, "push");
      navigation.catch(() => {});
      await received;
      assert.equal(state(root).pendingNavigation, true);
      pending.get("/hold-navigation").setHeader("content-type", "text/html");
      pending.get("/hold-navigation").end("<body>committed</body>");
      await navigation;
      // The native command acknowledges commit before the retained actor pumps load.
      await native.liveEvalEngine(root, "");
      assert.equal(state(root).pendingNavigation, false);
      assert.equal(state(root).pendingRequests, 0);
    }),
);
