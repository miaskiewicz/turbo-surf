export function enrichProtocolHeaders(requests, protocolNetwork) {
  // Automation APIs can omit browser-generated headers. CDP ExtraInfo records
  // retain those headers and cookie-blocking reasons in the intrusive pass.
  const mains = protocolNetwork.filter((e) => e.event === "requestWillBeSent");
  const extras = protocolNetwork.filter((e) => e.event === "requestWillBeSentExtraInfo");
  for (const main of mains) {
    const ownMains = mains.filter((e) => e.params.requestId === main.params.requestId);
    const ownExtras = extras.filter((e) => e.params.requestId === main.params.requestId);
    if (ownMains.length !== ownExtras.length) continue; // preserve raw ambiguous redirect events
    const extra = ownExtras[ownMains.indexOf(main)];
    const candidates = requests.filter(
      (r) =>
        !r.cdpRequestId &&
        r.url === main.params.request.url &&
        r.method === main.params.request.method,
    );
    const timestamp = main.params.wallTime * 1000;
    candidates.sort((a, b) => Math.abs(a.timeMs - timestamp) - Math.abs(b.timeMs - timestamp));
    const row = candidates[0];
    if (!row) continue;
    row.automationHeaders = row.headers;
    row.headers = Object.fromEntries(
      Object.entries(extra.params.headers).map(([k, v]) => [k.toLowerCase(), v]),
    );
    row.headerScope = "cdp-extra-info";
    row.cdpRequestId = main.params.requestId;
  }
}
