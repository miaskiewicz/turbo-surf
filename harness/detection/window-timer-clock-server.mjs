import { createServer } from "node:http";
import { setTimeout as pause } from "node:timers/promises";

export async function startWindowTimerClockServer() {
  const receipts = {
    delayedBody: { requests: 0, headersFlushed: false, bodyWritten: false },
    stalledBody: { requests: 0, headersFlushed: false, bodyWritten: false, socketClosed: false },
  };
  let closed;
  const socketClosed = new Promise((resolve) => {
    closed = resolve;
  });
  const server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    if (path === "/body" || path === "/stall") {
      const receipt = path === "/body" ? receipts.delayedBody : receipts.stalledBody;
      receipt.requests++;
      response.writeHead(200, {
        "Content-Type": "text/plain",
        "Content-Length": "4",
        Connection: "close",
      });
      response.flushHeaders();
      receipt.headersFlushed = true;
      if (path === "/body") {
        await pause(20);
        response.end("done");
        receipt.bodyWritten = true;
      } else {
        request.socket.once("close", () => {
          receipt.socketClosed = true;
          closed();
        });
      }
      return;
    }
    response.writeHead(200, { "Content-Type": "text/html", "Cache-Control": "no-store" });
    response.end("<!doctype html><body>");
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return {
    url: `http://127.0.0.1:${server.address().port}/`,
    receipts,
    async waitForSocketCancellation() {
      let deadline;
      try {
        await Promise.race([
          socketClosed,
          new Promise((_, reject) => {
            deadline = setTimeout(
              () => reject(new Error("Chrome did not cancel stalled socket")),
              2000,
            );
          }),
        ]);
      } finally {
        clearTimeout(deadline);
      }
    },
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
