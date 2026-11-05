// The identity battery's wss: TLS row (turbo-surf#179). A WebSocket handshake rides
// HTTP/1.1, so Chrome opens a fresh TLS connection whose ClientHello differs from its
// https one only where the WebSocket stack asks it to (ALPN). This records Chrome's
// https and wss ClientHellos from one frozen binary, measures turbo-surf's two the same
// way, and diffs them with the battery's transport view and known-gap rules.
//
//   node harness/detection/wss-hello.mjs --chrome <executable> --out fixtures/wss-hello-chrome<major>.json
//   node harness/detection/wss-hello.mjs --native <wss_hello example> [--control <fixture>]
//
// Only 127.0.0.1 listeners that close after the ClientHello; no certificate exchange,
// no HTTP, no remote traffic. The fresh Chrome profile is deleted afterwards.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { classify, summarize } from "./identity-diff.mjs";
import { describeClientHello } from "./identity-wire.mjs";
import { captureClientHello } from "./transport-hello.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
export const WSS_SCHEMA = 1;
export const KNOWN_GAPS = join(here, "chrome-identity-known-gaps.json");
export const CHROME_FLAGS = [
  "--remote-debugging-pipe",
  "--user-data-dir=<fresh-profile>",
  "--no-first-run",
  "--no-default-browser-check",
  "--use-mock-keychain",
  "--password-store=basic",
  "--disable-extensions",
  "--disable-component-extensions-with-background-pages",
  "about:blank",
];

// What each side's page does to open the connection; the URL is the capture listener.
export const CHROME_OPENERS = {
  https: (url) => ({ method: "Page.navigate", params: { url } }),
  wss: (url) => ({
    method: "Runtime.evaluate",
    params: { expression: `new WebSocket(${JSON.stringify(url.replace(/^https:/, "wss:"))}), 0` },
  }),
};

/** One captured ClientHello, described with the battery's wire parser. */
export async function captureHello(start) {
  const out = mkdtempSync(join(tmpdir(), "turbo-wss-hello-"));
  try {
    await captureClientHello(start, { out, label: "native" });
    const record = readFileSync(join(out, "native-client-hello.bin"));
    return { ...describeClientHello(record), rawBase64: record.toString("base64") };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

/** The fields a server can read from one ClientHello (the battery's transport view). */
export const helloView = (hello) => ({
  hello: hello.normalized,
  ja4: hello.ja4,
  ja3n: hello.ja3nHash,
  alpn: hello.alpn,
  applicationSettings: hello.applicationSettings ?? null,
  certificateCompression: hello.certificateCompression ?? null,
  groups: hello.groupNames,
  keyShareGroups: hello.keyShareGroups,
  signatureAlgorithms: hello.signatureAlgorithms,
  trustAnchors: hello.trustAnchors,
});

// Minimal CDP over --remote-debugging-pipe (NUL-delimited JSON on fds 3/4).
function pipeSession(chrome) {
  const pending = new Map();
  let nextId = 1;
  let buffer = Buffer.alloc(0);
  chrome.stdio[4].on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (let end = buffer.indexOf(0); end >= 0; end = buffer.indexOf(0)) {
      const message = JSON.parse(buffer.subarray(0, end).toString("utf8"));
      buffer = buffer.subarray(end + 1);
      const waiter = message.id && pending.get(message.id);
      if (!waiter) continue;
      pending.delete(message.id);
      if (message.error) waiter.reject(Error(JSON.stringify(message.error)));
      else waiter.resolve(message.result);
    }
  });
  return (method, params = {}, sessionId) =>
    new Promise((done, fail) => {
      const id = nextId++;
      pending.set(id, { resolve: done, reject: fail });
      chrome.stdio[3].write(
        `${JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })}\0`,
      );
    });
}

/** Record Chrome's https and wss ClientHellos from one fresh-profile run. */
export async function recordChrome(executable) {
  const profile = mkdtempSync(join(tmpdir(), "turbo-wss-chrome-"));
  const chrome = spawn(
    executable,
    CHROME_FLAGS.map((flag) =>
      flag === "--user-data-dir=<fresh-profile>" ? `--user-data-dir=${profile}` : flag,
    ),
    { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
  );
  const call = pipeSession(chrome);
  try {
    const version = await call("Browser.getVersion");
    const hellos = {};
    for (const [mode, opener] of Object.entries(CHROME_OPENERS)) {
      // A fresh target per mode, so no connection or session is reused between them.
      const { targetId } = await call("Target.createTarget", { url: "about:blank" });
      const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
      hellos[mode] = await captureHello(async (url) => {
        const { method, params } = opener(url);
        call(method, params, sessionId).catch(() => {});
        return () => call("Target.closeTarget", { targetId }).catch(() => {});
      });
    }
    return {
      schemaVersion: WSS_SCHEMA,
      browser: { product: version.product, userAgent: version.userAgent },
      executableSha256: sha256(readFileSync(executable)),
      collector: sha256(readFileSync(fileURLToPath(import.meta.url))),
      flags: CHROME_FLAGS,
      note: "Loopback ClientHello only (127.0.0.1 listener closes after the record). https = top-level navigation, wss = new WebSocket() from about:blank. Raw records keep randoms and key shares; views drop GREASE and the ECH payload length.",
      raw: Object.fromEntries(
        Object.entries(hellos).map(([mode, hello]) => [mode, hello.rawBase64]),
      ),
      views: Object.fromEntries(
        Object.entries(hellos).map(([mode, hello]) => [mode, helloView(hello)]),
      ),
    };
  } finally {
    const exited = new Promise((done) => chrome.once("exit", done));
    chrome.kill();
    await exited;
    rmSync(profile, { recursive: true, force: true, maxRetries: 10 });
  }
}

/** Measure turbo-surf's https and wss ClientHellos with the wss_hello example. */
export async function measureNative(binary) {
  const views = {};
  for (const mode of ["https", "wss"]) {
    const hello = await captureHello(async (url) => {
      const child = spawn(binary, [mode === "wss" ? url.replace(/^https:/, "wss:") : url], {
        stdio: "ignore",
      });
      return () =>
        new Promise((done) => {
          if (child.exitCode !== null) done();
          else {
            child.kill();
            child.once("close", done);
          }
        });
    });
    views[mode] = helloView(hello);
  }
  return { views };
}

/**
 * One view as comparable facts: every scalar field, the cipher list, and each extension
 * by type, so a removed extension is one fact instead of shifted list indexes. JA4 keeps
 * its version, ALPN and cipher parts; its extension count and hash follow the facts.
 */
export function helloFacts(view) {
  const facts = {
    ciphers: view.hello.ciphers,
    cipherGrease: view.hello.cipherGrease,
    extensionGreaseCount: view.hello.extensionGreaseCount,
    extensionTypes: view.hello.extensions.map((entry) => entry.type),
    "ja4.version": view.ja4.slice(0, 4),
    "ja4.alpn": view.ja4.split("_")[0].slice(-2),
    "ja4.ciphers": view.ja4.split("_")[1],
  };
  for (const key of [
    "alpn",
    "applicationSettings",
    "certificateCompression",
    "groups",
    "keyShareGroups",
    "signatureAlgorithms",
    "trustAnchors",
  ])
    facts[key] = view[key];
  for (const entry of view.hello.extensions) facts[`extension.${entry.type}`] = entry;
  return facts;
}

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const factDeltas = (expected, actual) =>
  [...new Set([...Object.keys(expected), ...Object.keys(actual)])]
    .filter((key) => !same(expected[key], actual[key]))
    .map((key) => ({ path: key, expected: expected[key] ?? null, actual: actual[key] ?? null }));

// Chrome's own https→wss change applied to turbo-surf's https facts. extensionTypes is
// a sorted list, so it is rebuilt from the per-type extension facts.
function expectedWss(chrome, nativeHttps) {
  const changed = new Set(factDeltas(chrome.https, chrome.wss).map((delta) => delta.path));
  const expected = {};
  for (const key of new Set([...Object.keys(nativeHttps), ...changed]))
    if (key !== "extensionTypes")
      expected[key] = (changed.has(key) ? chrome.wss : nativeHttps)[key];
  expected.extensionTypes = Object.keys(expected)
    .filter((key) => key.startsWith("extension.") && expected[key] !== undefined)
    .map((key) => Number(key.slice("extension.".length)))
    .sort((a, b) => a - b);
  return expected;
}

/**
 * The wss row. `ws-delta`: turbo-surf's wss hello must differ from its https hello
 * exactly where Chrome's does (always a tell). `chrome`: turbo-surf wss against Chrome
 * wss directly; a delta the https pair shows identically is the oracle's https
 * difference (measured by the page battery against the identity fixture), the rest
 * is classified with the battery's known-gap rules.
 */
export function diffWss(control, native, { knownGaps = [] } = {}) {
  const chrome = { https: helloFacts(control.views.https), wss: helloFacts(control.views.wss) };
  const ours = { https: helloFacts(native.views.https), wss: helloFacts(native.views.wss) };
  const context = { area: "transport", origin: "W", alpn: control.views.wss.alpn.join(",") };
  const deltas = factDeltas(expectedWss(chrome, ours.https), ours.wss).map((delta) => ({
    ...context,
    type: "ws-delta",
    ...delta,
    class: "tell",
  }));
  const httpsDeltas = factDeltas(chrome.https, ours.https);
  for (const delta of factDeltas(chrome.wss, ours.wss)) {
    const shared = httpsDeltas.some((other) => same(other, delta));
    const row = { ...context, type: "value", ...delta };
    row.class = shared ? "oracle-https" : classify(row, knownGaps);
    deltas.push(row);
  }
  return { deltas, summary: summarize(deltas) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  if (flag("--chrome") && flag("--out")) {
    const fixture = await recordChrome(resolve(flag("--chrome")));
    writeFileSync(resolve(flag("--out")), `${JSON.stringify(fixture, null, 2)}\n`);
    console.log(JSON.stringify({ out: resolve(flag("--out")), browser: fixture.browser.product }));
  } else if (flag("--native")) {
    const control = JSON.parse(
      readFileSync(resolve(flag("--control") ?? join(here, "fixtures/wss-hello-chrome156.json"))),
    );
    const native = await measureNative(resolve(flag("--native")));
    const result = diffWss(control, native, {
      knownGaps: JSON.parse(readFileSync(KNOWN_GAPS, "utf8")),
    });
    for (const delta of result.deltas.filter((entry) => entry.class === "tell"))
      console.log(`[tell] ${delta.origin} ${delta.path}: ${JSON.stringify(delta)}`);
    console.log(JSON.stringify(result.summary));
    process.exitCode = result.summary.tell ? 12 : 0;
  } else {
    console.error(
      "Usage: node wss-hello.mjs (--chrome <executable> --out <fixture> | --native <wss_hello> [--control <fixture>])",
    );
    process.exitCode = 2;
  }
}
