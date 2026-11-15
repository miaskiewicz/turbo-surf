import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
import { BrowserContext, Page } from "./index.mjs";
const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");

for (const mode of ["fetch", "classic-script"])
  test(
    `closing an opening root cancels its ${mode} body and preserves its actual popup`,
    { timeout: 10000 },
    async () => {
      let release;
      const held = new Promise((resolve) => (release = resolve));
      let rootBodies = 0;
      let recordAbortedBody;
      const abortedBody = new Promise((resolve) => (recordAbortedBody = resolve));
      const server = createServer(async (request, response) => {
        if (request.url === "/slow") {
          rootBodies++;
          response.once("close", () => {
            if (!response.writableFinished) recordAbortedBody();
          });
          response.writeHead(200, {
            "content-type": mode === "fetch" ? "application/json" : "text/javascript",
          });
          response.flushHeaders();
          await held;
          response.end(mode === "fetch" ? '{"ok":true}' : "window.closedRootScriptRan = true;");
        } else if (request.url === "/survivor") {
          response.end("surviving-target");
        } else {
          response.end(
            '<script>window.open("about:blank");</script>' +
              (mode === "fetch"
                ? '<script>fetch("/slow").catch(()=>{});</script>'
                : '<script src="/slow"></script>'),
          );
        }
      });
      await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
      const origin = `http://127.0.0.1:${server.address().port}`;
      const context = new BrowserContext();
      const page = await context.newPage();
      let navigation;
      try {
        const popupEvent = page.waitForEvent("popup");
        const headerEvent = page.waitForResponse((response) => response.url() === `${origin}/slow`);
        navigation = page.goto(origin, { waitUntil: "networkidle" });
        navigation.catch(() => {});
        const [popup, response] = await Promise.all([popupEvent, headerEvent]);
        const body = mode === "fetch" ? response.json() : response.text();
        body.catch(() => {});
        await assert.rejects(
          async () => native.liveCloseOpeningTarget(popup._session, page._targetId),
          /actual actor token/,
        );
        await assert.rejects(
          async () => native.liveCloseOpeningTarget(page._actorHub.actor, popup._targetId),
          /root target ownership/,
        );
        await assert.rejects(
          async () => native.liveCloseOpeningTarget(page._actorHub.actor, "18446744073709551615"),
          /ownership/,
        );
        await Promise.all([page.close(), page.close()]);
        await page.close();
        await abortedBody;
        await assert.rejects(body, /closed|cancel/i);
        await assert.rejects(navigation, /closed|cancel/i);
        assert.equal(page.isClosed(), true);
        assert.equal(popup.isClosed(), false);
        assert.equal(response.status(), 200);
        assert.equal(response.url(), `${origin}/slow`);
        assert.equal(response.request().frame(), page.mainFrame());
        assert.equal(rootBodies, 1);
        await page._actorHub.drain();
        assert.equal(
          [...page._actorHub.requests.values()].includes(page),
          false,
          "actual cancellation terminal must retire the root host response owner while the popup survives",
        );
        assert.throws(
          () => native.liveTargetNetworkState(page._actorHub.actor),
          /no such live session|closed/,
        );
        const closedState = JSON.parse(
          await native.liveCloseOpeningTarget(page._actorHub.actor, page._targetId),
        );
        assert.equal(closedState.actor, page._actorHub.actor);
        assert.equal(closedState.root, page._targetId);
        assert.equal(closedState.pages.find((info) => info.target === page._targetId).closed, true);
        assert.equal(
          closedState.pages.find((info) => info.target === popup._targetId).closed,
          false,
        );
        assert.equal(await popup.evaluate(() => 6 * 7), 42);
        assert.equal(
          await popup.evaluate(async (url) => (await fetch(url)).text(), `${origin}/survivor`),
          "surviving-target",
        );
      } finally {
        release();
        await navigation?.catch(() => {});
        await context.close();
        server.closeAllConnections();
        await new Promise((resolve) => server.close(resolve));
      }
    },
  );

// Opening can finish before the first close's native receipt reaches the host.
test("a concurrent close joins its owned pending close after opening ends", async () => {
  let release;
  const pending = new Promise((resolve) => (release = resolve));
  const page = Object.create(Page.prototype);
  page._openingClosePromise = pending;
  page._ownsContext = false;
  let nativeCloses = 0;
  page._actorHub = {
    opening: false,
    capability: "supported",
    ready: Promise.resolve(),
    closeTarget: async () => {
      nativeCloses++;
    },
  };
  let completed = false;
  const closing = page.close().then(() => (completed = true));
  await Promise.resolve();
  assert.equal(completed, false);
  release(true);
  await closing;
  assert.equal(completed, true);
  assert.equal(nativeCloses, 0, "a pending target close must not be issued twice");
});

test("an opening child never requests root cancellation", async () => {
  const page = Object.create(Page.prototype);
  page._targetId = "child";
  page._actorHub = { opening: true, capability: "supported", root: "root" };
  assert.equal(await page._closeOpeningTarget(), false);
  page._targetId = null;
  page._actorHub.root = null;
  assert.equal(await page._closeOpeningTarget(), false);
});

test("an owned opening-close acknowledgement never queues another native close", () => {
  const saved = native.liveClose;
  let closes = 0;
  let released = 0;
  native.liveClose = () => {
    closes++;
    return null;
  };
  try {
    const page = Object.create(Page.prototype);
    page._session = 4294967294;
    page._openingClosePromise = Promise.resolve(true);
    page._actorHub = { opening: false, retiringAliases: new Set() };
    page._releaseNativePageResources = () => {
      released++;
    };
    assert.equal(page._didNativeClose(), null);
    assert.equal(page._closed, true);
    assert.equal(page._didNativeClose(), null);
    assert.equal(closes, 0);
    assert.equal(released, 1);
    assert.equal(page._actorHub.retiringAliases.size, 0);
  } finally {
    native.liveClose = saved;
  }
});

test(
  "concurrent opening-root and last-popup closes complete the held native response",
  { timeout: 10000 },
  async () => {
    let release, recordAbort;
    const held = new Promise((resolve) => (release = resolve));
    const aborted = new Promise((resolve) => (recordAbort = resolve));
    const server = createServer(async (request, response) => {
      if (request.url === "/slow") {
        response.once("close", () => {
          if (!response.writableFinished) recordAbort();
        });
        response.writeHead(200, { "content-type": "application/json" });
        response.flushHeaders();
        await held;
        response.end('{"ok":true}');
      } else
        response.end('<script>window.open("about:blank");fetch("/slow").catch(()=>{});</script>');
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const context = new BrowserContext();
    const page = await context.newPage();
    let navigation;
    try {
      const popupEvent = page.waitForEvent("popup");
      const headers = page.waitForResponse((response) => response.url() === `${origin}/slow`);
      navigation = page.goto(origin, { waitUntil: "networkidle" });
      navigation.catch(() => {});
      const popup = await popupEvent;
      const response = await headers;
      const body = response.body();
      body.catch(() => {});
      await Promise.all([page.close(), popup.close()]);
      await aborted;
      await assert.rejects(body, /closed|cancel/i);
      await assert.rejects(navigation, /closed|cancel/i);
      assert.equal(page.isClosed(), true);
      assert.equal(popup.isClosed(), true);
      assert.equal(response.status(), 200);
      assert.equal(response.request().frame(), page.mainFrame());
    } finally {
      release();
      await navigation?.catch(() => {});
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
