import assert from "node:assert/strict";
import test from "node:test";
import { makeKeyboard } from "./keyboard.mjs";

test("keyboard delay is applied between keydown and keyup for press and type", async (t) => {
  const events = [];
  t.mock.method(globalThis, "setTimeout", (callback, delay) => {
    events.push(delay);
    callback();
  });
  const keyboard = makeKeyboard({
    async _ensureLive() {},
    async _sessionEval(script) {
      events.push(script.includes('new KeyboardEvent("keydown"') ? "down" : "up");
    },
  });

  await keyboard.press("a");
  await keyboard.press("b", { delay: 11 });
  await keyboard.type("cd", { delay: 25 });
  assert.deepEqual(events, ["down", "up", "down", 11, "up", "down", 25, "up", "down", 25, "up"]);
});
