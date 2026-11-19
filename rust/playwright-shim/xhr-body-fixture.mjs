// Shared localhost probe for the native shim and the Chromium oracle.
import assert from "node:assert/strict";
import { createServer } from "node:http";

async function xhrBodyProbe() {
  const rows = [],
    unhandled = [];
  let pending;
  const onUnhandled = (event) => {
    const error = { name: event.reason?.name, message: event.reason?.message };
    unhandled.push(error);
    pending?.({ event: "unhandled", ...error });
  };
  addEventListener("unhandledrejection", onUnhandled);
  try {
    for (const method of ["GET", "get", "HEAD", "hEaD", "POST"]) {
      const kinds = ["empty", "payload", "object"];
      if (method === "GET") kinds.push("blob", "params", "form", "buffer", "view", "document");
      for (const kind of kinds) {
        let conversions = 0;
        const values = {
          empty: "",
          payload: "retained-payload",
          object: {},
          blob: new Blob(["ignored"], { type: "application/x-ignored" }),
          params: new URLSearchParams("ignored=yes"),
          form: new FormData(),
          buffer: new ArrayBuffer(3),
          view: new Uint8Array([1, 2, 3]),
          document: document.implementation.createHTMLDocument("ignored"),
        };
        const body = values[kind];
        if (typeof body === "object")
          body.toString = () => {
            conversions++;
            if (method === "POST") return "retained-object";
            throw new Error("XHR string conversion fixture");
          };
        const result = await new Promise((resolve) => {
          pending = resolve;
          const xhr = new XMLHttpRequest();
          xhr.onload = () => resolve({ event: "load", status: xhr.status, text: xhr.responseText });
          xhr.onerror = () => resolve({ event: "error", status: xhr.status });
          try {
            xhr.open(method, `/api/${method}/${kind}`);
            if (kind === "payload" && method !== "POST")
              xhr.setRequestHeader("Content-Type", "application/x-fixture");
            xhr.send(body);
          } catch (error) {
            resolve({ event: "throw", name: error.name, message: error.message });
          }
        });
        pending = null;
        rows.push({ method, kind, conversions, ...result });
      }
    }
    const strictFetch = [],
      strictRequest = [];
    for (const method of ["GET", "HEAD"]) {
      try {
        new Request(location.href, { method, body: "forbidden" });
        strictRequest.push("accepted");
      } catch (error) {
        strictRequest.push(error.name);
      }
      try {
        await fetch("/api/forbidden-fetch-body", { method, body: "forbidden" });
        strictFetch.push("accepted");
      } catch (error) {
        strictFetch.push(error.name);
      }
    }
    return { rows, unhandled, strictFetch, strictRequest };
  } finally {
    removeEventListener("unhandledrejection", onUnhandled);
  }
}

export async function checkXhrBodies(page) {
  const wire = [],
    routes = [];
  const server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    if (request.url.startsWith("/api/"))
      wire.push({
        method: request.method,
        body: Buffer.concat(chunks).toString(),
        contentType: request.headers["content-type"] ?? null,
      });
    response.setHeader("content-type", request.url === "/" ? "text/html" : "text/plain");
    response.end(request.url === "/" ? "<!doctype html><body></body>" : "ok");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    await page.route("**/api/**", async (route, request) => {
      routes.push({
        method: request.method(),
        body: request.method() === "POST" ? (request.postData() ?? "") : request.postData(),
        contentType: request.headers()["content-type"] ?? null,
      });
      await route.continue();
    });
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    const result = await page.evaluate(xhrBodyProbe);
    assert.deepEqual(result.unhandled, [], JSON.stringify(result));
    assert.deepEqual(result.strictFetch, ["TypeError", "TypeError"]);
    assert.deepEqual(result.strictRequest, ["TypeError", "TypeError"]);
    assert.equal(result.rows.length, 21);
    for (const row of result.rows) {
      if (row.kind === "object" && row.method !== "POST") {
        assert.equal(row.event, "throw", JSON.stringify(row));
        assert.equal(row.name, "Error");
        assert.equal(row.conversions, 1);
        continue;
      }
      assert.equal(row.event, "load", JSON.stringify(row));
      assert.equal(row.status, 200);
      assert.equal(row.conversions, row.kind === "object" ? 1 : 0);
      assert.equal(row.text, row.method.toUpperCase() === "HEAD" ? "" : "ok");
    }
    const expectedRoutes = result.rows
      .filter((row) => row.event === "load")
      .map(({ method, kind }) => ({
        method: method.toUpperCase(),
        body:
          method === "POST"
            ? kind === "empty"
              ? ""
              : kind === "object"
                ? "retained-object"
                : "retained-payload"
            : null,
        contentType:
          method === "POST"
            ? "text/plain;charset=UTF-8"
            : kind === "payload"
              ? "application/x-fixture"
              : null,
      }));
    assert.deepEqual(routes, expectedRoutes);
    assert.deepEqual(
      wire,
      expectedRoutes.map(({ method, body, contentType }) => ({
        method,
        body: body ?? "",
        contentType,
      })),
    );
    return { ...result, routes, wire };
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}
