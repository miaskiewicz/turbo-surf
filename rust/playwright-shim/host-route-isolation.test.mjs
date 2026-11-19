import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
let newPage;
let native;
try {
  ({ newPage } = await import("./index.mjs"));
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test(
  "a Page binding deadline preserves a peer's held route in the same actor",
  { timeout: 10000 },
  async () => {
    const server = createServer((req, res) => {
      res.setHeader("content-type", "text/html");
      res.end(
        req.url === "/deadline"
          ? '<script>fetch("/a").catch(()=>{})</script>'
          : "<button onclick=\"window.open('/peer')\">peer</button>",
      );
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const page = newPage();
    let release;
    const held = new Promise((resolve) => {
      release = resolve;
    });
    let peerResult;
    let peerError;
    let peerPending;
    let peerEntered = false;
    try {
      await page.goto(origin, { waitUntil: "networkidle", timeout: 0 });
      const opened = page.waitForEvent("popup", { timeout: 2000 });
      await page.locator("button").click();
      const peer = await opened;
      await peer.waitForURL(origin + "/peer", { timeout: 2000 });
      assert.equal(Reflect.get(page, "_actorHub"), Reflect.get(peer, "_actorHub"));
      const state = JSON.parse(native.liveAuxiliaryState(Reflect.get(page, "_session")));
      assert.equal(state.root, Reflect.get(page, "_targetId"));
      assert.ok(
        state.pages.some(
          (entry) => entry.target === Reflect.get(peer, "_targetId") && !entry.closed,
        ),
      );
      await peer.route("**/b", async (route) => {
        peerEntered = true;
        await held;
        await route.fulfill({ body: "peer-held" });
      });
      await page.route("**/a", async () => {
        peerPending = peer.evaluate(async () => (await fetch("/b")).text());
        peerPending.then(
          (value) => {
            peerResult = value;
          },
          (error) => {
            peerError = error;
          },
        );
        await new Promise(() => {});
      });
      await assert.rejects(
        page.goto(origin + "/deadline", { waitUntil: "networkidle", timeout: 1500 }),
        (error) => error.name === "TimeoutError",
      );
      assert.equal(peerEntered, true, "the peer route was genuinely pending during A's deadline");
      assert.equal(peerError, undefined, "A's deadline must not reject B's pending fetch");
      assert.equal(peerResult, undefined);
      release();
      assert.equal(await peerPending, "peer-held");
      assert.equal(peer.isClosed(), false);
      await peer.unroute("**/b");
      await peer.route("**/b", (route) => route.fulfill({ body: "peer-fresh" }));
      assert.equal(await peer.evaluate(async () => (await fetch("/b")).text()), "peer-fresh");
    } finally {
      release();
      await page.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
