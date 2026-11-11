import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import test from "node:test";

function idleClock() {
  const runtime = readFileSync(
    new URL("../crates/turbo-surf-render/src/runtime.rs", import.meta.url),
    "utf8",
  );
  const start = runtime.indexOf("// requestIdleCallback must");
  const source =
    start < 0
      ? readFileSync(
          new URL("../crates/turbo-surf-render/src/idle_callback.js", import.meta.url),
          "utf8",
        )
      : runtime.slice(start, runtime.indexOf("// WHATWG URL", start));
  let now = 0;
  let callback;
  const context = vm.createContext({
    __perfNow: () => now,
    setTimeout: (fn) => {
      callback = fn;
      return 7;
    },
    clearTimeout: () => {
      callback = null;
    },
  });
  vm.runInContext(source, context);
  return {
    context,
    setNow: (value) => {
      now = value;
    },
    fire: () => callback?.(),
  };
}

test("expired idle requests expose timeout with no idle budget", () => {
  const clock = idleClock();
  let deadline;
  clock.context.requestIdleCallback(
    (value) => {
      deadline = value;
    },
    { timeout: 1 },
  );
  clock.setNow(30);
  clock.fire();
  assert.equal(deadline.didTimeout, true);
  assert.equal(deadline.timeRemaining(), 0);
});

test("ordinary idle requests retain a decreasing budget and cancellation", () => {
  const clock = idleClock();
  let deadline;
  clock.context.requestIdleCallback((value) => {
    deadline = value;
  });
  clock.setNow(10);
  clock.fire();
  assert.equal(deadline.didTimeout, false);
  assert.equal(deadline.timeRemaining(), 50);
  clock.setNow(70);
  assert.equal(deadline.timeRemaining(), 0);
  let called = false;
  const id = clock.context.requestIdleCallback(() => {
    called = true;
  });
  clock.context.cancelIdleCallback(id);
  clock.fire();
  assert.equal(called, false);
});
