import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
import { validateHistorySnapshot } from "./history-snapshot.mjs";
let native, newPage;
try {
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  ({ newPage } = await import("./index.mjs"));
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test(
  "native history metadata resolves exact root and child aliases without exposing stored state",
  { timeout: 10000 },
  async () => {
    const server = createServer((request, response) => {
      response.setHeader("content-type", "text/html");
      response.setHeader("cache-control", "no-store");
      response.end(`<body>${request.url}</body>`);
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const base = `http://127.0.0.1:${server.address().port}`;
    const root = newPage();
    let child;
    const state = (alias) =>
      validateHistorySnapshot(JSON.parse(native.liveTargetHistoryState(alias)));
    try {
      await root.goto(`${base}/root`, { waitUntil: "networkidle" });
      await root.evaluate(() =>
        history.pushState({ secret: "root-state-must-remain-native" }, "", "/root-spa"),
      );
      const rootState = state(root._session);
      assert.equal(rootState.entries[rootState.index].url, `${base}/root-spa`);
      const opened = root.waitForEvent("popup", { timeout: 2000 });
      opened.catch(() => {});
      await root.evaluate(() => {
        window.open("/child", "metadata-child");
      });
      child = await opened;
      await child.waitForLoadState("load", { timeout: 2000 });
      await child.evaluate(() =>
        history.pushState({ secret: "child-state-must-remain-native" }, "", "/child-spa"),
      );
      const childState = state(child._session);
      assert.equal(childState.entries[childState.index].url, `${base}/child-spa`);
      assert.notEqual(childState.document, rootState.document);
      assert.notEqual(childState.group, rootState.group);
      assert.equal(state(root._session).entries[rootState.index].url, `${base}/root-spa`);
      assert.doesNotMatch(JSON.stringify(rootState), /root-state-must-remain-native/);
      assert.doesNotMatch(JSON.stringify(childState), /child-state-must-remain-native/);
      assert.throws(() => native.liveTargetHistoryState(0xffffffff), /no such live session/);
      const childAlias = child._session;
      await child.close();
      assert.throws(
        () => native.liveTargetHistoryState(childAlias),
        /no such live session|history target is closed/,
      );
      assert.equal(state(root._session).entries[rootState.index].url, `${base}/root-spa`);
    } finally {
      await child?.close();
      await root.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
