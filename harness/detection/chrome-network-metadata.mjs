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
      const { request, wallTime, redirectResponse } = event.params;
      if (redirectResponse) attach(event, redirectResponse, requestMatches.get(key(event)));
      requestMatches.delete(key(event));
      if (event.targetScope !== "main-frame-navigation") continue;
      const candidates = requests.filter(
        (row) =>
          row.cdpTargetScope === "main-frame-navigation" &&
          row.url === request.url &&
          row.method === request.method &&
          (event.sessionId === undefined || row.sessionId === event.sessionId),
      );
      const time = wallTime * 1000;
      const first = candidates[0];
      const browserStart = first?.timing?.startTime;
      const matchingEvents = events.filter(
        (entry) =>
          entry.event === "requestWillBeSent" &&
          entry.targetScope === "main-frame-navigation" &&
          entry.sessionId === event.sessionId &&
          entry.params.request.url === request.url &&
          entry.params.request.method === request.method,
      );
      if (
        candidates.length === 1 &&
        matchingEvents.length === 1 &&
        !assigned.has(first) &&
        Number.isFinite(time) &&
        time > 0 &&
        Number.isFinite(browserStart) &&
        browserStart > 0
      ) {
        assigned.add(first);
        requestMatches.set(key(event), first);
      }
    } else if (event.event === "responseReceived")
      attach(event, event.params.response, requestMatches.get(key(event)));
  }
  for (const row of responses)
    if (!row.transport)
      row.transport = { source: "unavailable", reason: "no-unambiguous-cdp-response" };

  function attach(event, response, request) {
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
}

export function summarizeResponseTransport(responses = []) {
  const histogram = (values) => Object.fromEntries(values.map((value) => [value, 0]));
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
  for (const response of responses) {
    const transport = response.transport?.source === "cdp-response" ? response.transport : null;
    summary[transport ? "observed" : "unknown"]++;
    const protocol = transport?.protocol;
    const tls = transport?.securityDetails?.protocol;
    summary.protocols[
      protocol == null ? "unknown" : Object.hasOwn(summary.protocols, protocol) ? protocol : "other"
    ]++;
    summary.tlsProtocols[
      tls == null ? "unknown" : Object.hasOwn(summary.tlsProtocols, tls) ? tls : "other"
    ]++;
    for (const field of ["securityDetails", "timing"])
      summary[field][transport?.[field] ? "present" : "missing"]++;
    for (const field of ["connectionReused", "fromDiskCache", "fromServiceWorker"])
      summary[field][
        typeof transport?.[field] === "boolean" ? String(transport[field]) : "unknown"
      ]++;
  }
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
