import { mock, test } from "node:test";
import assert from "node:assert/strict";
import { cdpConnection } from "./cdp-connection.mjs";

test("flattened target commands resolve independently and preserve event sessions", async () => {
  const sent = [],
    events = [];
  const socket = { send: (message) => sent.push(JSON.parse(message)) };
  const connection = cdpConnection(socket, (event) => events.push(event));
  const page = connection.call("Network.getResponseBody", { requestId: "1" }, "page");
  const worker = connection.call("Network.getResponseBody", { requestId: "1" }, "worker");
  assert.deepEqual(
    sent.map((row) => row.sessionId),
    ["page", "worker"],
  );
  socket.onmessage({ data: JSON.stringify({ id: sent[1].id, result: { body: "worker" } }) });
  socket.onmessage({
    data: JSON.stringify({
      method: "Runtime.exceptionThrown",
      sessionId: "worker",
      params: { text: "error" },
    }),
  });
  socket.onmessage({ data: JSON.stringify({ id: sent[0].id, result: { body: "page" } }) });
  assert.deepEqual(await Promise.all([page, worker]), [{ body: "page" }, { body: "worker" }]);
  assert.equal(events[0].sessionId, "worker");
  connection.close();
});

test("closing a connection rejects pending commands; protocol errors stay errors", async () => {
  const sent = [];
  const socket = { send: (message) => sent.push(JSON.parse(message)) };
  const connection = cdpConnection(socket, () => {});
  const failed = connection.call("Page.enable", {}, "worker");
  socket.onmessage({
    data: JSON.stringify({ id: sent[0].id, error: { message: "Unsupported domain" } }),
  });
  await assert.rejects(failed, /Unsupported domain/);
  const pending = connection.call("Runtime.evaluate", {}, "worker");
  connection.close();
  await assert.rejects(pending, /oracle closed/);
});

test("a command fails only after a generous 60 s cap (turbo-surf#50)", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const socket = { send: () => {} };
    const connection = cdpConnection(socket, () => {});
    let settled = null;
    const pending = connection.call("Page.getFrameTree").then(
      () => (settled = "resolved"),
      (error) => (settled = error.message),
    );
    mock.timers.tick(59_999);
    await Promise.resolve();
    assert.equal(settled, null);
    mock.timers.tick(1);
    await pending;
    assert.equal(settled, "CDP Page.getFrameTree timed out");
    connection.close();
  } finally {
    mock.timers.reset();
  }
});
