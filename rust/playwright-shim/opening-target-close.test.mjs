import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
import { BrowserContext, Page } from "./index.mjs";
const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");

for (const mode of ["closed-root", "closed-root-and-popup", "open", "status-error"])
  test(`actor-open attachment boundary: ${mode}`, { timeout: 10000 }, async () => {
    const server = createServer((_request, response) =>
      response.end(
        mode === "closed-root-and-popup"
          ? '<script>window.open("about:blank")</script>'
          : "<title>attached</title>",
      ),
    );
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const context = new BrowserContext();
    const page = await context.newPage();
    const savedStatus = native.liveInitialHydrationStatus;
    let attached, resume;
    const attaching = new Promise((resolve) => (attached = resolve));
    const attachment = new Promise((resolve) => (resume = resolve));
    const complete = page._completeActorOpen.bind(page);
    page._completeActorOpen = async (session) => {
      await complete(session);
      attached();
      await attachment;
    };
    let navigation;
    try {
      const popupEvent = mode === "closed-root-and-popup" ? page.waitForEvent("popup") : null;
      navigation = page.goto(`http://127.0.0.1:${server.address().port}`, {
        waitUntil: "networkidle",
      });
      navigation.catch(() => {});
      await Promise.race([
        attaching,
        navigation.then(() => {
          throw new Error("Navigation completed without the attachment boundary");
        }),
      ]);
      if (mode === "closed-root") await page.close();
      if (mode === "closed-root-and-popup") {
        const popup = await popupEvent;
        await Promise.all([page.close(), popup.close()]);
        assert.equal(popup.isClosed(), true);
      }
      if (mode === "status-error")
        native.liveInitialHydrationStatus = () => {
          throw new Error("fixture status failed");
        };
      resume();
      if (mode === "open") {
        await navigation;
        assert.equal(page.isClosed(), false);
        assert.equal(await page.title(), "attached");
      } else {
        const expected = mode === "status-error" ? /fixture status failed/ : /closed|cancel/i;
        await assert.rejects(navigation, expected);
        if (mode !== "status-error") assert.equal(page.isClosed(), true);
      }
    } finally {
      resume();
      await navigation?.catch(() => {});
      native.liveInitialHydrationStatus = savedStatus;
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  });

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

function deferred() {
  let resolve, reject;
  const promise = new Promise((pass, fail) => {
    resolve = pass;
    reject = fail;
  });
  return { promise, resolve, reject };
}

// Delivery gates retain the native result itself; no actor identity or state is synthesized.
function holdActualOpeningSnapshot(page) {
  const originalState = native.liveAuxiliaryState;
  const originalAuxWait = native.liveAuxiliaryWait;
  const originalNetworkWait = native.liveNetworkLogWait;
  const snapshot = deferred();
  const releaseSnapshot = deferred();
  const releaseWaits = deferred();
  let freezeWaits = false;
  let captureNext = false;
  const observed = { heldWaits: 0, snapshot: null };
  const wait =
    (original) =>
    async (...args) => {
      const actual = await original(...args);
      if (freezeWaits && args[0] === page._actorHub?.actor) {
        observed.heldWaits++;
        await releaseWaits.promise;
      }
      return actual;
    };
  native.liveAuxiliaryWait = wait(originalAuxWait);
  native.liveNetworkLogWait = wait(originalNetworkWait);
  native.liveAuxiliaryState = (session) => {
    const actual = originalState(session);
    if (!captureNext || session !== page._actorHub?.actor) return actual;
    captureNext = false;
    const metadata = JSON.parse(actual);
    if (metadata.opening !== true || metadata.closed) {
      snapshot.reject(new Error("Fixture did not capture an actual live opening actor"));
      return actual;
    }
    observed.snapshot = metadata;
    snapshot.resolve(metadata);
    return releaseSnapshot.promise.then(() => actual);
  };
  return {
    observed,
    snapshot: snapshot.promise,
    freeze() {
      freezeWaits = true;
    },
    capture() {
      captureNext = true;
    },
    releaseSnapshot: releaseSnapshot.resolve,
    restore() {
      freezeWaits = false;
      releaseWaits.resolve();
      releaseSnapshot.resolve();
      native.liveAuxiliaryState = originalState;
      native.liveAuxiliaryWait = originalAuxWait;
      native.liveNetworkLogWait = originalNetworkWait;
    },
  };
}

test(
  "completed native opening publishes readiness after an older held actor snapshot",
  { timeout: 10000 },
  async (t) => {
    const scriptBody = deferred();
    const server = createServer(async (request, response) => {
      response.setHeader(
        "content-type",
        request.url === "/held.js" ? "text/javascript" : "text/html",
      );
      if (request.url === "/held.js") {
        response.writeHead(200);
        response.flushHeaders();
        await scriptBody.promise;
        response.end("globalThis.attachmentScriptCompleted=true;");
      } else response.end('<title>actual readiness</title><script src="/held.js"></script>');
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const context = new BrowserContext();
    const page = await context.newPage();
    const entered = deferred();
    const attached = deferred();
    const releaseAttachment = deferred();
    const complete = page._completeActorOpen.bind(page);
    page._completeActorOpen = async (session) => {
      entered.resolve(session);
      await complete(session);
      attached.resolve(session);
      await releaseAttachment.promise;
    };
    const controlled = holdActualOpeningSnapshot(page);
    const origin = `http://127.0.0.1:${server.address().port}`;
    let navigation, older;
    try {
      const headers = page.waitForResponse((response) => response.url() === `${origin}/held.js`);
      navigation = page.goto(origin, { waitUntil: "networkidle" });
      navigation.catch(() => {});
      assert.equal((await headers).status(), 200);
      const actor = page._actorHub;
      assert.equal(actor.opening, true);
      let ready = false;
      actor.ready.then(
        () => {
          ready = true;
        },
        () => {},
      );
      controlled.freeze();
      await actor.drain();
      assert.equal(
        actor.pending,
        null,
        "fixture must start its held snapshot on an idle real actor",
      );
      controlled.capture();
      older = actor.drain();
      older.catch(() => {});
      const captured = await controlled.snapshot;
      assert.equal(captured.actor, actor.actor);
      assert.equal(captured.opening, true);
      assert.equal(ready, false);
      scriptBody.resolve();
      const completedSession = await entered.promise;
      assert.equal(completedSession, page._session);
      controlled.releaseSnapshot();
      await attached.promise;
      t.diagnostic(
        JSON.stringify({
          oldOpening: captured.opening,
          oldRevision: captured.revision,
          heldNativeWaits: controlled.observed.heldWaits,
          readyAfterActualAttachment: ready,
        }),
      );
      assert.equal(
        ready,
        true,
        "completed native attachment must consume a fresh opening:false receipt",
      );
      await page.close();
      assert.equal(page.isClosed(), true);
      releaseAttachment.resolve();
      await assert.rejects(navigation, /closed|cancel/i);
      await older;
    } finally {
      scriptBody.resolve();
      controlled.restore();
      releaseAttachment.resolve();
      await older?.catch(() => {});
      await page._actorHub?.drain(true).catch(() => {});
      await navigation?.catch(() => {});
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

test(
  "closing a root after actual opening refresh cancels its pending navigation",
  { timeout: 10000 },
  async () => {
    const server = createServer((_request, response) =>
      response.end('<script>window.open("about:blank")</script>'),
    );
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const context = new BrowserContext();
    const page = await context.newPage();
    let reached, resume;
    const refreshed = new Promise((resolve) => (reached = resolve));
    const paused = new Promise((resolve) => (resume = resolve));
    const actualRefresh = page._refreshFromSession.bind(page);
    let first = true;
    page._refreshFromSession = async (...args) => {
      const value = await actualRefresh(...args);
      if (first) {
        first = false;
        assert.equal(args[0], true, "the actual initial opening refresh must reach the boundary");
        reached();
        await paused;
      }
      return value;
    };
    let navigation;
    let outcome = "pending";
    try {
      const popupEvent = page.waitForEvent("popup");
      navigation = page.goto(`http://127.0.0.1:${server.address().port}`, {
        waitUntil: "networkidle",
      });
      navigation.then(
        () => {
          outcome = "fulfilled";
        },
        () => {
          outcome = "rejected";
        },
      );
      const [popup] = await Promise.all([popupEvent, refreshed]);
      assert.equal(outcome, "pending");
      await page.close();
      assert.equal(page.isClosed(), true);
      assert.equal(popup.isClosed(), false);
      resume();
      await assert.rejects(navigation, /closed|cancel/i);
      assert.equal(await popup.evaluate(() => 42), 42);
    } finally {
      resume();
      await navigation?.catch(() => {});
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

test(
  "initial networkidle waits for a held fetch body after the page is usable",
  { timeout: 10000 },
  async () => {
    let release;
    const held = new Promise((resolve) => (release = resolve));
    let bodies = 0;
    const server = createServer(async (request, response) => {
      if (request.url === "/held") {
        bodies++;
        response.writeHead(200, { "content-type": "application/json" });
        response.flushHeaders();
        await held;
        response.end('{"ok":true}');
      } else {
        response.end('<script>fetch("/held").catch(()=>{});</script>');
      }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const context = new BrowserContext();
    const page = await context.newPage();
    let navigation;
    let outcome = "pending";
    let resolveOpening, rejectOpening, initialOpening;
    const openingFinished = new Promise((resolve, reject) => {
      resolveOpening = resolve;
      rejectOpening = reject;
    });
    openingFinished.catch(() => {});
    const actualOpen = page._openLiveSession.bind(page);
    page._openLiveSession = (...args) => {
      const opening = actualOpen(...args);
      if (!initialOpening) {
        initialOpening = opening;
        opening.then(resolveOpening, rejectOpening);
      }
      return opening;
    };
    try {
      const headers = page.waitForResponse((response) => response.url() === `${origin}/held`);
      navigation = page.goto(origin, { waitUntil: "networkidle" });
      navigation.then(
        () => {
          outcome = "fulfilled";
        },
        () => {
          outcome = "rejected";
        },
      );
      const response = await headers;
      await openingFinished;
      assert.equal(await page.evaluate(() => 42), 42);
      assert.equal(bodies, 1);
      assert.equal(
        outcome,
        "pending",
        "an active response body must prevent initial networkidle even after actual page evaluation works",
      );
      release();
      await navigation;
      assert.equal(outcome, "fulfilled");
      assert.deepEqual(await response.json(), { ok: true });
    } finally {
      release();
      await navigation?.catch(() => {});
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
