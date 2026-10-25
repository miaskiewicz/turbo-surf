// Field-by-field comparison of Chrome identity battery fixtures. Two modes:
//
//   parity  — a Chrome fixture against a turbo-surf fixture of the same version.
//             Every difference is a tell unless it is environment-only, a
//             collector artifact, or a documented known gap.
//   version — an older Chrome fixture against a newer one. Version tokens are
//             normalized away; what remains is the release delta, each mapped to
//             the action the re-version helper takes (or the human it needs).
//
// Pure functions only; chrome-identity.test.mjs covers them offline.

/** Chrome's major-seeded GREASE brand list (mirror of fingerprint::brand_versions). */
export function brandVersions(major, version, full) {
  const characters = [" ", "(", ":", "-", ".", "/", ")", ";", "=", "?", "_"];
  const versions = ["8", "99", "24"];
  const orders = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
  ];
  const grease = versions[major % 3];
  const brands = [
    {
      brand: `Not${characters[major % 11]}A${characters[(major + 1) % 11]}Brand`,
      version: full ? `${grease}.0.0.0` : grease,
    },
    { brand: "Chromium", version },
    { brand: "Google Chrome", version },
  ];
  const output = [];
  orders[major % 6].forEach((destination, index) => {
    output[destination] = brands[index];
  });
  return output;
}

export const brandHeader = (major, version, full) =>
  brandVersions(major, version, full)
    .map(({ brand, version: value }) => `"${brand}";v="${value}"`)
    .join(", ");

/** The `{ major, full }` a fixture reports (Chrome product, else UA-CH). */
export function versionOf(fixture) {
  const product = /Chrome\/(\d+)\.(\d+\.\d+\.\d+)/.exec(fixture.browser?.product ?? "");
  const full =
    fixture.js?.userAgentData?.high?.uaFullVersion ??
    (product ? `${product[1]}.${product[2]}` : fixture.build?.reportedFullVersion);
  const major = Number(String(full).split(".")[0]);
  return { major, full };
}

/** Replace every spelling of the release version with placeholders. */
export function normalizeVersion(text, { major, full }) {
  if (typeof text !== "string") return text;
  return text
    .split(brandHeader(major, full, true))
    .join("{FULL_BRANDS}")
    .split(brandHeader(major, String(major), false))
    .join("{BRANDS}")
    .split(full)
    .join("{FULL}")
    .split(`Chrome/${major}.0.0.0`)
    .join("Chrome/{MAJOR}.0.0.0");
}

/** Replace brand lists that follow the GREASE algorithm with placeholders. */
export function normalizeBrands(value, { major, full }) {
  const expected = new Map([
    [JSON.stringify(brandVersions(major, String(major), false)), "{BRANDS}"],
    [JSON.stringify(brandVersions(major, full, true)), "{FULL_BRANDS}"],
  ]);
  const walk = (node) => {
    if (Array.isArray(node)) {
      const placeholder = expected.get(JSON.stringify(node));
      return placeholder ?? node.map(walk);
    }
    if (node && typeof node === "object")
      return Object.fromEntries(Object.entries(node).map(([key, item]) => [key, walk(item)]));
    return node;
  };
  return walk(value);
}

const HINTS = [
  "sec-ch-ua-arch",
  "sec-ch-ua-bitness",
  "sec-ch-ua-full-version",
  "sec-ch-ua-full-version-list",
  "sec-ch-ua-model",
  "sec-ch-ua-platform-version",
  "sec-ch-ua-wow64",
  "sec-ch-ua-form-factors",
];

/** Request class (mirror of browser::request_class in turbo-surf-core). */
export function requestClass(headers, method) {
  const value = (name) => headers.find(([key]) => key.toLowerCase() === name)?.[1] ?? undefined;
  const mode = value("sec-fetch-mode") ?? "";
  const destination = value("sec-fetch-dest") ?? "";
  let base;
  if (method === "OPTIONS" && value("access-control-request-method") !== undefined)
    base = "preflight";
  else if (mode === "navigate" || (!mode && value("upgrade-insecure-requests") !== undefined))
    base = "navigate";
  else if (mode === "no-cors") base = "no-cors";
  else if (["script", "style", "font"].includes(destination)) base = "cors-resource";
  else base = "cors";
  return HINTS.some((hint) => value(hint) !== undefined) ? `${base}-hinted` : base;
}

// Header names that are the page author's, not the browser's.
const AUTHOR_HEADER = /^x-/i;
const lower = (name) => name.toLowerCase();
const isPseudo = (name) => name.startsWith(":");

/**
 * Requests keyed `kind@origin#method#n` (n counts repeats, e.g. a Critical-CH
 * retry). The origin label keeps racy browser-UI requests (favicons) stable.
 */
export function requestIndex(requests, tiers) {
  const index = new Map();
  for (const request of requests) {
    if (tiers && !tiers.includes(request.tier) && request.kind !== "favicon") continue;
    let n = 0;
    let key;
    do key = `${request.kind}@${request.origin}#${request.method}#${n++}`;
    while (index.has(key));
    index.set(key, request);
  }
  return index;
}

// Header values that depend on the measuring machine or network, not on the
// browser build: Chrome's network-quality estimates and the window geometry.
export const ENVIRONMENT_HEADERS = new Set(["downlink", "rtt", "ect", "sec-ch-viewport-width"]);

// JS paths that describe the measuring machine, or that the collector itself
// changes (CDP remote debugging sets navigator.webdriver).
export const ENVIRONMENT_PATHS = [
  /^navigator\.hardwareConcurrency$/,
  /^screen\./,
  /^intl\.dateTime\.timeZone$/,
  /^navigator\.connectionType$/,
];
export const COLLECTOR_PATHS = [/^navigator\.webdriver$/];

function compareHeaders(left, right, context, normalize) {
  const deltas = [];
  const push = (type, extra) => deltas.push({ area: "request", type, ...context, ...extra });
  const names = (request) =>
    request.headers.map(([name]) => name).filter((name) => !isPseudo(name));
  const leftNames = names(left).map(lower);
  const rightNames = names(right).map(lower);
  const pseudo = (request) => request.headers.map(([name]) => name).filter(isPseudo);
  if (left.protocol !== right.protocol)
    push("protocol", { expected: left.protocol, actual: right.protocol });
  if (JSON.stringify(pseudo(left)) !== JSON.stringify(pseudo(right)))
    push("pseudo-order", { expected: pseudo(left), actual: pseudo(right) });
  const shared = (list, other) => list.filter((name) => other.includes(name));
  if (
    JSON.stringify(shared(leftNames, rightNames)) !== JSON.stringify(shared(rightNames, leftNames))
  )
    push("header-order", { expected: leftNames, actual: rightNames });
  const map = (request) =>
    new Map(
      request.headers.filter(([name]) => !isPseudo(name)).map(([n, v]) => [lower(n), [n, v]]),
    );
  const leftMap = map(left);
  const rightMap = map(right);
  for (const [name, [spelling, value]] of leftMap) {
    const other = rightMap.get(name);
    if (!other) {
      push("header-missing", { header: name, expected: value });
      continue;
    }
    if (left.protocol !== "h2" && spelling !== other[0])
      push("header-casing", { header: name, expected: spelling, actual: other[0] });
    if (normalize(value) !== normalize(other[1]))
      push("header-value", { header: name, expected: value, actual: other[1] });
  }
  for (const [name, [, value]] of rightMap)
    if (!leftMap.has(name)) push("header-extra", { header: name, actual: value });
  if (
    left.protocol === "h2" &&
    right.protocol === "h2" &&
    JSON.stringify(left.streamPriority) !== JSON.stringify(right.streamPriority)
  )
    push("stream-priority", { expected: left.streamPriority, actual: right.streamPriority });
  return deltas;
}

/** The first fresh (non-resumed) connection for an origin and ALPN. */
export function representativeConnection(connections, origin, alpn, ids) {
  return connections.find(
    (connection) =>
      connection.origin === origin &&
      (connection.alpn ?? "http/1.1") === alpn &&
      (!ids || ids.has(connection.id)) &&
      connection.hello &&
      !connection.hello.extensionOrder.includes(41),
  );
}

export const transportView = (connection) =>
  connection && {
    hello: connection.hello.normalized,
    ja4: connection.hello.ja4,
    ja3n: connection.hello.ja3nHash,
    alpn: connection.hello.alpn,
    applicationSettings: connection.hello.applicationSettings ?? null,
    certificateCompression: connection.hello.certificateCompression ?? null,
    groups: connection.hello.groupNames,
    keyShareGroups: connection.hello.keyShareGroups,
    signatureAlgorithms: connection.hello.signatureAlgorithms,
    trustAnchors: connection.hello.trustAnchors,
    h2: connection.h2 && {
      settings: connection.h2.settings,
      connectionWindowIncrement: connection.h2.connectionWindowIncrement,
      priorityFrames: connection.h2.priorityFrames,
      preamble: connection.h2.preamble,
      akamai: connection.h2.akamai,
    },
  };

/** Flatten nested JSON into `a.b.c` leaves; primitive arrays stay whole. */
export function flatten(value, prefix = "", out = {}) {
  const primitive = (item) => item === null || typeof item !== "object";
  if (Array.isArray(value) && value.every(primitive)) out[prefix] = value;
  else if (value && typeof value === "object")
    for (const [key, item] of Object.entries(value))
      flatten(item, prefix ? `${prefix}.${key}` : key, out);
  else out[prefix] = value;
  return out;
}

function compareTree(area, expected, actual, normalize = (value) => value) {
  const deltas = [];
  const left = flatten(expected ?? {});
  const right = flatten(actual ?? {});
  for (const path of new Set([...Object.keys(left), ...Object.keys(right)])) {
    const a = JSON.stringify(left[path], (_, value) => normalize(value));
    const b = JSON.stringify(right[path], (_, value) => normalize(value));
    if (a === b) continue;
    const delta = { area, type: "value", path, expected: left[path], actual: right[path] };
    if (Array.isArray(left[path]) && Array.isArray(right[path])) {
      delta.added = right[path].filter((item) => !left[path].includes(item));
      delta.removed = left[path].filter((item) => !right[path].includes(item));
      if (!delta.added.length && !delta.removed.length) delta.type = "order";
    }
    deltas.push(delta);
  }
  return deltas;
}

// A known-gap rule matches field values (`"/regex/"` strings are patterns). For
// list deltas, `allowAdded`/`allowRemoved` pin the exact tolerated members, so a
// new extra global is still a tell.
export const matches = (rule, delta) =>
  Object.entries(rule.match).every(([key, expected]) => {
    const value = delta[key];
    if (typeof expected === "string" && expected.startsWith("/") && expected.endsWith("/"))
      return value !== undefined && new RegExp(expected.slice(1, -1)).test(String(value));
    return value === expected;
  }) &&
  (!rule.allowAdded || (delta.added ?? []).every((item) => rule.allowAdded.includes(item))) &&
  (!rule.allowRemoved || (delta.removed ?? []).every((item) => rule.allowRemoved.includes(item)));

function classify(delta, knownGaps) {
  if (delta.area === "js") {
    if (ENVIRONMENT_PATHS.some((pattern) => pattern.test(delta.path))) return "environment";
    if (COLLECTOR_PATHS.some((pattern) => pattern.test(delta.path))) return "collector";
  }
  if (delta.area === "request" && ENVIRONMENT_HEADERS.has(delta.header)) return "environment";
  const gap = knownGaps.find((rule) => matches(rule, delta));
  if (gap) {
    delta.gap = gap.id;
    return "known-gap";
  }
  return "tell";
}

// The native tier that answers each battery request kind.
export const NOJS_KINDS = ["nav", "h1-nav", "http-nav", "ch-accept", "ch-next", "critical"];
export const RENDER_KINDS = [
  "nav",
  "style",
  "font-style",
  "script",
  "module",
  "img",
  "font",
  "iframe",
  "fetch",
  "fetch-post",
  "fetch-cors",
  "fetch-preflight",
  "xhr",
  "form-post",
  "click-nav",
  "ch-accept",
  "ch-sub",
  "favicon",
];

/** Chrome fixture vs turbo-surf fixture: every delta, classified. */
export function diffParity(chrome, native, { knownGaps = [] } = {}) {
  const deltas = [];
  const expected = requestIndex(chrome.requests);
  const nativeIds = new Set();
  for (const [tier, kinds] of [
    ["nojs", NOJS_KINDS],
    ["render", RENDER_KINDS],
  ]) {
    const actual = requestIndex(native.requests, [tier]);
    for (const [key, request] of expected) {
      if (!kinds.includes(request.kind)) continue;
      const counterpart = actual.get(key);
      const context = { tier, kind: request.kind, key };
      if (!counterpart) {
        deltas.push({ area: "request", type: "request-missing", ...context });
        continue;
      }
      nativeIds.add(counterpart.connection);
      deltas.push(...compareHeaders(request, counterpart, context, (value) => value));
    }
    for (const [key, request] of actual)
      if (!expected.has(key))
        deltas.push({ area: "request", type: "request-extra", tier, kind: request.kind, key });
  }
  for (const [origin, alpn] of [
    ["A", "h2"],
    ["B", "http/1.1"],
  ]) {
    const left = transportView(representativeConnection(chrome.connections, origin, alpn));
    const right = transportView(
      representativeConnection(native.connections, origin, alpn, nativeIds),
    );
    if (!left || !right) {
      deltas.push({ area: "transport", type: "connection-missing", origin, alpn, path: origin });
      continue;
    }
    for (const delta of compareTree("transport", left, right))
      deltas.push({ ...delta, origin, alpn });
  }
  for (const delta of compareTree("js", chrome.js, native.js)) deltas.push(delta);
  for (const delta of deltas) delta.class = classify(delta, knownGaps);
  return { deltas, summary: summarize(deltas) };
}

export function summarize(deltas) {
  const counts = {};
  for (const delta of deltas) counts[delta.class] = (counts[delta.class] ?? 0) + 1;
  return counts;
}

// What the re-version helper does with each kind of release delta.
export const ACTIONS = {
  registry: "update the registry entry from the measurement (automatic)",
  engine: "JS-engine/binding delta: recorded for the V8 engine step, never spoofed",
  human: "needs a human: the transport or request code cannot express it from data",
  info: "recorded only",
};

function actionFor(delta) {
  if (delta.area === "transport") return "human";
  if (delta.kind === "favicon" || ENVIRONMENT_HEADERS.has(delta.header)) return "info";
  if (
    delta.area === "js" &&
    [...ENVIRONMENT_PATHS, ...COLLECTOR_PATHS].some((pattern) => pattern.test(delta.path))
  )
    return "info";
  if (delta.area === "request") {
    if (delta.type === "header-order" || delta.type === "pseudo-order") return "registry";
    if (delta.type === "header-value" && ["accept", "priority"].includes(delta.header))
      return "registry";
    if (delta.type === "stream-priority") return "info";
    return "human";
  }
  if (delta.area === "js") {
    if (delta.path === "navigator.deviceMemory") return "registry";
    if (/^(engine|features\.builtins|features\.prototypes\.(?!Navigator)|intl\.)/.test(delta.path))
      return "engine";
    if (delta.path.startsWith("features.")) return "engine";
    if (/^(navigator|userAgentData)\./.test(delta.path)) return "human";
    return "info";
  }
  return "info";
}

/** Older Chrome fixture vs newer Chrome fixture: the release delta, with actions. */
export function diffVersions(previous, next) {
  const oldVersion = versionOf(previous);
  const newVersion = versionOf(next);
  const deltas = [];
  const errors = [];
  // The GREASE brand algorithm is code, not data: a mismatch needs a human.
  for (const fixture of [previous, next]) {
    const version = versionOf(fixture);
    const nav = (fixture.requests ?? []).find((request) => request.kind === "nav");
    const brands = nav?.headers.find(([name]) => lower(name) === "sec-ch-ua")?.[1];
    if (brands && brands !== brandHeader(version.major, String(version.major), false))
      errors.push(
        `sec-ch-ua for Chrome ${version.major} is ${brands}, not the brand algorithm's ${brandHeader(version.major, String(version.major), false)}`,
      );
  }
  const measured = (fixture) => new Set(fixture.measured ?? ["requests", "transport", "js"]);
  const both = (part) => measured(previous).has(part) && measured(next).has(part);
  if (both("requests")) {
    const left = requestIndex(previous.requests);
    const right = requestIndex(next.requests);
    for (const [key, request] of right) {
      const before = left.get(key);
      const context = { kind: request.kind, key };
      if (!before) {
        if (!previous.partial) deltas.push({ area: "request", type: "request-new", ...context });
        continue;
      }
      deltas.push(
        ...compareHeaders(before, request, context, (value) =>
          normalizeVersion(normalizeVersion(value, oldVersion), newVersion),
        ),
      );
    }
  }
  if (both("transport"))
    for (const [origin, alpn] of [
      ["A", "h2"],
      ["B", "http/1.1"],
    ]) {
      const left = transportView(representativeConnection(previous.connections, origin, alpn));
      const right = transportView(representativeConnection(next.connections, origin, alpn));
      if (left && right)
        for (const delta of compareTree("transport", left, right))
          deltas.push({ ...delta, origin, alpn });
    }
  if (both("js")) {
    const normalize = (value) => normalizeVersion(normalizeVersion(value, oldVersion), newVersion);
    deltas.push(
      ...compareTree(
        "js",
        normalizeBrands(previous.js, oldVersion),
        normalizeBrands(next.js, newVersion),
        normalize,
      ),
    );
  }
  // A partial (imported) baseline only constrains what it actually recorded.
  const kept = previous.partial
    ? deltas.filter((delta) => delta.expected !== undefined && delta.expected !== null)
    : deltas;
  for (const delta of kept) delta.action = actionFor(delta);
  return { from: oldVersion, to: newVersion, deltas: kept, errors };
}

/**
 * Merge measured header orders into one order consistent with every sample
 * (a topological merge). Returns `{ order }` or `{ conflict: [a, b] }`.
 */
export function mergeOrders(sequences) {
  const nodes = [];
  const after = new Map();
  const add = (name) => {
    if (!after.has(name)) {
      after.set(name, new Set());
      nodes.push(name);
    }
  };
  for (const sequence of sequences)
    for (let i = 0; i < sequence.length; i++) {
      add(sequence[i]);
      if (i) after.get(sequence[i - 1]).add(sequence[i]);
    }
  const incoming = new Map(nodes.map((name) => [name, 0]));
  for (const targets of after.values())
    for (const target of targets) incoming.set(target, incoming.get(target) + 1);
  const order = [];
  // Stable Kahn: always take the earliest-seen ready node.
  const ready = () => nodes.find((name) => !order.includes(name) && incoming.get(name) === 0);
  for (let next = ready(); next !== undefined; next = ready()) {
    order.push(next);
    for (const target of after.get(next)) incoming.set(target, incoming.get(target) - 1);
  }
  if (order.length !== nodes.length) {
    const stuck = nodes.filter((name) => !order.includes(name));
    return { conflict: stuck.slice(0, 2) };
  }
  return { order };
}

/** Chrome's H1 spelling for each header name seen on an HTTP/1.1 request. */
export function h1Spellings(requests) {
  const spellings = new Map();
  for (const request of requests)
    if (request.protocol === "http/1.1")
      for (const [name] of request.headers)
        if (name !== lower(name)) spellings.set(lower(name), name);
  return spellings;
}

/**
 * Registry entry measured from a Chrome fixture. `previous` supplies the fields
 * the battery does not measure (transport recipe) and the cookie slot (a cold
 * profile sends no cookie). Throws when a measurement cannot become data.
 */
export function measuredRelease(fixture, previous) {
  const { major, full } = versionOf(fixture);
  const product = fixture.browser?.product;
  if (product && product !== `Chrome/${full}`)
    throw new Error(`Browser.getVersion ${product} disagrees with uaFullVersion ${full}`);
  const requests = fixture.requests.filter((request) => request.tier === "chrome");
  const index = requestIndex(requests);
  const nav = index.get("nav@A#GET#0");
  if (!nav) throw new Error("The fixture has no cold top-level navigation (kind nav)");
  const value = (request, name) =>
    request?.headers.find(([key]) => lower(key) === name)?.[1] ?? null;
  const navOrder = nav.headers.map(([name]) => lower(name)).filter((name) => !isPseudo(name));
  const headerOrder = withCookieSlot(navOrder, previous?.headerOrder ?? []);
  const spellings = h1Spellings(requests);
  const classes = new Map();
  for (const [key, request] of index) {
    // A Critical-CH restart reorders headers; turbo-surf does not restart.
    if (key.startsWith("critical@") && !key.endsWith("#0")) continue;
    const names = request.headers
      .map(([name]) => lower(name))
      .filter((name) => !isPseudo(name) && !AUTHOR_HEADER.test(name));
    const cls = requestClass(request.headers, request.method);
    if (!classes.has(cls)) classes.set(cls, []);
    classes.get(cls).push(names);
  }
  const requestOrders = {};
  for (const [cls, sequences] of [...classes].sort(([a], [b]) => (a < b ? -1 : 1))) {
    const merged = mergeOrders(sequences);
    if (merged.conflict)
      throw new Error(
        `Chrome ${major} orders ${merged.conflict.join(" / ")} inconsistently within request class ${cls}; split the class before re-versioning`,
      );
    requestOrders[cls] = withCookieSlot(merged.order, headerOrder).map(
      (name) => spellings.get(name) ?? name,
    );
  }
  const accept = {};
  const priority = {};
  const cold = [...index.values()].filter(
    (request) => requestClass(request.headers, request.method).endsWith("-hinted") === false,
  );
  for (const request of cold) {
    const destination = value(request, "sec-fetch-dest");
    if (!destination || request.method === "OPTIONS" || request.kind === "favicon") continue;
    if (!["document", "iframe"].includes(destination))
      accept[destination] ??= value(request, "accept");
    const header = value(request, "priority");
    if (header) priority[destination] ??= header;
  }
  const sorted = (object) =>
    Object.fromEntries(Object.entries(object).sort(([a], [b]) => (a < b ? -1 : 1)));
  return {
    major,
    fullVersion: full,
    headerOrder,
    navigationAccept: value(nav, "accept"),
    defaultDeviceMemory: fixture.js?.navigator?.deviceMemory ?? previous?.defaultDeviceMemory,
    transportBaseMajor: previous?.transportBaseMajor,
    requestOrders,
    accept: sorted(accept),
    priority: sorted(priority),
  };
}

/** Keep the registry's `cookie` slot (cold captures carry no cookie). */
export function withCookieSlot(order, previous) {
  if (order.includes("cookie")) return order;
  const at = previous.indexOf("cookie");
  if (at < 0) return [...order, "cookie"];
  // Re-anchor after the nearest preceding header the new order still has.
  for (let i = at - 1; i >= 0; i--) {
    const anchor = order.indexOf(previous[i]);
    if (anchor >= 0) return [...order.slice(0, anchor + 1), "cookie", ...order.slice(anchor + 1)];
  }
  return ["cookie", ...order];
}
