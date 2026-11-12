import { readFile } from "node:fs/promises";

function overrideRequest(request, options = {}) {
  const next = { ...request };
  if (options.url !== undefined) next.url = String(options.url);
  if (options.method !== undefined) next.method = String(options.method).toUpperCase();
  if (options.headers !== undefined) {
    next.overriddenHeaders = true;
    next.headers = Object.fromEntries(
      Object.entries(options.headers)
        .filter(([, v]) => v !== undefined)
        .map(([k, v]) => [k.toLowerCase(), String(v)]),
    );
  }
  if (options.postData !== undefined) {
    const body = options.postData;
    next.bodyBytes = Array.from(
      Buffer.isBuffer(body)
        ? body
        : Buffer.from(typeof body === "object" ? JSON.stringify(body) : String(body)),
    );
  }
  return next;
}

async function fulfillPayload(options = {}) {
  const response = options.response;
  const status = options.status ?? response?.status() ?? 200;
  if (!Number.isInteger(status) || status < 200 || status > 599)
    throw new RangeError("Invalid route.fulfill response status");
  const headers = Object.fromEntries(
    Object.entries(options.headers ?? (response ? await response.allHeaders() : {})).map(
      ([name, value]) => [name.toLowerCase(), String(value)],
    ),
  );
  let body = response ? await response.body() : Buffer.alloc(0);
  if (options.path !== undefined) body = await readFile(options.path);
  if (options.body !== undefined)
    body = Buffer.isBuffer(options.body) ? options.body : Buffer.from(String(options.body));
  if (options.json !== undefined) {
    body = Buffer.from(JSON.stringify(options.json));
    if (!Object.keys(headers).some((k) => k.toLowerCase() === "content-type"))
      headers["content-type"] = "application/json";
  }
  if (options.contentType !== undefined) headers["content-type"] = String(options.contentType);
  headers["content-length"] = String(body.length);
  return {
    status,
    headers,
    bodyBytes: Array.from(body),
  };
}

export function routeResponse(record) {
  let bytes = Buffer.from(record.bodyBytes);
  const ensure = () => {
    if (bytes === null) throw new Error("Response has been disposed");
    return bytes;
  };
  return {
    url: () => record.url,
    status: () => record.status,
    statusText: () => record.statusText,
    ok: () => record.status >= 200 && record.status < 300,
    headers: () => Object.fromEntries(record.headers),
    async allHeaders() {
      return Object.fromEntries(record.headers);
    },
    async headersArray() {
      return record.headers.map(([name, value]) => ({ name, value }));
    },
    async body() {
      return Buffer.from(ensure());
    },
    async text() {
      return ensure().toString("utf8");
    },
    async json() {
      return JSON.parse(ensure().toString("utf8"));
    },
    async dispose() {
      bytes = null;
    },
  };
}

export async function invokeHostRoute(handler, data, createRequest, fetchResponse, signal) {
  const requestData = data.request;
  const request = createRequest(requestData);
  let decision = null;
  let handling;
  const settle = (next) => {
    if (signal.aborted) throw signal.reason;
    if (decision || handling) throw new Error("Route is already handled");
    decision = next;
  };
  const route = {
    request: () => request,
    fulfill(options) {
      if (signal.aborted) return Promise.reject(signal.reason);
      if (decision || handling) return Promise.reject(new Error("Route is already handled"));
      handling = fulfillPayload(options).then((fulfilled) => {
        if (signal.aborted) throw signal.reason;
        decision = { fulfilled };
      });
      return handling;
    },
    async continue(options) {
      settle({ action: "continue", request: overrideRequest(requestData, options) });
    },
    async fallback(options) {
      settle({ action: "fallback", request: overrideRequest(requestData, options) });
    },
    async abort(errorCode = "failed") {
      settle({ action: "abort", errorCode: String(errorCode) });
    },
    async fetch(options) {
      if (decision) throw new Error("Route is already handled");
      const next = overrideRequest(requestData, options);
      return fetchResponse(next, options ?? {});
    },
  };
  let abort;
  const canceled = new Promise((_, reject) => {
    abort = () => reject(signal.reason);
    if (signal.aborted) abort();
    else signal.addEventListener("abort", abort, { once: true });
  });
  try {
    await Promise.race([
      Promise.resolve().then(async () => {
        signal.throwIfAborted();
        await handler(route, request);
        await handling;
      }),
      canceled,
    ]);
  } finally {
    signal.removeEventListener("abort", abort);
  }
  if (!decision) throw new Error("Route handler completed without handling the request");
  return decision;
}
