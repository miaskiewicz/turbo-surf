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
const { Page } = await import("./index.mjs");
const normalizeTargetError = Reflect.get(Page.prototype, "_targetNavigationError").bind(null);

test("target navigation timeout normalization preserves unrelated rejection values", () => {
  const authored = new RangeError("Target navigation timeout after 30ms");
  for (const value of [undefined, null, "host rejection", authored])
    assert.equal(normalizeTargetError(value), value);
  assert.equal(authored.name, "RangeError");
  const timeout = Object.assign(new Error("Target navigation timeout after 30ms"), {
    code: "GenericFailure",
  });
  assert.equal(normalizeTargetError(timeout), timeout);
  assert.equal(timeout.name, "TimeoutError");
});

test("owned navigation milestone timeouts preserve the timer cause and unrelated rejections", () => {
  const normalize = Reflect.get(Page.prototype, "_navigationMilestoneError").bind(null);
  const timer = Object.assign(new Error("Timeout 30ms exceeded while waiting for event load"), {
    name: "TimeoutError",
  });
  const navigation = normalize(timer, "load", 30);
  assert.equal(navigation.name, "TimeoutError");
  assert.equal(navigation.message, "Target navigation timeout after 30ms");
  assert.equal(navigation.cause, timer);
  for (const value of [undefined, null, "rejected", new RangeError(timer.message)])
    assert.equal(normalize(value, "load", 30), value);
  assert.equal(normalize(timer, "domcontentloaded", 30), timer);
  assert.equal(normalize(timer, "load", 31), timer);
});

test("navigation binding deadlines require the exact owned hub error", async () => {
  const { LiveActorHub, ownsBindingDeadline } = await import("./actor-hub.mjs");
  assert.equal(ownsBindingDeadline(new Error("unknown"), undefined, undefined), false);
  const owner = {};
  const deadline = Date.now() - 1;
  let timer;
  try {
    LiveActorHub.prototype._bindingDeadline.call({}, deadline, false, owner);
  } catch (error) {
    timer = error;
  }
  assert.equal(timer?.name, "TimeoutError");
  const normalize = Reflect.get(Page.prototype, "_navigationBindingError").bind(owner);
  const navigation = normalize(timer, deadline, 30);
  assert.equal(navigation.message, "Target navigation timeout after 30ms");
  assert.equal(navigation.name, "TimeoutError");
  assert.equal(navigation.cause, timer);
  assert.equal(normalize(timer, deadline + 1, 30), timer);
  const foreign = Reflect.get(Page.prototype, "_navigationBindingError").bind({});
  assert.equal(foreign(timer, deadline, 30), timer);
  const lookalike = Object.assign(new Error(timer.message), { name: "TimeoutError" });
  for (const error of [lookalike, undefined, null, "rejected"])
    assert.equal(normalize(error, deadline, 30), error);
});

async function fixture(run) {
  const timers = new Set();
  const server = createServer((request, response) => {
    if (request.url === "/slow.js") {
      const timer = setTimeout(() => {
        timers.delete(timer);
        response.setHeader("content-type", "application/javascript");
        response.end("document.body.dataset.late='old'");
      }, 180);
      timers.add(timer);
    } else response.end("<body>ready</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const open = (metadata, html = '<body><script src="/slow.js"></script></body>', timeout = 30) =>
    native.liveOpen(
      html,
      base,
      "[]",
      "",
      undefined,
      JSON.stringify(metadata),
      "",
      timeout,
      undefined,
      "",
      true,
    );
  try {
    await run({ base, open });
  } finally {
    for (const timer of timers) clearTimeout(timer);
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("initial timeout keeps legacy native open rejection by default", () =>
  fixture(async ({ open }) => {
    await assert.rejects(open({}), /navigation timeout after 30ms/);
  }));

test("retained initial timeout keeps the actual native target usable", () =>
  fixture(async ({ base, open }) => {
    const actor = await open({ retainInitialHydrationTimeout: true });
    try {
      assert.deepEqual(JSON.parse(native.liveInitialHydrationStatus(actor)), {
        timedOut: true,
        timeoutMs: 30,
      });
      const initial = JSON.parse(native.liveAuxiliaryState(actor));
      assert.equal(typeof initial.root, "string");
      const receipt = JSON.parse(
        await native.liveNavigateTarget(
          actor,
          `${base}/ready`,
          "replace",
          JSON.stringify({ timeoutMs: 1000, waitUntil: "load" }),
        ),
      );
      assert.equal(receipt.status, 200);
      assert.equal(JSON.parse(native.liveAuxiliaryState(actor)).root, initial.root);
      assert.equal(
        JSON.parse(native.liveReadEngine(actor, "return document.body.textContent")),
        "ready",
      );
    } finally {
      native.liveActorClose(actor);
    }
    assert.throws(() => native.liveInitialHydrationStatus(actor), /closed|no such live actor/);
  }));

test("initial timeout status rejects pending and unknown actors", () =>
  fixture(async ({ open }) => {
    const reserved = native.liveNetworkLogCreate();
    const opening = open({ hostNetworkLog: reserved, retainInitialHydrationTimeout: true });
    assert.throws(() => native.liveInitialHydrationStatus(reserved), /still pending/);
    const actor = await opening;
    try {
      assert.equal(JSON.parse(native.liveInitialHydrationStatus(actor)).timedOut, true);
      assert.throws(() => native.liveInitialHydrationStatus(0), /no such live actor/);
    } finally {
      native.liveActorClose(actor);
    }
  }));

test("initial timeout status cannot be borrowed through children or a retired root", () =>
  fixture(async ({ open }) => {
    const actor = await open({ retainInitialHydrationTimeout: true }, "<body>root</body>", 1000);
    try {
      assert.deepEqual(JSON.parse(native.liveInitialHydrationStatus(actor)), {
        timedOut: false,
        timeoutMs: null,
      });
      await native.liveEvalEngineExpression(
        actor,
        "(() => { window.open('about:blank', 'status-child'); return true; })()",
      );
      const state = JSON.parse(native.liveAuxiliaryState(actor));
      const childTarget = state.pages.find((page) => page.target !== state.root).target;
      const child = native.liveAuxiliaryAdopt(actor, childTarget);
      assert.throws(() => native.liveInitialHydrationStatus(child), /requires its actor token/);
      native.liveClose(actor);
      assert.equal(JSON.parse(native.liveAuxiliaryState(child)).closed, false);
      assert.throws(() => native.liveInitialHydrationStatus(actor), /root is clos/);
      assert.equal(JSON.parse(native.liveReadEngine(child, "return document.URL")), "about:blank");
    } finally {
      native.liveActorClose(actor);
    }
  }));
