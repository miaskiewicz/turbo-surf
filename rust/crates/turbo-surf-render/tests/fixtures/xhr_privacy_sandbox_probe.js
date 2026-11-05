(() => {
  const t = (f) => {
    try {
      return "ok:" + String(f());
    } catch (e) {
      return e.name + ": " + e.message;
    }
  };
  const log = [];
  const spy = (target) =>
    new Proxy(target, {
      get(o, k) {
        log.push(String(k));
        return o[k];
      },
    });
  const P = XMLHttpRequest.prototype;
  const d = (k) => Object.getOwnPropertyDescriptor(P, k);
  const x = new XMLHttpRequest();
  const r = {
    order: Object.getOwnPropertyNames(P).slice(-4),
    shape: ["setAttributionReporting", "setPrivateToken"].map((k) => [
      d(k).value.name,
      d(k).value.length,
      d(k).enumerable,
      d(k).writable,
      String(d(k).value),
    ]),
  };
  r.attribution = [
    t(() => x.setAttributionReporting(spy({}))),
    log.splice(0).join(),
    t(() => x.setAttributionReporting()),
    t(() => x.setAttributionReporting(5)),
    t(() => P.setAttributionReporting.call({}, {})),
  ];
  r.closed = [
    t(() => x.setPrivateToken()),
    t(() => P.setPrivateToken.call({}, {})),
    t(() => x.setPrivateToken(5)),
    t(() => x.setPrivateToken(null)),
    t(() => x.setPrivateToken(spy({}))),
    log.splice(0).join(),
    t(() => x.setPrivateToken(spy({ version: 1, operation: "send-redemption-record" }))),
    log.splice(0).join(),
  ];
  x.open("GET", "/");
  r.opened = [
    t(() => x.setPrivateToken({ operation: "token-request" })),
    t(() => x.setPrivateToken({ version: 1, operation: "bogus" })),
    t(() => x.setPrivateToken({ version: 2, operation: "token-request" })),
    t(() => x.setPrivateToken({ version: "1", operation: "token-request" })),
    t(() => x.setPrivateToken({ version: 1, operation: "token-redemption", refreshPolicy: "bad" })),
    t(() =>
      x.setPrivateToken({ version: 1, operation: "token-redemption", refreshPolicy: "refresh" }),
    ),
    t(() => x.setPrivateToken({ version: 1, operation: "token-request", issuers: 5 })),
    t(() => x.setPrivateToken({ version: 1, operation: "send-redemption-record" })),
    t(() => x.setPrivateToken({ version: 1, operation: "send-redemption-record", issuers: [] })),
    t(() =>
      x.setPrivateToken({
        version: 1,
        operation: "send-redemption-record",
        issuers: ["not a url"],
      }),
    ),
    t(() =>
      x.setPrivateToken({
        version: 1,
        operation: "send-redemption-record",
        issuers: ["http://a.test"],
      }),
    ),
    t(() =>
      x.setPrivateToken({
        version: 1,
        operation: "send-redemption-record",
        issuers: ["https://a.test", "http://localhost:1"],
      }),
    ),
    t(() => x.setAttributionReporting({ eventSourceEligible: true, triggerEligible: false })),
  ];
  x.send();
  r.sent = [
    t(() => x.setPrivateToken({ version: 1, operation: "token-request" })),
    t(() => x.setAttributionReporting({})),
  ];
  x.abort();
  return JSON.stringify(r);
})();
