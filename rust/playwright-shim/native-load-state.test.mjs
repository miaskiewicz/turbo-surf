import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { NativeLoadState } from "./native-load-state.mjs";

test("a live Page does not pass a load wait before actual native load", async () => {
  const page = new EventEmitter();
  const state = new NativeLoadState(page);
  let finished = false;
  const waiting = state.wait("load", { timeout: 1000 }).then(() => {
    finished = true;
  });
  state.observe("domContentLoaded");
  await Promise.resolve();
  assert.equal(finished, false);
  state.observe("load");
  await waiting;
  assert.equal(finished, true);
  await state.wait("domcontentloaded", { timeout: 1 });
});

test("a real new document resets prior load and closure rejects its wait", async () => {
  const page = new EventEmitter();
  const state = new NativeLoadState(page);
  state.observe("load");
  state.reset();
  const waiting = state.wait("load", { timeout: 1000 });
  page.emit("close");
  await assert.rejects(waiting, /closed/);
});
