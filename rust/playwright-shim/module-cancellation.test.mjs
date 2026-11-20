import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");

const rootPage = `<!doctype html><body><script type="module">
        globalThis.finishedFirstTrace = [];
        const stage = name => fetch("/finished-stage?" + encodeURIComponent(name));
        await stage("prefinished");
        globalThis.precompiledHealthy = await import("/healthy-precompiled.mjs");
        globalThis.precompiledIdentity = precompiledHealthy.token;
        globalThis.precompiledFunction = precompiledHealthy.readMarker;
        if(precompiledHealthy.marker!==37 || precompiledIdentity.owner!==globalThis || precompiledIdentity.document!==document)
          throw new Error("Healthy precompiled root module premise failed");
        await stage("precompiled-ready");
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
          precompiledReady: precompiledFunction()===37 && precompiledIdentity.document===document && precompiledRuns===1,
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
      </script></body>`;
const readyPage =
  '<body data-ready="yes"><a id="child" target="_blank" href="/child">child</a></body>';
const routes = new Map([
  ["/cancel-finished-first", ["text/html", rootPage]],
  [
    "/healthy-precompiled.mjs",
    [
      "application/javascript",
      "globalThis.precompiledRuns=(globalThis.precompiledRuns||0)+1;export const marker=37;export const token={owner:globalThis,document};export function readMarker(){return marker;}",
    ],
  ],
  [
    "/healthy-fresh.mjs",
    [
      "application/javascript",
      "globalThis.freshRuns=(globalThis.freshRuns||0)+1;export const marker=41;export const owner=globalThis;export const ownerDocument=document;",
    ],
  ],
  [
    "/peer-held.mjs",
    [
      "application/javascript",
      "globalThis.peerModuleRuns=(globalThis.peerModuleRuns||0)+1;globalThis.peerModuleStarted=true;const ownDocument=document;await fetch('/peer-gate');globalThis.peerModuleFinished=true;export const marker=53;export {ownDocument};",
    ],
  ],
  ["/after", ["text/html", '<body><script src="/slow.js"></script></body>']],
]);

function send(response, type, body) {
  response.setHeader("content-type", type);
  response.end(body);
}

async function fixture(run) {
  const timers = new Set();
  const requests = [];
  let peerGateResponse;
  let markPeerGateEntered;
  const peerGateEntered = new Promise((resolve) => {
    markPeerGateEntered = resolve;
  });
  const releasePeerGate = () => {
    if (peerGateResponse && !peerGateResponse.writableEnded) peerGateResponse.end("released");
  };
  const server = createServer((request, response) => {
    requests.push(request.url);
    if (request.url === "/peer-gate") {
      peerGateResponse = response;
      markPeerGateEntered();
      return;
    }
    if (request.url.startsWith("/finished-stage?")) {
      response.writeHead(204);
      response.end();
      return;
    }
    if (request.url === "/slow.js") {
      const timer = setTimeout(() => {
        timers.delete(timer);
        send(response, "application/javascript", "document.body.dataset.loaded='yes'");
      }, 180);
      timers.add(timer);
      return;
    }
    send(response, ...(routes.get(request.url) || ["text/html", readyPage]));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const context = new shim.BrowserContext();
  try {
    await run({ context, origin, requests, peerGateEntered, releasePeerGate });
  } finally {
    releasePeerGate();
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
  "Page.evaluate still awaits its authored pending peer module during observational metadata collection",
  { timeout: 10000 },
  () =>
    fixture(async ({ context, origin, peerGateEntered, releasePeerGate, requests }) => {
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { waitUntil: "networkidle" });
      const created = page.waitForEvent("popup", { timeout: 2000 });
      await page.locator("#child").click();
      const peer = await created;
      await peer.waitForURL(`${origin}/child`, { timeout: 2000 });
      let settled = false;
      const evaluation = peer
        .evaluate(async () => {
          const module = await import(location.origin + "/peer-held.mjs");
          return {
            marker: module.marker,
            sameDocument: module.ownDocument === document,
            finished: peerModuleFinished,
            runs: peerModuleRuns,
          };
        })
        .then(
          (value) => {
            settled = true;
            return { value };
          },
          (error) => {
            settled = true;
            return { error };
          },
        );
      await peerGateEntered;
      await new Promise((resolve) => setTimeout(resolve, 40));
      assert.equal(settled, false, "the authored await must not become a snapshot-only read");
      assert.equal(requests.filter((url) => url === "/peer-gate").length, 1);
      releasePeerGate();
      assert.deepEqual(await evaluation, {
        value: { marker: 53, sameDocument: true, finished: true, runs: 1 },
      });
    }),
);

test(
  "pending module cancellation preserves the root module map and another actual Page graph",
  { timeout: 20000 },
  () =>
    fixture(async ({ context, origin, requests, peerGateEntered, releasePeerGate }) => {
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
          top.location.pathname === "/cancel-finished-first"
        ) {
          parent.finishedFirstTrace.push("pending-init");
          while (true) {}
        }
      });
      const page = await context.newPage();
      await page.goto(`${origin}/ready`, { timeout: 0, waitUntil: "networkidle" });
      const target = actualTarget(page);
      const created = page.waitForEvent("popup", { timeout: 2000 });
      await page.locator("#child").click();
      const peer = await created;
      await peer.waitForURL(`${origin}/child`, { timeout: 2000 });
      const peerTarget = actualTarget(peer);
      const actorState = JSON.parse(native.liveAuxiliaryState(Reflect.get(page, "_session")));
      assert.equal(actorState.root, target);
      assert.ok(
        actorState.pages.some((entry) => entry.target === peerTarget && !entry.closed),
        "peer is owned by the same actual native actor",
      );
      await peer.evaluate(() => {
        globalThis.peerSettled = null;
        globalThis.peerImport = import(location.origin + "/peer-held.mjs");
        peerImport.then(
          (module) => {
            globalThis.peerSettled = { marker: module.marker };
          },
          (error) => {
            globalThis.peerSettled = { error: error.name, message: error.message };
          },
        );
        return "queued";
      });
      await peerGateEntered;
      assert.equal(requests.filter((url) => url === "/peer-gate").length, 1);
      await assert.rejects(
        page.goto(`${origin}/cancel-finished-first`, { timeout: 2000, waitUntil: "networkidle" }),
        (error) =>
          error.name === "TimeoutError" &&
          [
            "Target navigation timeout after 2000ms",
            "Timeout 2000ms exceeded while waiting for event load",
          ].includes(error.message),
      );
      assert.equal(page.isClosed(), false);
      assert.equal(actualTarget(page), target);
      assert.ok(requests.includes("/finished-stage?precompiled-ready"));
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
          compiledMarker: precompiledFunction(),
          compiledIdentity: precompiledHealthy.token === precompiledIdentity,
          compiledOwner:
            precompiledIdentity.owner === globalThis && precompiledIdentity.document === document,
          compiledRuns: precompiledRuns,
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
            precompiledReady: true,
          },
          closed: false,
          same: true,
          sameDocument: true,
          count: 18,
          authored: true,
          ready: "complete",
          body: "yes",
          compiledMarker: 37,
          compiledIdentity: true,
          compiledOwner: true,
          compiledRuns: 1,
        },
      );
      assert.ok(
        Date.now() - readStarted < 3000,
        "cancelled root module evaluation blocked the retained document read",
      );
      assert.deepEqual(
        await page.evaluate(async () => {
          const cached = await import(location.origin + "/healthy-precompiled.mjs");
          const fresh = await import(location.origin + "/healthy-fresh.mjs");
          return {
            sameNamespace: cached === precompiledHealthy,
            sameToken: cached.token === precompiledIdentity,
            sameFunction: cached.readMarker === precompiledFunction,
            cachedMarker: cached.readMarker(),
            cachedRuns: precompiledRuns,
            freshMarker: fresh.marker,
            freshRuns,
            freshOwner: fresh.owner === globalThis && fresh.ownerDocument === document,
          };
        }),
        {
          sameNamespace: true,
          sameToken: true,
          sameFunction: true,
          cachedMarker: 37,
          cachedRuns: 1,
          freshMarker: 41,
          freshRuns: 1,
          freshOwner: true,
        },
      );
      assert.equal(requests.filter((url) => url === "/healthy-precompiled.mjs").length, 1);
      assert.deepEqual(
        await peer.evaluate(() => ({
          started: peerModuleStarted,
          finished: typeof peerModuleFinished,
          settled: peerSettled,
          runs: peerModuleRuns,
        })),
        { started: true, finished: "undefined", settled: null, runs: 1 },
      );
      releasePeerGate();
      assert.deepEqual(
        await peer.evaluate(() =>
          peerImport.then((module) => ({
            marker: module.marker,
            sameDocument: module.ownDocument === document,
            finished: peerModuleFinished,
            runs: peerModuleRuns,
          })),
        ),
        { marker: 53, sameDocument: true, finished: true, runs: 1 },
      );
      assert.equal(actualTarget(peer), peerTarget);
      assert.equal(peer.isClosed(), false);
      assert.equal(requests.filter((url) => url === "/peer-gate").length, 1);
      await recover(page, origin, target);
    }),
);
