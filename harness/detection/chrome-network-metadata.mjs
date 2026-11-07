// CDP reports browser response metadata, not packets or a ClientHello capture.
export const responseNetworkEvents = [
  "requestWillBeSent",
  "requestWillBeSentExtraInfo",
  "responseReceived",
  "responseReceivedExtraInfo",
  "loadingFinished",
  "loadingFailed",
];

export function responseTransport(response) {
  const value = (key, type) => (typeof response?.[key] === type ? response[key] : null);
  return {
    source: "cdp-response",
    protocol: value("protocol", "string"),
    securityState: value("securityState", "string"),
    securityDetails: response?.securityDetails ?? null,
    timing: response?.timing ?? null,
    connectionReused: value("connectionReused", "boolean"),
    connectionId: value("connectionId", "number"),
    remoteIPAddress: value("remoteIPAddress", "string"),
    remotePort: value("remotePort", "number"),
    fromDiskCache: value("fromDiskCache", "boolean"),
    fromServiceWorker: value("fromServiceWorker", "boolean"),
    fromPrefetchCache: value("fromPrefetchCache", "boolean"),
    alternateProtocolUsage: value("alternateProtocolUsage", "string"),
  };
}

// Patchright context callbacks include targets outside the page CDP session.
// Enrich only a one-to-one URL/method pair in independently verified main-frame navigation
// scopes. Clock values are validated, but cross-session start times and callback
// arrival times are not treated as request identity anchors.
export function enrichResponseTransport(requests, responses, events) {
  const assigned = new Set();
  const requestMatches = new Map();
  const key = (event) => `${event.sessionId ?? ""}:${event.params.requestId}`;
  for (const event of events) {
    if (event.event === "requestWillBeSent") {
      const { redirectResponse } = event.params;
      if (redirectResponse)
        attachTransport(responses, event, redirectResponse, requestMatches.get(key(event)));
      requestMatches.delete(key(event));
      if (event.targetScope !== "main-frame-navigation") continue;
      const first = uniqueNavigationRequest(requests, events, event, assigned);
      if (!first) continue;
      assigned.add(first);
      requestMatches.set(key(event), first);
    } else if (event.event === "responseReceived")
      attachTransport(responses, event, event.params.response, requestMatches.get(key(event)));
  }
  for (const row of responses)
    if (!row.transport)
      row.transport = { source: "unavailable", reason: "no-unambiguous-cdp-response" };
}

const positiveFinite = (value) => Number.isFinite(value) && value > 0;

// The single unassigned navigation row a main-frame requestWillBeSent identifies, or null
// when the URL/method pair is ambiguous on either side or its clocks are unusable.
export function uniqueNavigationRequest(requests, events, event, assigned) {
  const { request, wallTime } = event.params;
  const candidates = requests.filter(
    (row) =>
      row.cdpTargetScope === "main-frame-navigation" &&
      row.url === request.url &&
      row.method === request.method &&
      (event.sessionId === undefined || row.sessionId === event.sessionId),
  );
  const first = candidates[0];
  const matchingEvents = events.filter(
    (entry) =>
      entry.event === "requestWillBeSent" &&
      entry.targetScope === "main-frame-navigation" &&
      entry.sessionId === event.sessionId &&
      entry.params.request.url === request.url &&
      entry.params.request.method === request.method,
  );
  const unique = candidates.length === 1 && matchingEvents.length === 1 && !assigned.has(first);
  return unique && positiveFinite(wallTime * 1000) && positiveFinite(first?.timing?.startTime)
    ? first
    : null;
}

function attachTransport(responses, event, response, request) {
  if (!request) return;
  const candidates = responses.filter(
    (row) =>
      !row.transport &&
      row.requestId === request.id &&
      row.url === response.url &&
      row.status === response.status,
  );
  if (candidates.length !== 1) return;
  const row = candidates[0];
  row.protocolRequestId = event.params.requestId;
  row.transport = responseTransport(response);
  row.transport.association = "main-frame-navigation-unique-url-method";
}

const histogram = (values) => Object.fromEntries(values.map((value) => [value, 0]));
// A histogram bucket: "unknown" for a missing value, "other" for one outside the buckets.
const bucketOf = (counts, value) =>
  value == null ? "unknown" : Object.hasOwn(counts, value) ? value : "other";

function tallyResponseTransport(summary, response) {
  const transport = response.transport?.source === "cdp-response" ? response.transport : null;
  summary[transport ? "observed" : "unknown"]++;
  summary.protocols[bucketOf(summary.protocols, transport?.protocol)]++;
  summary.tlsProtocols[bucketOf(summary.tlsProtocols, transport?.securityDetails?.protocol)]++;
  for (const field of ["securityDetails", "timing"])
    summary[field][transport?.[field] ? "present" : "missing"]++;
  for (const field of ["connectionReused", "fromDiskCache", "fromServiceWorker"])
    summary[field][
      typeof transport?.[field] === "boolean" ? String(transport[field]) : "unknown"
    ]++;
}

export function summarizeResponseTransport(responses = []) {
  const summary = {
    source: "cdp-response",
    packetCapture: false,
    responses: responses.length,
    observed: 0,
    unknown: 0,
    protocols: histogram(["h2", "h3", "http/1.1", "http/1.0", "other", "unknown"]),
    tlsProtocols: histogram(["TLS 1.2", "TLS 1.3", "QUIC", "other", "unknown"]),
    securityDetails: { present: 0, missing: 0 },
    timing: { present: 0, missing: 0 },
    connectionReused: { true: 0, false: 0, unknown: 0 },
    fromDiskCache: { true: 0, false: 0, unknown: 0 },
    fromServiceWorker: { true: 0, false: 0, unknown: 0 },
  };
  for (const response of responses) tallyResponseTransport(summary, response);
  return summary;
}

export function summarizeNativeResponseTransport(responses = []) {
  const protocols = { h2: 0, h3: 0, "http/1.1": 0, "http/1.0": 0, other: 0, unknown: 0 };
  const known = new Map([
    ["HTTP/2.0", "h2"],
    ["HTTP/2", "h2"],
    ["HTTP/3.0", "h3"],
    ["HTTP/3", "h3"],
    ["HTTP/1.1", "http/1.1"],
    ["HTTP/1.0", "http/1.0"],
  ]);
  for (const row of responses)
    protocols[
      typeof row.protocol !== "string" ? "unknown" : (known.get(row.protocol) ?? "other")
    ]++;
  return {
    source: "native-application-response",
    responses: responses.length,
    observed: responses.length - protocols.unknown,
    unknown: protocols.unknown,
    protocols,
    tlsDetails: "not-collected",
    packetCapture: false,
  };
}
