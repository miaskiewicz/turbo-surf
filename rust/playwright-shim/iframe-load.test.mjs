import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let BrowserContext;
try {
  ({ BrowserContext } = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs"));
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("iframe load: native addon not built, skipping");
  process.exit(0);
}

test(
  "native iframe load completes its real child lifecycle beside a held and throwing peer",
  { timeout: 10000 },
  async () => {
    let releasePeer;
    const pendingPeer = new Promise((resolve) => {
      releasePeer = resolve;
    });
    const server = createServer(async (request, response) => {
      response.setHeader("content-type", "text/html");
      if (request.url === "/peer") {
        response.end(
          '<body><script>setTimeout(()=>{throw new Error("peer-only-timer")},0)</script><script src="/held.js"></script>',
        );
      } else if (request.url === "/held.js") {
        await pendingPeer;
        response.setHeader("content-type", "application/javascript");
        response.end('globalThis.peerReleased=true;throw new Error("peer-only-resource")');
      } else if (request.url === "/root") {
        response.end(
          '<body><iframe srcdoc="<body data-child=yes><script>globalThis.authoredChild=true;setTimeout(()=>{globalThis.authoredTimer=true},0)</script></body>"></iframe></body>',
        );
      } else response.end("<body>Ready</body>");
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const context = new BrowserContext();
    try {
      await context.addInitScript(() => {
        globalThis.initCount = (globalThis.initCount || 0) + 1;
        globalThis.lifecycle = [];
        addEventListener("load", () => {
          lifecycle.push([
            "load",
            document.readyState,
            window.frameElement ? authoredChild : frames[0]?.document.readyState,
          ]);
        });
      });
      const page = await context.newPage();
      await page.goto(origin + "/ready", { waitUntil: "networkidle" });
      const peerPromise = page.waitForEvent("popup");
      await page.evaluate(() => {
        window.open("about:blank");
      });
      const peer = await peerPromise;
      const peerErrors = [];
      peer.on("pageerror", (error) => peerErrors.push(error.message));
      await peer.goto(origin + "/peer", { waitUntil: "commit" });
      const rootErrors = [];
      page.on("pageerror", (error) => rootErrors.push(error.message));
      await page.goto(origin + "/root", { waitUntil: "networkidle", timeout: 2000 });
      assert.deepEqual(
        await page.evaluate(() => ({
          rootInit: initCount,
          childInit: frames[0].initCount,
          rootReady: document.readyState,
          childReady: frames[0].document.readyState,
          authored: frames[0].authoredChild,
          timer: frames[0].authoredTimer,
          rootEvents: lifecycle,
          childEvents: frames[0].lifecycle,
        })),
        {
          rootInit: 1,
          childInit: 1,
          rootReady: "complete",
          childReady: "complete",
          authored: true,
          timer: true,
          rootEvents: [["load", "complete", "complete"]],
          childEvents: [["load", "complete", true]],
        },
      );
      assert.deepEqual(rootErrors, []);
      releasePeer();
      await peer.waitForLoadState("load", { timeout: 2000 });
      assert.equal(await peer.evaluate(() => peerReleased), true);
      assert.ok(peerErrors.some((message) => message.includes("peer-only-")));
      assert.deepEqual(rootErrors, []);
    } finally {
      releasePeer();
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
