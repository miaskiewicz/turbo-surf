import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { test } from "node:test";
import {
  callWithLifecycle,
  captureOwnedLaunch,
  collectorLifecycle,
} from "./collector-lifecycle.mjs";
import { cdpConnection } from "./cdp-connection.mjs";

const noopSpawn = () => {};
// macOS answers kill(-pgid) with EPERM, not ESRCH, while the group's members exit or wait
// to be reaped.
const eperm = () => {
  throw Object.assign(new Error("kill EPERM"), { code: "EPERM" });
};
// Patchright spawns Chrome detached, so the child the collector owns is the group leader.
const leader = (pid) =>
  Object.assign(new EventEmitter(), { pid, exitCode: null, signalCode: null });

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
  const module = { spawn: noopSpawn };
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
  assert.equal(module.spawn, noopSpawn);
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

test("a zombie-only browser group counts as retired; a live one's EPERM still fails (turbo-surf#50)", async () => {
  const exited = collectorLifecycle({
    signals: new EventEmitter(),
    platform: "darwin",
    graceMs: 1,
    kill: eperm,
  });
  exited.own({ pid: 123, exitCode: 0, signalCode: null });
  await exited.stopBrowser();
  exited.dispose();
  const signalled = collectorLifecycle({
    signals: new EventEmitter(),
    platform: "darwin",
    graceMs: 1,
    kill: eperm,
  });
  signalled.own({ pid: 124, exitCode: null, signalCode: "SIGTERM" });
  await signalled.stopBrowser();
  signalled.dispose();
  const live = collectorLifecycle({
    signals: new EventEmitter(),
    platform: "darwin",
    graceMs: 1,
    kill: eperm,
  });
  live.own({ pid: 125, exitCode: null, signalCode: null });
  await assert.rejects(live.stopBrowser(), /kill EPERM/);
  live.dispose();
});

test("a Patchright leader whose group goes EPERM just before Node reaps it is retired; a live one still fails (turbo-surf#64)", async () => {
  // After context.close() macOS answers kill(-pgid) with EPERM a few ms before Node's 'exit'.
  const calls = [];
  const closing = leader(126);
  const retiring = collectorLifecycle({
    signals: new EventEmitter(),
    platform: "darwin",
    graceMs: 200,
    kill(...args) {
      calls.push(args);
      if (closing.exitCode === null) {
        setTimeout(() => {
          closing.exitCode = 0;
          closing.emit("exit", 0, null);
        }, 5);
      }
      eperm();
    },
  });
  retiring.own(closing);
  const started = Date.now();
  await retiring.stopBrowser();
  retiring.dispose();
  assert.deepEqual(calls, [
    [-126, "SIGTERM"],
    [-126, "SIGKILL"],
  ]);
  assert.equal(closing.listenerCount("exit"), 0);
  // The reap wait ends on 'exit', not on the grace timer.
  assert.ok(Date.now() - started < 200 * 2, `stop took ${Date.now() - started} ms`);
  const live = leader(127);
  const holding = collectorLifecycle({
    signals: new EventEmitter(),
    platform: "darwin",
    graceMs: 20,
    kill: eperm,
  });
  holding.own(live);
  await assert.rejects(holding.stopBrowser(), /kill EPERM/);
  assert.equal(live.listenerCount("exit"), 0);
  holding.dispose();
});
