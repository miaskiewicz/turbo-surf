(async () => {
  const out = {};
  const D = Document.prototype;
  const shape = (value) => (value === undefined ? "undefined" : value);
  const settle = async (fn, realm = window) => {
    let promise;
    try {
      promise = fn();
    } catch (error) {
      return ["threw", error.name, error.message];
    }
    const own = promise instanceof Promise,
      framed = promise instanceof realm.Promise;
    try {
      return [own, framed, "fulfilled", shape(await promise)];
    } catch (error) {
      return [
        own,
        framed,
        "rejected",
        error.name,
        error.message,
        error instanceof realm.DOMException || error instanceof realm.TypeError,
      ];
    }
  };
  const names = ["browsingTopics", "hasPrivateToken", "hasRedemptionRecord"];
  out.secure = isSecureContext;
  out.present = names.map((name) => [name in document, name in D]);
  out.descriptors = names.map((name) => {
    const d = Object.getOwnPropertyDescriptor(D, name);
    return d
      ? [
          d.enumerable,
          d.configurable,
          d.writable,
          d.value.name,
          d.value.length,
          "prototype" in d.value,
          Function.prototype.toString.call(d.value),
        ]
      : null;
  });
  if (!isSecureContext) return out;

  // browsingTopics: brand, BrowsingTopicsOptions conversion, then Chrome's removal rejection.
  const read = [];
  out.topics = [
    await settle(() => document.browsingTopics()),
    await settle(() => document.browsingTopics({ skipObservation: true })),
    await settle(() =>
      document.browsingTopics({
        get skipObservation() {
          read.push("skipObservation");
          return 1;
        },
      }),
    ),
    await settle(() =>
      document.browsingTopics({
        get skipObservation() {
          throw new RangeError("boom");
        },
      }),
    ),
    await settle(() => document.browsingTopics(null)),
    await settle(() => document.browsingTopics(undefined)),
    await settle(() => document.browsingTopics(() => 1)),
    await settle(() => document.browsingTopics(1)),
    await settle(() => document.browsingTopics("x")),
    await settle(() => D.browsingTopics.call({})),
    await settle(() => D.browsingTopics.call({}, 5)),
    await settle(() => D.browsingTopics.call(null)),
    read,
  ];

  // Issuer validation precedes the document checks; redemption records never exist.
  const issuers = [
    undefined,
    "",
    "notaurl",
    "relative/path",
    "http://issuer.example",
    "https://issuer.example:99999",
    "https://",
    "wss://x.example",
    "blob:https://x.example/u",
    "file:///x",
    "data:,x",
    " https://issuer.example",
    "https://issuer.example",
    "HTTPS://ISSUER.EXAMPLE/path?q",
    " https://issuer.example ",
    "https://u:p@issuer.example\u0000",
    "https://%41.example",
    "https://127.0.0.1",
    "http://127.0.0.1:5",
    "http://localhost:1",
    "http://foo.localhost",
    "http://[::1]",
    Symbol.iterator,
    {
      toString() {
        return "https://object.example";
      },
    },
  ];
  out.redemption = [];
  for (const issuer of issuers)
    out.redemption.push([
      String(issuer),
      await settle(() =>
        issuer === undefined
          ? document.hasRedemptionRecord()
          : document.hasRedemptionRecord(issuer),
      ),
    ]);
  out.invalidTokens = [];
  for (const issuer of issuers.slice(0, 12))
    out.invalidTokens.push([
      String(issuer),
      await settle(() =>
        issuer === undefined ? document.hasPrivateToken() : document.hasPrivateToken(issuer),
      ),
    ]);
  out.invalidTokens.push(["symbol", await settle(() => document.hasPrivateToken(Symbol.iterator))]);
  out.brands = [
    await settle(() => D.hasPrivateToken.call({})),
    await settle(() => D.hasPrivateToken.call({}, "https://a.example")),
    await settle(() => D.hasRedemptionRecord.call(null, "https://a.example")),
    await settle(() => D.hasRedemptionRecord.call(document.body, "bad")),
  ];

  // A top-frame origin associates with at most two distinct issuers; its frames share them.
  out.tokens = [];
  for (const issuer of [
    "https://one.example",
    "https://ONE.example:443/x",
    "https://two.example",
    "https://one.example",
    "https://three.example",
    "https://two.example",
    "http://localhost:1",
  ])
    out.tokens.push([issuer, await settle(() => document.hasPrivateToken(issuer))]);
  out.tokens.push([
    "redemption",
    await settle(() => document.hasRedemptionRecord("https://three.example")),
  ]);

  // Documents without a browsing context have no top-frame origin.
  const created = document.implementation.createHTMLDocument("");
  out.detached = [
    await settle(() => created.hasPrivateToken("bad")),
    await settle(() => created.hasPrivateToken("https://two.example")),
    await settle(() => created.hasRedemptionRecord("https://two.example")),
    await settle(() => created.browsingTopics(5)),
    await settle(() => created.browsingTopics()),
  ];
  const frame = (allow) => {
    const element = document.createElement("iframe");
    if (allow !== undefined) element.setAttribute("allow", allow);
    document.body.append(element);
    return element;
  };
  const same = frame(),
    win = same.contentWindow,
    doc = same.contentDocument;
  out.frame = [
    await settle(() => doc.browsingTopics(), win),
    await settle(() => doc.hasPrivateToken("https://two.example"), win),
    await settle(() => doc.hasPrivateToken("https://four.example"), win),
    await settle(
      () => win.Document.prototype.hasPrivateToken.call(document, "https://one.example"),
      win,
    ),
    await settle(() => D.hasRedemptionRecord.call(doc, "https://one.example"), win),
  ];
  const denied = frame(
    "browsing-topics 'none'; private-state-token-redemption 'none'; private-state-token-issuance 'none'",
  );
  out.denied = [
    denied.contentDocument.featurePolicy.allowsFeature("browsing-topics"),
    denied.contentDocument.featurePolicy.allowsFeature("private-state-token-redemption"),
    await settle(() => denied.contentDocument.browsingTopics(), denied.contentWindow),
    await settle(
      () => denied.contentDocument.hasPrivateToken("https://two.example"),
      denied.contentWindow,
    ),
    await settle(
      () => denied.contentDocument.hasRedemptionRecord("https://two.example"),
      denied.contentWindow,
    ),
  ];
  const FramePrototype = win.Document.prototype;
  same.remove();
  out.removed = [
    await settle(() => doc.browsingTopics(), win),
    await settle(() => doc.hasPrivateToken("https://two.example"), win),
    await settle(() => doc.hasRedemptionRecord("https://two.example"), win),
    await settle(() => doc.hasPrivateToken("bad"), win),
    await settle(() => FramePrototype.hasPrivateToken.call(document, "bad"), win),
  ];
  return out;
})();
