import assert from "node:assert/strict";
import test from "node:test";
import { waitForTargetNetworkIdle } from "./target-network-idle.mjs";
const ready = {
  pendingRequests: 0,
  pendingNavigation: false,
  revision: "0",
  closed: false,
  error: null,
};
function clock() {
  let time = 0;
  return {
    now: () => time,
    pause: async (ms) => {
      time += ms;
    },
    time: () => time,
  };
}
test("target activity resets the full quiet interval", async () => {
  const c = clock();
  await waitForTargetNetworkIdle(() => ({ ...ready, revision: c.time() < 250 ? "0" : "1" }), {
    ...c,
  });
  assert.equal(c.time(), 750);
});
test("pending navigation and body work cannot appear idle", async () => {
  const c = clock();
  await waitForTargetNetworkIdle(
    () => ({
      ...ready,
      pendingNavigation: c.time() < 100,
      pendingRequests: c.time() < 200 ? 1 : 0,
    }),
    { ...c },
  );
  assert.equal(c.time(), 700);
});
test("unprovable and closed targets reject without an idle result", async () => {
  await assert.rejects(
    waitForTargetNetworkIdle(() => ({ ...ready, error: "unknown target" })),
    /unknown target/,
  );
  await assert.rejects(
    waitForTargetNetworkIdle(() => ({ ...ready, closed: true })),
    /closed/,
  );
});
test("busy target obeys timeout while timeout zero remains unlimited", async () => {
  const c = clock();
  await assert.rejects(
    waitForTargetNetworkIdle(() => ({ ...ready, pendingRequests: 1 }), { ...c, timeout: 100 }),
    { name: "TimeoutError" },
  );
  const d = clock();
  await waitForTargetNetworkIdle(() => ({ ...ready, pendingRequests: d.time() < 150 ? 1 : 0 }), {
    ...d,
    timeout: 0,
  });
  assert.equal(d.time(), 650);
});

test("a late snapshot cannot turn an expired deadline into success", async () => {
  let time = 0;
  await assert.rejects(
    waitForTargetNetworkIdle(() => ready, {
      timeout: 100,
      now: () => time,
      pause: async () => {
        time += 1000;
      },
    }),
    { name: "TimeoutError" },
  );
});

test("missing native closed-state evidence cannot pass idle", async () => {
  const { closed: _closed, ...missing } = ready;
  await assert.rejects(
    waitForTargetNetworkIdle(() => missing),
    /Invalid native target/,
  );
});
