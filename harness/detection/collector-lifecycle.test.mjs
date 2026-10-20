import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { test } from "node:test";
import {
  callWithLifecycle,
  captureOwnedLaunch,
  collectorLifecycle,
} from "./collector-lifecycle.mjs";
import { cdpConnection } from "./cdp-connection.mjs";

test("interrupt rejects blocked work, checkpoints once and retires only owned group", async () => {
  const signals = new EventEmitter(),
    calls = [],
    checkpoints = [];
  const lifecycle = collectorLifecycle({
    signals,
    platform: "darwin",
    graceMs: 1,
    kill: (...args) => calls.push(args),
    onAbort: (signal) => checkpoints.push(signal),
  });
  lifecycle.own({ pid: 123 });
  const blocked = lifecycle.race(new Promise(() => {}));
  signals.emit("SIGTERM");
  signals.emit("SIGINT");
  await assert.rejects(blocked, /SIGTERM/);
  await lifecycle.stopBrowser();
  assert.deepEqual(checkpoints, ["SIGTERM"]);
  assert.deepEqual(calls, [
    [-123, "SIGTERM"],
    [-123, "SIGKILL"],
  ]);
  await assert.rejects(lifecycle.pause(10000), /SIGTERM/);
  lifecycle.dispose();
  assert.equal(signals.listenerCount("SIGTERM"), 0);
});

test("launch ownership matches exact profile and restores spawn on success and failure", async () => {
  for (const fails of [false, true]) {
    const owned = [],
      spawned = [];
    const original = (command, args) => {
      const child = { pid: spawned.length + 1 };
      spawned.push({ command, args });
      return child;
    };
    const module = { spawn: original };
    const launch = captureOwnedLaunch(
      module,
      "/private/owned",
      async () => {
        module.spawn("other", ["--user-data-dir=/private/owned-other"]);
        module.spawn("chrome", ["--user-data-dir=/private/owned"]);
        if (fails) throw new Error("launch failed");
        return { marker: true };
      },
      (child) => owned.push(child),
    );
    if (fails) await assert.rejects(launch, /launch failed/);
    else assert.equal((await launch).marker, true);
    assert.deepEqual(owned, [{ pid: 2 }]);
    assert.equal(module.spawn, original);
  }
});

test("missing process ownership closes context and fails closed", async () => {
  let closed = false;
  const original = () => {};
  const module = { spawn: original };
  await assert.rejects(
    captureOwnedLaunch(
      module,
      "/owned",
      async () => ({
        close: async () => {
          closed = true;
        },
      }),
      () => assert.fail(),
    ),
    /ownership capture unavailable/,
  );
  assert.equal(closed, true);
  assert.equal(module.spawn, original);
});

test("duplicate owned profile is rejected before an untracked child can spawn", async () => {
  let spawned = 0;
  const original = () => ({ pid: ++spawned });
  const module = { spawn: original };
  await assert.rejects(
    captureOwnedLaunch(
      module,
      "/owned",
      async () => {
        module.spawn("chrome", ["--user-data-dir=/owned"]);
        module.spawn("chrome", ["--user-data-dir=/owned"]);
      },
      () => {},
    ),
    /Multiple Chrome/,
  );
  assert.equal(spawned, 1);
  assert.equal(module.spawn, original);
});

test("aborting target setup keeps its finally command catchable without dispatching after abort", async () => {
  const signals = new EventEmitter();
  const lifecycle = collectorLifecycle({ signals, graceMs: 1 });
  const sent = [];
  const socket = { send: (message) => sent.push(JSON.parse(message)) };
  const connection = cdpConnection(socket, () => {});
  let setupError, cleanupError;
  const setup = (async () => {
    try {
      await callWithLifecycle(lifecycle, connection, "Network.enable", {}, "worker");
    } catch (error) {
      setupError = error;
    } finally {
      await callWithLifecycle(
        lifecycle,
        connection,
        "Runtime.runIfWaitingForDebugger",
        {},
        "worker",
      ).catch((error) => {
        cleanupError = error;
      });
    }
  })();
  try {
    assert.deepEqual(
      sent.map(({ method, sessionId }) => ({ method, sessionId })),
      [{ method: "Network.enable", sessionId: "worker" }],
    );
    signals.emit("SIGTERM");
    connection.close();
    await assert.doesNotReject(setup);
    assert.equal(setupError, lifecycle.signal.reason);
    assert.equal(cleanupError, lifecycle.signal.reason);
    assert.equal(sent.length, 1, "post-abort cleanup must not send a protocol command");
  } finally {
    connection.close();
    lifecycle.dispose();
    await lifecycle.stopBrowser();
  }
});
