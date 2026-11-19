import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { waitForEmitterEvent } from "./event-waiter.mjs";

test("an event waiter observes a subsequent real event", async () => {
  const emitter = new EventEmitter();
  const payload = { owned: true };
  const waiting = waitForEmitterEvent(emitter, "popup", { timeout: 1000 });
  emitter.emit("popup", payload);
  assert.equal(await waiting, payload);
  assert.equal(emitter.listenerCount("popup"), 0);
  assert.equal(emitter.listenerCount("close"), 0);
});
test("unmatched load waits reject a TimeoutError", async () => {
  const emitter = new EventEmitter();
  await assert.rejects(waitForEmitterEvent(emitter, "load", { timeout: 10 }), {
    name: "TimeoutError",
  });
  assert.equal(emitter.listenerCount("load"), 0);
});
test("async predicates filter payloads and failures reject the waiter", async () => {
  const emitter = new EventEmitter();
  const waiting = waitForEmitterEvent(emitter, "response", {
    predicate: async (value) => value === "selected",
    timeout: 1000,
  });
  emitter.emit("response", "ignored");
  emitter.emit("response", "selected");
  assert.equal(await waiting, "selected");
  const failed = waitForEmitterEvent(emitter, "popup", () => {
    throw new Error("predicate failed");
  });
  emitter.emit("popup", {});
  await assert.rejects(failed, /predicate failed/);
});
test("closure rejects an unlimited unrelated wait but resolves a close wait", async () => {
  const emitter = new EventEmitter();
  const unrelated = waitForEmitterEvent(emitter, "popup", { timeout: 0 });
  unrelated.catch(() => {});
  const closed = waitForEmitterEvent(emitter, "close", { timeout: 0 });
  emitter.emit("close", emitter);
  assert.equal(await closed, emitter);
  await assert.rejects(unrelated, /closed/i);
});
