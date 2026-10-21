import assert from "node:assert/strict";
import test from "node:test";
import { waitForWebSocketOpen } from "./websocket-open.mjs";

class Socket extends EventTarget {
  readyState = 0;
}

test("Chrome socket startup resolves on open and removes startup listeners", async () => {
  const socket = new Socket();
  const result = waitForWebSocketOpen(socket, 100);
  socket.dispatchEvent(new Event("open"));
  await result;
  socket.dispatchEvent(new Event("error"));
  socket.dispatchEvent(new Event("close"));
  const alreadyOpen = new Socket();
  alreadyOpen.readyState = 1;
  await waitForWebSocketOpen(alreadyOpen, 100);
});

test("Chrome socket startup rejects timeout, error and early close", async () => {
  await assert.rejects(waitForWebSocketOpen(new Socket(), 1), /open timed out/);
  for (const event of ["error", "close"]) {
    const socket = new Socket();
    const result = waitForWebSocketOpen(socket, 100);
    socket.dispatchEvent(new Event(event));
    await assert.rejects(result, /before opening/);
  }
  const closed = new Socket();
  closed.readyState = 3;
  await assert.rejects(waitForWebSocketOpen(closed, 100), /closed before opening/);
});
