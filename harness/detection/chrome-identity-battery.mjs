// Chrome identity battery: one localhost run that records everything a server or
// page script can use to tell which browser (and which Chrome version) is
// talking. See CHROME-REVERSION.md for what each section measures.
//
//   node harness/detection/chrome-identity-battery.mjs --chrome <executable> --out <file>
//   node harness/detection/chrome-identity-battery.mjs --native <identity_battery> --out <file>
//
// --chrome drives a (frozen) Chrome over CDP with a fresh profile; --native runs
// turbo-surf's no-JS fetch tier and JS render tier through the identity_battery
// example. Both write the same fixture shape, which identity-diff.mjs compares.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { release, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
import { startIdentityServers } from "./identity-server.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
export const BATTERY_SCHEMA = 1;
export const PROBE = join(here, "chrome-identity-probe.js");
export const COLLECTOR_FILES = [
  "chrome-identity-battery.mjs",
  "chrome-identity-probe.js",
  "identity-server.mjs",
  "identity-wire.mjs",
];
const sha256 = (data) => createHash("sha256").update(data).digest("hex");
export const collectorHashes = () =>
  Object.fromEntries(COLLECTOR_FILES.map((name) => [name, sha256(readFileSync(join(here, name)))]));

// The client-hint opt-in a high-entropy fingerprinting site sends.
export const ACCEPT_CH = [
  "Sec-CH-UA-Arch",
  "Sec-CH-UA-Bitness",
  "Sec-CH-UA-Full-Version",
  "Sec-CH-UA-Full-Version-List",
  "Sec-CH-UA-Model",
  "Sec-CH-UA-Platform-Version",
  "Sec-CH-UA-WoW64",
  "Sec-CH-UA-Form-Factors",
  "Sec-CH-Prefers-Color-Scheme",
  "Sec-CH-Prefers-Reduced-Motion",
  "Device-Memory",
  "Sec-CH-Device-Memory",
  "Downlink",
  "RTT",
  "ECT",
  "Sec-CH-DPR",
  "Sec-CH-Viewport-Width",
].join(", ");

// Request kinds the battery page issues as subresources of the cold navigation.
export const PAGE_KINDS = [
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
];

const html = (body, head = "") =>
  `<!doctype html><html><head><meta charset="utf-8">${head}</head><body>${body}</body></html>`;
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

function batteryPage(tier, origins) {
  const base = `/t/${tier}/k`;
  const cross = `${origins.AX}${base}`;
  return html(
    `<button id="nav-link" style="display:block;width:300px;height:80px" onclick="location.assign('${base}/click-nav')">next</button>
<img src="${base}/img" alt="" width="1" height="1">
<iframe name="frame" src="${base}/iframe"></iframe>
<iframe name="formframe"></iframe>
<form id="form" method="post" action="${base}/form-post" target="formframe"><input name="q" value="battery"></form>
<script>
(async () => {
  const step = async (callback) => { try { await callback(); } catch (error) { (window[Symbol.for("batteryErrors")] ||= []).push(String(error)); } };
  await step(() => fetch("${base}/fetch").then((r) => r.text()));
  await step(() => fetch("${base}/fetch-post", { method: "POST", body: "a=1", headers: { "content-type": "application/x-www-form-urlencoded" } }).then((r) => r.text()));
  await step(() => fetch("${cross}/fetch-cors").then((r) => r.text()));
  await step(() => fetch("${cross}/fetch-preflight", { method: "PUT", body: "{}", headers: { "content-type": "application/json", "x-battery": "1" } }).then((r) => r.text()));
  await step(() => new Promise((done) => { const x = new XMLHttpRequest(); x.open("GET", "${base}/xhr"); x.onloadend = done; x.send(); }));
  await step(() => document.getElementById("form").submit());
  if (document.readyState !== "complete") await new Promise((done) => addEventListener("load", done));
  window[Symbol.for("batteryDone")] = true;
})();
</script>`,
    `<link rel="stylesheet" href="${base}/style"><script src="${base}/script"></script><script type="module" src="${base}/module"></script>`,
  );
}

/** Response for one logged battery request (pure; ports come from the servers). */
export function batteryRoute(entry, origins) {
  const tier = entry.tier ?? "none";
  const text = (body, type = "text/plain") => ({
    headers: { "content-type": type, "cache-control": "no-store" },
    body,
  });
  const cors = (reply) => {
    const origin = entry.headers.find(([name]) => name.toLowerCase() === "origin")?.[1];
    reply.headers["access-control-allow-origin"] = origin || "*";
    reply.headers["access-control-allow-methods"] = "GET, PUT";
    reply.headers["access-control-allow-headers"] = "content-type, x-battery";
    return reply;
  };
  switch (entry.kind) {
    case "nav": {
      // A first-party cookie makes every later request on A measure the cookie slot.
      const reply = text(batteryPage(tier, origins), "text/html");
      reply.headers["set-cookie"] = "battery=1; Path=/; Secure; SameSite=Lax";
      return reply;
    }
    case "style":
      return text("body{margin:8px}", "text/css");
    case "font-style":
      return text(
        `@font-face{font-family:Battery;src:url("/t/${tier}/k/font") format("woff2")}`,
        "text/css",
      );
    case "iframe":
      // The web font lives in the frame so the top document's layout (and the
      // trusted click on it) does not depend on font-face support.
      return text(
        html(
          '<p style="font-family:Battery">font</p>',
          `<link rel="stylesheet" href="/t/${tier}/k/font-style">`,
        ),
        "text/html",
      );
    case "script":
    case "module":
      return text('globalThis[Symbol.for("batteryScript")] = true;', "text/javascript");
    case "img":
      return text(PNG, "image/png");
    case "font":
      return text(Buffer.alloc(16), "font/woff2");
    case "fetch-cors":
    case "fetch-preflight":
      return cors(text("ok"));
    case "ch-accept":
      return {
        headers: {
          "content-type": "text/html",
          "cache-control": "no-store",
          "accept-ch": ACCEPT_CH,
        },
        body: html(
          `<script>fetch("/t/${tier}/k/ch-sub").then((r) => r.text()).finally(() => { window[Symbol.for("batteryDone")] = true; });</script>`,
        ),
      };
    case "critical":
      return {
        headers: {
          "content-type": "text/html",
          "cache-control": "no-store",
          "accept-ch": "Sec-CH-UA-Full-Version-List, Sec-CH-UA-Arch",
          "critical-ch": "Sec-CH-UA-Full-Version-List",
        },
        body: html("critical"),
      };
    default:
      return entry.kind ? text(html(entry.kind), "text/html") : { status: 404, body: "" };
  }
}

/** Replace live ports and the tier prefix so two runs compare structurally. */
export function labelize(value, origins) {
  let text = String(value);
  for (const [label, origin] of Object.entries(origins).sort((a, b) => b[1].length - a[1].length))
    text = text.split(origin).join(`<${label}>`);
  for (const [label, origin] of Object.entries(origins)) {
    const host = new URL(origin).host;
    text = text.split(host).join(`<${label}-host>`);
  }
  // Referers of tier-less requests (favicons) still carry a tier prefix.
  return text.replace(/\/t\/[a-z0-9-]+\/k\//g, "/t/~/k/");
}

/** Fixture form of one logged request: labels instead of ports, per-tier paths. */
export function normalizeRequest(entry, origins) {
  return {
    tier: entry.tier,
    kind: entry.kind ?? (entry.path === "/favicon.ico" ? "favicon" : "other"),
    method: entry.method,
    path: labelize(entry.path, origins),
    origin: entry.origin,
    protocol: entry.protocol,
    headers: entry.headers.map(([name, value]) => [name, labelize(value, origins)]),
    bodyBytes: entry.bodyBytes,
    connection: entry.connection,
    streamPriority: entry.streamPriority ?? null,
  };
}

const pause = (ms) => new Promise((done) => setTimeout(done, ms));
// Page state lives under registered symbols so it never shows up in the probe's
// Object.getOwnPropertyNames(globalThis) inventory.
export const DONE = 'window[Symbol.for("batteryDone")] === true';
export const ERRORS = 'window[Symbol.for("batteryErrors")] || []';

async function stopProcess(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
  await Promise.race([new Promise((done) => child.once("close", done)), pause(2000)]);
  if (child.exitCode === null && child.signalCode === null) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {}
  }
}

const has =
  (kind, tier, count = 1) =>
  (requests) =>
    requests.filter((entry) => entry.kind === kind && entry.tier === tier).length >= count;

/** Drive one Chrome executable through the battery. */
export async function runChrome(executable, servers) {
  const { origins } = servers;
  const tier = "chrome";
  const profile = mkdtempSync(join(tmpdir(), "turbo-identity-chrome-"));
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    `--ignore-certificate-errors-spki-list=${servers.certificate.spki}`,
    ...chromeExtensionArgs({ url: origins.A, disableExtensions: true }),
    "about:blank",
  ];
  const chrome = spawn(executable, flags, {
    detached: true,
    stdio: ["ignore", "ignore", "pipe"],
  });
  const errors = [];
  const missing = [];
  let stderr = "";
  let socket;
  let connection;
  chrome.stderr.on("data", (chunk) => {
    if (stderr.length < 1048576) stderr += String(chunk);
  });
  try {
    const deadline = Date.now() + 30000;
    let endpoint;
    while (!(endpoint = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr)?.[1])) {
      if (chrome.exitCode !== null || Date.now() > deadline)
        throw new Error("Chrome startup failed");
      await pause(50);
    }
    socket = new WebSocket(endpoint);
    await new Promise((done, fail) => {
      socket.onopen = done;
      socket.onerror = fail;
    });
    connection = cdpConnection(
      socket,
      (event) => {
        if (event.method === "Runtime.exceptionThrown")
          errors.push(event.params.exceptionDetails?.exception?.description ?? "exception");
      },
      30000,
    );
    const version = await connection.call("Browser.getVersion");
    const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await connection.call("Target.attachToTarget", {
      targetId,
      flatten: true,
    });
    const call = (method, params = {}) => connection.call(method, params, sessionId);
    await call("Runtime.enable");
    await call("Page.enable");
    const evaluate = async (expression) => {
      const result = await call("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
      });
      if (result.exceptionDetails)
        throw new Error(result.exceptionDetails.exception?.description ?? "evaluation failed");
      return result.result.value;
    };
    const waitDone = async (timeoutMs = 20000) => {
      const end = Date.now() + timeoutMs;
      while (Date.now() < end) {
        if ((await evaluate(DONE).catch(() => false)) === true) return;
        await pause(100);
      }
      throw new Error("battery page did not finish");
    };
    const expect = async (kind, count = 1, timeoutMs = 10000) => {
      try {
        await servers.waitFor(has(kind, tier, count), timeoutMs, kind);
      } catch {
        missing.push(kind);
      }
    };
    await call("Page.navigate", { url: `${origins.A}/t/${tier}/k/nav` });
    await waitDone();
    for (const kind of PAGE_KINDS) await expect(kind, 1, 5000);
    const js = await evaluate(readFileSync(PROBE, "utf8"));
    const pageErrors = await evaluate(ERRORS);
    // Trusted input: a real CDP mouse click on the link (user activation).
    const box = await evaluate(
      "(() => { const r = document.getElementById('nav-link').getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()",
    );
    for (const type of ["mouseMoved", "mousePressed", "mouseReleased"])
      await call("Input.dispatchMouseEvent", {
        type,
        x: box.x,
        y: box.y,
        button: type === "mouseMoved" ? "none" : "left",
        clickCount: type === "mouseMoved" ? 0 : 1,
      });
    await expect("click-nav");
    const navigate = async (url, kind, count = 1) => {
      await call("Page.navigate", { url });
      await expect(kind, count);
    };
    await navigate(`${origins.B}/t/${tier}/k/h1-nav`, "h1-nav");
    await navigate(`${origins.D}/t/${tier}/k/http-nav`, "http-nav");
    await navigate(`${origins.C}/t/${tier}/k/ch-accept`, "ch-accept");
    await waitDone(10000).catch(() => missing.push("ch-accept-done"));
    await expect("ch-sub");
    await navigate(`${origins.C}/t/${tier}/k/ch-next`, "ch-next");
    await navigate(`${origins.CX}/t/${tier}/k/critical`, "critical", 2);
    await pause(500);
    await connection.call("Target.closeTarget", { targetId });
    return {
      browser: {
        product: version.product,
        v8: version.jsVersion,
        revision: version.revision,
        userAgent: version.userAgent,
        executableSha256: sha256(readFileSync(executable)),
      },
      flags: flags.map((flag) =>
        flag.startsWith("--user-data-dir=")
          ? "--user-data-dir=<fresh-profile>"
          : flag.startsWith("--ignore-certificate-errors-spki-list=")
            ? "--ignore-certificate-errors-spki-list=<ephemeral-loopback-spki>"
            : flag,
      ),
      js,
      pageErrors,
      errors,
      missing,
      tiers: [tier],
    };
  } finally {
    connection?.close();
    socket?.close();
    await stopProcess(chrome);
    rmSync(profile, { recursive: true, force: true });
  }
}

/** Run turbo-surf's identity_battery example (no-JS tier then render tier). */
export async function runNative(binary, servers) {
  const { origins } = servers;
  const scratch = mkdtempSync(join(tmpdir(), "turbo-identity-native-"));
  try {
    const certificate = join(scratch, "cert.der");
    writeFileSync(certificate, servers.certificate.der);
    const config = {
      certificate,
      probe: PROBE,
      done: DONE,
      errors: ERRORS,
      nojs: [
        `${origins.A}/t/nojs/k/nav`,
        `${origins.B}/t/nojs/k/h1-nav`,
        `${origins.D}/t/nojs/k/http-nav`,
        `${origins.C}/t/nojs/k/ch-accept`,
        `${origins.C}/t/nojs/k/ch-next`,
        `${origins.CX}/t/nojs/k/critical`,
      ],
      render: {
        page: `${origins.A}/t/render/k/nav`,
        hints: `${origins.C}/t/render/k/ch-accept`,
        waitMs: 8000,
      },
    };
    const configPath = join(scratch, "config.json");
    writeFileSync(configPath, JSON.stringify(config));
    const child = spawn(binary, [configPath], { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      if (stderr.length < 1048576) stderr += chunk;
    });
    const code = await new Promise((done) => child.on("close", done));
    if (code !== 0) throw new Error(`identity_battery exited ${code}: ${stderr.slice(-4000)}`);
    const report = JSON.parse(stdout.trim().split("\n").at(-1));
    return {
      build: report.build,
      js: report.render?.js ?? null,
      pageErrors: report.render?.pageErrors ?? [],
      errors: report.errors ?? [],
      missing: [],
      tiers: ["nojs", "render"],
      binarySha256: sha256(readFileSync(binary)),
      details: {
        nojs: report.nojs,
        render: report.render && { ...report.render, js: undefined, pageErrors: undefined },
      },
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

/** Run the battery against Chrome or turbo-surf and return the fixture object. */
export async function collect({ chrome, native }) {
  const servers = await startIdentityServers((entry) => batteryRoute(entry, servers.origins));
  try {
    const run = await (chrome ? runChrome(chrome, servers) : runNative(native, servers)).catch(
      (error) => {
        const seen = servers.requests.map((entry) => `${entry.tier}/${entry.kind}`).join(" ");
        const tls = servers.connections.map(
          (entry) => `${entry.origin}:${entry.alpn ?? entry.tlsError ?? "?"}`,
        );
        throw new Error(
          `${error.message}; requests seen: ${seen || "none"}; connections: ${tls.join(" ") || "none"}`,
          { cause: error },
        );
      },
    );
    const { origins } = servers;
    const requests = servers.requests
      .filter((entry) => run.tiers.includes(entry.tier) || entry.path === "/favicon.ico")
      .map((entry) => normalizeRequest(entry, origins));
    const used = new Set(requests.map((entry) => entry.connection));
    const connections = servers
      .connectionRecords()
      .filter((record) => used.has(record.id))
      .map(({ raw, ...record }) => ({
        ...record,
        hello: record.hello && { ...record.hello, rawBase64: raw },
      }));
    return {
      schemaVersion: BATTERY_SCHEMA,
      subject: chrome ? "chrome" : "turbo-surf",
      collectedAt: new Date().toISOString(),
      ...(run.browser ? { browser: run.browser } : {}),
      ...(run.build
        ? { build: run.build, binarySha256: run.binarySha256, nativeRun: run.details }
        : {}),
      ...(run.flags ? { flags: run.flags } : {}),
      collector: collectorHashes(),
      environment: { platform: process.platform, arch: process.arch, osRelease: release() },
      note: "Authored loopback battery only (127.0.0.1/localhost/::1). Fresh profile, no cookies, no remote traffic. ClientHello randoms and key shares are ephemeral.",
      missing: run.missing,
      errors: run.errors,
      pageErrors: run.pageErrors,
      requests,
      connections,
      js: run.js,
    };
  } finally {
    await servers.close();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const chrome = flag("--chrome");
  const native = flag("--native");
  const out = flag("--out");
  if (!(chrome || native) || (chrome && native) || !out) {
    console.error(
      "Usage: node chrome-identity-battery.mjs (--chrome <executable> | --native <identity_battery>) --out <file.json>",
    );
    process.exit(2);
  }
  const fixture = await collect({
    chrome: chrome && resolve(chrome),
    native: native && resolve(native),
  });
  writeFileSync(resolve(out), `${JSON.stringify(fixture, null, 2)}\n`);
  console.log(
    JSON.stringify({
      out: resolve(out),
      subject: fixture.subject,
      browser: fixture.browser?.product,
      requests: fixture.requests.length,
      connections: fixture.connections.length,
      missing: fixture.missing,
    }),
  );
}
