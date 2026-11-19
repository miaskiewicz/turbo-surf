import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");

async function fixture(run) {
  const timers = new Set();
  const requests = [];
  const schedule = (callback, delay) => {
    const timer = setTimeout(() => {
      timers.delete(timer);
      callback();
    }, delay);
    timers.add(timer);
  };
  const server = createServer((request, response) => {
    requests.push(request.url);
    if (request.url === "/slow.js") {
      schedule(() => {
        response.setHeader("content-type", "application/javascript");
        response.end("document.body.dataset.loaded='yes'");
      }, 180);
    } else if (request.url === "/before") {
      schedule(() => response.end('<body data-ready="yes"></body>'), 180);
    } else if (request.url === "/iframe") {
      response.end(
        '<body><iframe id="deadline" srcdoc="<body data-child=yes><script>document.body.dataset.script=&quot;yes&quot;</script></body>"></iframe></body>',
      );
    } else if (request.url === "/mixed") {
      response.end(
        '<body><iframe id="finished" srcdoc="<body>finished</body>"></iframe><iframe id="deadline" srcdoc="<body>pending</body>"></iframe></body>',
      );
    } else if (request.url.startsWith("/finished-stage?")) {
      response.writeHead(204);
      response.end();
    } else if (request.url === "/finished-first") {
      response.end(`<!doctype html><body><script type="module">
        globalThis.finishedFirstTrace = [];
        const stage = name => fetch("/finished-stage?" + encodeURIComponent(name));
        await stage("prefinished");
        const load = frame => new Promise(resolve => frame.addEventListener("load", resolve, {once:true}));
        const finished = document.createElement("iframe");
        finished.id = "finished";
        finished.srcdoc = '<body data-finished="yes"><script>globalThis.finishedAuthored=true;<' + '/script></body>';
        const firstLoad = load(finished);
        document.body.appendChild(finished);
        await firstLoad;
        const owner = finished.contentWindow;
        if (owner.finishedCount !== 17 || owner.finishedAuthored !== true || owner.document.readyState !== "complete")
          throw new Error("Finished child did not complete before pending admission");
        globalThis.finishedBeforePending = {
          count: owner.finishedCount,
          complete: owner.document.readyState,
          authored: owner.finishedAuthored,
          sameDocument: owner.finishedDocument === owner.document,
          sameWindow: owner.finishedDocument.defaultView === owner,
          sameFunction: owner.finishedFunction === owner.Function,
        };
        finishedFirstTrace.push("finished-load");
        await stage("finished-load");
        const pending = document.createElement("iframe");
        pending.id = "deadline";
        pending.srcdoc = '<body>pending</body>';
        const pendingLoad = load(pending);
        finishedFirstTrace.push("pending-admit");
        await stage("pending-admit");
        document.body.appendChild(pending);
        await pendingLoad;
        finishedFirstTrace.push("pending-load");
        await stage("pending-load");
      </script></body>`);
    } else if (request.url === "/iframe-script") {
      response.end('<body><iframe srcdoc="<script>while(true){}</script>"></iframe></body>');
    } else if (request.url === "/after") {
      response.end('<body><script src="/slow.js"></script></body>');
    } else {
      response.end(
        '<body data-ready="yes"><a id="child" target="_blank" href="/child">child</a></body>',
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const context = new shim.BrowserContext();
  try {
    await run({ context, origin, requests });
  } finally {
    await context.close();
    for (const timer of timers) clearTimeout(timer);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

function actualTarget(page) {
  const target = Reflect.get(page, "_targetId");
  assert.equal(typeof target, "string");
  return target;
}

async function recover(page, origin, target) {
  const recoveryStarted = Date.now();
  assert.equal(page.isClosed(), false);
  assert.equal(actualTarget(page), target);
  await page.goto(`${origin}/ready`, { waitUntil: "networkidle" });
  assert.equal(await page.evaluate(() => document.body.dataset.ready), "yes");
  assert.equal(actualTarget(page), target);
  await page.goto(`${origin}/after`, { timeout: 0, waitUntil: "networkidle" });
  assert.equal(await page.evaluate(() => document.body.dataset.loaded), "yes");
  assert.equal(actualTarget(page), target);
  assert.ok(
    Date.now() - recoveryStarted < 3000,
    "cancelled initialization must not consume another session execution cap during recovery",
  );
}

test(
  "navigation deadline before response headers preserves the actual document",
  { timeout: 10000 },
  () =>
    fixture(async ({ context, origin }) => {
      const page = await context.newPage();
      await page.goto(`${origin}/before`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      await page.evaluate(() => {
        globalThis.keptDocument = document;
      });
      await assert.rejects(
        page.reload({ timeout: 30, waitUntil: "networkidle" }),
        /timeout after 30ms/,
      );
      assert.equal(page.isClosed(), false);
      assert.equal(await page.evaluate(() => document === globalThis.keptDocument), true);
      await recover(page, origin, target);
    }),
);

test(
  "navigation deadline after response commit retains a complete owned backing",
  { timeout: 10000 },
  () =>
    fixture(async ({ context, origin }) => {
      const page = await context.newPage();
      await page.goto(`${origin}/after`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      await assert.rejects(
        page.reload({ timeout: 30, waitUntil: "networkidle" }),
        /timeout after 30ms/,
      );
      await recover(page, origin, target);
    }),
);

test("a navigation deadline does not close or replace its sibling popup", { timeout: 10000 }, () =>
  fixture(async ({ context, origin }) => {
    const page = await context.newPage();
    await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
    const created = page.waitForEvent("popup", { timeout: 2000 });
    await page.locator("#child").click();
    const child = await created;
    await child.waitForURL(`${origin}/child`, { timeout: 2000 });
    await child.evaluate(() => {
      globalThis.childCounter = 17;
    });
    const rootTarget = actualTarget(page);
    const childTarget = actualTarget(child);
    await assert.rejects(
      page.goto(`${origin}/after`, { timeout: 30, waitUntil: "networkidle" }),
      /timeout after 30ms/,
    );
    assert.equal(child.isClosed(), false);
    assert.equal(actualTarget(child), childTarget);
    assert.equal(await child.evaluate(() => ++globalThis.childCounter), 18);
    await recover(page, origin, rootTarget);
    assert.equal(await child.evaluate(() => globalThis.childCounter), 18);
  }),
);

// This new harness cap also observes a broken implementation reaching the unchanged
// 10s session guard; the product deadline and elapsed assertion remain 30ms/1s.
test(
  "parsed iframe context initialization keeps the original driver execution deadline",
  { timeout: 15000 },
  () =>
    fixture(async ({ context, origin }) => {
      await context.addInitScript(() => {
        if (window.frameElement?.id === "deadline" && window.top.location.pathname === "/iframe")
          while (true) {}
      });
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      const started = Date.now();
      await assert.rejects(
        page.goto(`${origin}/iframe`, { timeout: 30, waitUntil: "networkidle" }),
        /timeout after 30ms/,
      );
      assert.ok(
        Date.now() - started < 1000,
        "frame initialization must not inherit the 10s session cap",
      );
      await recover(page, origin, target);
    }),
);

test(
  "parsed iframe authored scripts remain interruptible after trusted setup",
  { timeout: 15000 },
  () =>
    fixture(async ({ context, origin }) => {
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      const started = Date.now();
      await assert.rejects(
        page.goto(`${origin}/iframe-script`, { timeout: 30, waitUntil: "networkidle" }),
        /timeout after 30ms/,
      );
      assert.ok(
        Date.now() - started < 1000,
        "authored frame execution must retain its driver deadline",
      );
      await recover(page, origin, target);
    }),
);

test(
  "successful parsed iframe initialization executes once in its actual owner",
  { timeout: 10000 },
  () =>
    fixture(async ({ context, origin }) => {
      await context.addInitScript(() => {
        globalThis.initializationCount = (globalThis.initializationCount || 0) + 1;
        globalThis.initializationFunction = Function;
        globalThis.initializationOwner = () => globalThis;
        globalThis.initializationDocument = document;
        if (window.frameElement?.id === "deadline" && window.top.location.pathname === "/iframe")
          globalThis.deadlineFixtureEntered = true;
      });
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      await page.goto(`${origin}/iframe`, { timeout: 0, waitUntil: "networkidle" });
      assert.equal(actualTarget(page), target);
      assert.deepEqual(
        await page.evaluate(() => [globalThis.initializationCount, frames[0].initializationCount]),
        [1, 1],
      );
      assert.equal(await page.evaluate(() => frames[0].document.body.dataset.child), "yes");
      assert.equal(await page.evaluate(() => frames[0].document.body.dataset.script), "yes");
      assert.equal(await page.evaluate(() => frames[0].deadlineFixtureEntered), true);
      assert.deepEqual(
        await page.evaluate(() => [
          globalThis.initializationDocument === document,
          globalThis.initializationDocument.defaultView === globalThis,
          globalThis.initializationFunction === Function,
          globalThis.initializationOwner() === globalThis,
          globalThis.initializationFunction("return globalThis")() === globalThis,
          frames[0].initializationFunction === frames[0].Function,
          frames[0].initializationOwner() === frames[0],
        ]),
        [true, true, true, true, true, true, true],
      );
    }),
);

test(
  "navigation root initialization belongs to the actual native target document",
  { timeout: 10000 },
  () =>
    fixture(async ({ context, origin }) => {
      await context.addInitScript(() => {
        globalThis.capturedInitializationDocument = document;
        globalThis.capturedInitializationFunction = Function;
        globalThis.capturedInitializationOwner = () => globalThis;
        globalThis.guestNavigationMarker = Symbol("guest-navigation");
      });
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      const replacement = `${origin}/ready?replacement=1`;
      await page.goto(replacement, { timeout: 0, waitUntil: "networkidle" });
      assert.equal(actualTarget(page), target);
      const state = JSON.parse(native.liveAuxiliaryState(Reflect.get(page, "_session")));
      assert.equal(state.root, target);
      const info = state.pages.find((entry) => entry.target === target);
      assert.deepEqual(
        { target: info.target, url: info.url, closed: info.closed },
        {
          target,
          url: replacement,
          closed: false,
        },
      );
      assert.deepEqual(
        await page.evaluate(() => ({
          sameDocument: globalThis.capturedInitializationDocument === document,
          correctWindow: globalThis.capturedInitializationDocument.defaultView === globalThis,
          sameFunction: globalThis.capturedInitializationFunction === Function,
          correctOwner: globalThis.capturedInitializationOwner() === globalThis,
          constructorOwner:
            globalThis.capturedInitializationFunction("return globalThis")() === globalThis,
          documentURL: globalThis.capturedInitializationDocument.URL,
          markerType: typeof globalThis.guestNavigationMarker,
        })),
        {
          sameDocument: true,
          correctWindow: true,
          sameFunction: true,
          correctOwner: true,
          constructorOwner: true,
          documentURL: replacement,
          markerType: "symbol",
        },
      );
    }),
);

test(
  "expiry preserves completed child initialization while retiring only pending work",
  { timeout: 20000 },
  () =>
    fixture(async ({ context, origin, requests }) => {
      await context.addInitScript(() => {
        if (window.frameElement?.id === "finished" && document.URL === "about:srcdoc") {
          globalThis.finishedCount = 17;
          globalThis.finishedDocument = document;
          globalThis.finishedFunction = Function;
          parent.finishedChild = window;
          parent.finishedFirstTrace.push("finished-init");
        }
        if (
          window.frameElement?.id === "deadline" &&
          document.URL === "about:srcdoc" &&
          top.location.pathname === "/finished-first"
        ) {
          parent.finishedFirstTrace.push("pending-init");
          while (true) {}
        }
      });
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      await assert.rejects(
        page.goto(`${origin}/finished-first`, { timeout: 2000, waitUntil: "networkidle" }),
        (error) =>
          error.name === "TimeoutError" &&
          [
            "Target navigation timeout after 2000ms",
            "Timeout 2000ms exceeded while waiting for event load",
          ].includes(error.message),
      );
      assert.equal(page.isClosed(), false);
      assert.equal(actualTarget(page), target);
      assert.deepEqual(
        requests
          .filter((url) => url.startsWith("/finished-stage?"))
          .map((url) => decodeURIComponent(url.split("?")[1])),
        ["prefinished", "finished-load", "pending-admit"],
      );
      const readStarted = Date.now();
      assert.deepEqual(
        await page.evaluate(() => ({
          trace: finishedFirstTrace,
          before: finishedBeforePending,
          closed: finishedChild.closed,
          same: finishedChild === document.getElementById("finished").contentWindow,
          sameDocument: finishedChild.finishedDocument === finishedChild.document,
          count: ++finishedChild.finishedCount,
          authored: finishedChild.finishedAuthored,
          ready: finishedChild.document.readyState,
          body: finishedChild.document.body.dataset.finished,
        })),
        {
          trace: ["finished-init", "finished-load", "pending-admit", "pending-init"],
          before: {
            count: 17,
            complete: "complete",
            authored: true,
            sameDocument: true,
            sameWindow: true,
            sameFunction: true,
          },
          closed: false,
          same: true,
          sameDocument: true,
          count: 18,
          authored: true,
          ready: "complete",
          body: "yes",
        },
      );
      assert.ok(
        Date.now() - readStarted < 3000,
        "late owned iframe initialization blocked the post-timeout read",
      );
      await recover(page, origin, target);
    }),
);
