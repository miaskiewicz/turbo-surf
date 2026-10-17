export function enrichProtocolHeaders(requests, protocolNetwork) {
  // CDP ExtraInfo preserves browser-generated headers and cookie-blocking reasons.
  // Redirect hops reuse IDs; only complete, order-validated groups can be joined.
  const mains = protocolNetwork.filter((e) => e.event === "requestWillBeSent");
  const extras = protocolNetwork.filter((e) => e.event === "requestWillBeSentExtraInfo");
  const sameSession = (left, right) => left.sessionId === right.sessionId;
  const sameGroup = (left, right) =>
    sameSession(left, right) && left.params.requestId === right.params.requestId;
  const sameRequest = (row, event) =>
    row.url === event.params.request.url && row.method === event.params.request.method;
  for (const row of requests)
    row.headerScope ||=
      row.protocolRequestId === undefined ? "automation-api" : "cdp-request-event";
  const visited = new Set();
  for (const main of mains) {
    if (visited.has(main)) continue;
    const ownMains = mains.filter((event) => sameGroup(event, main));
    const ownExtras = extras.filter((event) => sameGroup(event, main));
    for (const event of ownMains) visited.add(event);
    if (ownMains.length !== ownExtras.length) continue;
    const rawRows = requests.filter(
      (row) => sameSession(row, main) && row.protocolRequestId === main.params.requestId,
    );
    let rows;
    if (rawRows.length) {
      // Direct CDP supplies the authoritative session/request IDs on each hop.
      if (
        rawRows.length !== ownMains.length ||
        !rawRows.every((row, index) => sameRequest(row, ownMains[index]))
      )
        continue;
      rows = rawRows;
    } else {
      rows = ownMains.map((event) => {
        if (event.targetScope !== "main-frame-navigation") return null;
        const candidates = requests.filter(
          (row) =>
            row.protocolRequestId === undefined &&
            row.cdpTargetScope === "main-frame-navigation" &&
            sameSession(row, event) &&
            sameRequest(row, event),
        );
        const matchingEvents = mains.filter(
          (entry) =>
            sameSession(entry, event) &&
            entry.targetScope === "main-frame-navigation" &&
            entry.params.request.url === event.params.request.url &&
            entry.params.request.method === event.params.request.method,
        );
        const row = candidates[0];
        const start = row?.timing?.startTime;
        return candidates.length === 1 &&
          matchingEvents.length === 1 &&
          Number.isFinite(start) &&
          start > 0 &&
          Number.isFinite(event.params.wallTime) &&
          event.params.wallTime > 0
          ? row
          : null;
      });
    }
    for (const [index, row] of rows.entries()) {
      if (!row || row.cdpRequestId !== undefined) continue;
      row.automationHeaders = row.headers;
      row.headers = Object.fromEntries(
        Object.entries(ownExtras[index].params.headers).map(([name, value]) => [
          name.toLowerCase(),
          value,
        ]),
      );
      row.headerScope = "cdp-extra-info";
      row.cdpRequestId = main.params.requestId;
    }
  }
}
