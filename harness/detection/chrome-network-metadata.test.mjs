import assert from "node:assert/strict";
import { test } from "node:test";
import {
  enrichResponseTransport,
  responseNetworkEvents,
  responseTransport,
  summarizeResponseTransport,
  summarizeNativeResponseTransport,
} from "./chrome-network-metadata.mjs";

const url = "https://example.test/resource";
const requestEvent = (requestId, target, wallTime, extra = {}) => ({
  event: "requestWillBeSent",
  targetScope: "main-frame-navigation",
  params: { requestId, request: { url: target, method: "GET" }, wallTime, ...extra },
});
const mainRequests = (rows) =>
  rows.map((row) => ({
    cdpTargetScope: "main-frame-navigation",
    timing: { startTime: row.timeMs },
    ...row,
  }));
const responseEvent = (requestId, target, protocol) => ({
  event: "responseReceived",
  params: { requestId, response: { url: target, status: 200, protocol } },
});

test("CDP response metadata retains TLS, timing and cache fields without inventing omitted values", () => {
  const securityDetails = {
    protocol: "TLS 1.3",
    cipher: "AES_128_GCM",
    subjectName: "example.test",
    sanList: ["example.test"],
    issuer: "Test certificate",
  };
  const timing = { requestTime: 1234, dnsStart: -1, dnsEnd: -1, sslStart: 1, sslEnd: 9 };
  const metadata = responseTransport({
    protocol: "h2",
    securityDetails,
    timing,
    connectionId: 0,
    remotePort: 443,
    connectionReused: false,
    fromDiskCache: false,
  });
  assert.deepEqual(metadata.securityDetails, securityDetails);
  assert.deepEqual(metadata.timing, timing);
  assert.equal(metadata.connectionId, 0);
  assert.equal(metadata.connectionReused, false);
  assert.equal(metadata.fromDiskCache, false);
  assert.equal(metadata.fromServiceWorker, null);
  assert.equal(metadata.remoteIPAddress, null);
  assert.equal(responseTransport({}).protocol, null);
  assert.equal(responseTransport({ protocol: "http/1.1" }).securityDetails, null);
  assert.ok(responseNetworkEvents.includes("responseReceived"));
  assert.ok(responseNetworkEvents.includes("loadingFinished"));
});

test("Patchright association preserves unique redirect hops and parallel requests", () => {
  const requests = mainRequests([
    { id: 1, url, method: "GET", timeMs: 1000 },
    { id: 2, url: url + "/final", method: "GET", timeMs: 2000 },
    { id: 3, url: url + "/parallel", method: "GET", timeMs: 3000 },
  ]);
  const responses = [
    { requestId: 1, url, status: 302 },
    { requestId: 2, url: url + "/final", status: 200 },
    { requestId: 3, url: url + "/parallel", status: 200 },
  ];
  enrichResponseTransport(requests, responses, [
    requestEvent("a", url, 1),
    requestEvent("b", url + "/parallel", 3),
    responseEvent("b", url + "/parallel", "http/1.1"),
    requestEvent("a", url + "/final", 2, {
      redirectResponse: { url, status: 302, protocol: "h2" },
    }),
    responseEvent("a", url + "/final", "h3"),
  ]);
  assert.deepEqual(
    responses.map((row) => row.transport.protocol),
    ["h2", "h3", "http/1.1"],
  );
  assert.deepEqual(
    responses.map((row) => row.protocolRequestId),
    ["a", "a", "b"],
  );
});

test("missing events and ambiguous request associations stay explicitly unknown", () => {
  const requests = mainRequests([
    { id: 1, url, method: "GET", timeMs: 1000 },
    { id: 2, url, method: "GET", timeMs: 1000 },
  ]);
  const responses = requests.map((request) => ({ requestId: request.id, url, status: 200 }));
  enrichResponseTransport(requests, responses, [
    requestEvent("a", url, 1),
    responseEvent("a", url, "h2"),
    responseEvent("unmatched", "https://other.test/", "h3"),
  ]);
  assert.ok(responses.every((row) => row.transport.source === "unavailable"));
  assert.equal(summarizeResponseTransport(responses).unknown, 2);
});

test("CDP request IDs remain session scoped and incomplete wall times cannot match", () => {
  const requests = mainRequests([
    { id: 1, url, method: "GET", timeMs: 1000, sessionId: "page" },
    { id: 2, url, method: "GET", timeMs: 1000, sessionId: "worker" },
    { id: 3, url, method: "GET", timeMs: 5000, sessionId: "missing" },
  ]);
  const responses = requests.map((request) => ({ requestId: request.id, url, status: 200 }));
  enrichResponseTransport(requests, responses, [
    { ...requestEvent("a", url, 1), sessionId: "page" },
    { ...requestEvent("a", url, 1), sessionId: "worker" },
    { ...responseEvent("a", url, "h3"), sessionId: "worker" },
    { ...responseEvent("a", url, "h2"), sessionId: "page" },
    { ...requestEvent("a", url, undefined), sessionId: "missing" },
    { ...responseEvent("a", url, "h2"), sessionId: "missing" },
  ]);
  assert.deepEqual(
    responses.map((row) => row.transport.protocol ?? null),
    ["h2", "h3", null],
  );
});

test("main-page CDP metadata cannot attach to a closer worker or iframe callback", () => {
  const requests = [
    {
      id: 1,
      url,
      method: "GET",
      timeMs: 1005,
      timing: { startTime: 1000 },
      cdpTargetScope: "main-frame-navigation",
    },
    {
      id: 2,
      url,
      method: "GET",
      timeMs: 1000,
      timing: { startTime: 1000 },
      cdpTargetScope: "other-target",
    },
    {
      id: 3,
      url,
      method: "GET",
      timeMs: 1000,
      timing: { startTime: 1000 },
      cdpTargetScope: "unverified-target",
    },
  ];
  const responses = requests.map((row) => ({ requestId: row.id, url, status: 200 }));
  enrichResponseTransport(requests, responses, [
    requestEvent("main", url, 1),
    responseEvent("main", url, "h2"),
  ]);
  assert.equal(responses[0].transport.protocol, "h2");
  assert.equal(responses[1].transport.source, "unavailable");
  assert.equal(responses[2].transport.source, "unavailable");
  const excluded = [{ requestId: 1, url, status: 200 }];
  enrichResponseTransport([requests[0]], excluded, [
    { ...requestEvent("child", url, 1), targetScope: "unverified-target" },
    responseEvent("child", url, "h3"),
  ]);
  assert.equal(excluded[0].transport.source, "unavailable");
});

test("repeated same-frame URLs and missing browser timing remain unavailable", () => {
  const duplicateRequests = mainRequests([
    { id: 1, url, method: "GET", timeMs: 1000 },
    { id: 2, url, method: "GET", timeMs: 2000 },
  ]);
  const duplicates = duplicateRequests.map((row) => ({ requestId: row.id, url, status: 200 }));
  enrichResponseTransport(duplicateRequests, duplicates, [
    requestEvent("a", url, 1),
    responseEvent("a", url, "h2"),
    requestEvent("b", url, 2),
    responseEvent("b", url, "h3"),
  ]);
  assert.ok(duplicates.every((row) => row.transport.source === "unavailable"));
  for (const startTime of [undefined, NaN, Infinity, -1, 0]) {
    const rows = [{ requestId: 1, url, status: 200 }];
    enrichResponseTransport(
      mainRequests([{ id: 1, url, method: "GET", timeMs: 1000, timing: { startTime } }]),
      rows,
      [requestEvent("a", url, 1), responseEvent("a", url, "h2")],
    );
    assert.equal(rows[0].transport.source, "unavailable", String(startTime));
  }
  const duplicateEvents = [{ requestId: 1, url, status: 200 }];
  enrichResponseTransport(
    mainRequests([{ id: 1, url, method: "GET", timeMs: 1000 }]),
    duplicateEvents,
    [
      requestEvent("a", url, 1),
      responseEvent("a", url, "h2"),
      requestEvent("b", url, 2),
      responseEvent("b", url, "h3"),
    ],
  );
  assert.equal(duplicateEvents[0].transport.source, "unavailable");
});

test("public transport aggregates contain only fixed buckets and availability counts", () => {
  const secret = "private-cookie-credential-query";
  const rows = [
    {
      transport: responseTransport({
        protocol: "h2",
        securityDetails: { protocol: "TLS 1.3", issuer: secret, sanList: [secret] },
        timing: { requestTime: 987654321, sslStart: 1, sslEnd: 5 },
        connectionId: 777777,
        remoteIPAddress: secret,
        connectionReused: true,
        fromDiskCache: false,
      }),
    },
    { transport: responseTransport({ protocol: secret, securityDetails: { protocol: secret } }) },
    { status: 200, protocol: "h3" },
    { transport: { source: "unavailable" } },
  ];
  const summary = summarizeResponseTransport(rows);
  assert.equal(summary.responses, 4);
  assert.equal(summary.observed, 2);
  assert.equal(summary.unknown, 2);
  assert.equal(summary.protocols.h2, 1);
  assert.equal(summary.protocols.other, 1);
  assert.equal(summary.protocols.unknown, 2);
  assert.equal(summary.tlsProtocols["TLS 1.3"], 1);
  assert.deepEqual(summary.timing, { present: 1, missing: 3 });
  assert.deepEqual(summary.connectionReused, { true: 1, false: 0, unknown: 3 });
  assert.deepEqual(summary.fromDiskCache, { true: 0, false: 1, unknown: 3 });
  assert.doesNotMatch(JSON.stringify(summary), /private|987654321|777777|example\.test/);
});

test("native application protocol evidence never becomes an inferred TLS or packet measurement", () => {
  const summary = summarizeNativeResponseTransport([
    { protocol: "HTTP/2.0" },
    { protocol: "HTTP/3.0" },
    { protocol: "HTTP/1.1" },
    { protocol: "private-future-protocol" },
    {},
  ]);
  assert.equal(summary.source, "native-application-response");
  assert.equal(summary.observed, 4);
  assert.equal(summary.unknown, 1);
  assert.equal(summary.protocols.h2, 1);
  assert.equal(summary.protocols.h3, 1);
  assert.equal(summary.protocols.other, 1);
  assert.equal(summary.tlsDetails, "not-collected");
  assert.equal(summary.packetCapture, false);
  assert.doesNotMatch(JSON.stringify(summary), /private/);
});
