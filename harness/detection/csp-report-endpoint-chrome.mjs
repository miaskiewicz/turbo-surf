// Observe authored HTTP reports in normal installed Chrome, without reporting flags.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createHttpServer } from "node:http";
import { createServer as createHttpsServer } from "node:https";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
const [casesArgument, probeArgument, outputArgument] = process.argv.slice(2);
if (!casesArgument || !probeArgument || !outputArgument) throw Error("Usage: cases probe output");
const casesText = readFileSync(resolve(casesArgument), "utf8"),
  probe = readFileSync(resolve(probeArgument), "utf8"),
  cases = JSON.parse(casesText),
  output = resolve(outputArgument);
const profile = mkdtempSync(join(tmpdir(), "turbo-csp-report-")),
  reports = [],
  closed = [],
  requests = [],
  reportingEvents = [];
const tls = process.env.TURBO_CSP_REPORT_TLS === "1";
const scheme = tls ? "https" : "http";
let server, chrome, socket, connection;
const createServer = tls
  ? (listener) =>
      createHttpsServer(
        {
          key: readFileSync(process.env.TURBO_CSP_REPORT_KEY),
          cert: readFileSync(process.env.TURBO_CSP_REPORT_CERT),
        },
        listener,
      )
  : createHttpServer;
// Chrome's legacy report encoder retains lone UTF16 units as WTF8 bytes.
const decodeReport = (bytes) =>
  bytes.toString("latin1").replace(/[\xed][\xa0-\xbf][\x80-\xbf]/g, (part) => {
    const unit =
      ((part.charCodeAt(0) & 15) << 12) |
      ((part.charCodeAt(1) & 63) << 6) |
      (part.charCodeAt(2) & 63);
    return "\\u" + unit.toString(16).padStart(4, "0");
  });
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const expand = (value, id) =>
  value
    ?.replaceAll("{id}", String(id))
    .replaceAll("{port}", String(server.address().port))
    .replaceAll("http://", scheme + "://");
const escaped = (value) => value.replaceAll("&", "&amp;").replaceAll('"', "&quot;");
server = createServer(async (request, response) => {
  const target = new URL(request.url, "http://localhost"),
    id = Number(target.searchParams.get("case") || 0),
    item = cases[id];
  if (
    [
      "/report",
      "/fallback",
      "/hold-report",
      "/redirect302",
      "/redirect307",
      "/redirect-target",
    ].includes(target.pathname)
  ) {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    reports.push({
      case: id,
      url: request.url,
      method: request.method,
      headers: request.headers,
      rawBodyBase64: body.toString("base64"),
      body: body.length
        ? JSON.parse(Buffer.from(decodeReport(body), "latin1").toString("utf8"))
        : null,
    });
    if (request.method === "OPTIONS") {
      response.writeHead(204, {
        "Access-Control-Allow-Origin": "*",
        "Access-Control-Allow-Methods": "POST",
        "Access-Control-Allow-Headers": "content-type",
      });
      response.end();
      return;
    }
    if (target.pathname === "/redirect302" || target.pathname === "/redirect307") {
      response.writeHead(Number(target.pathname.slice(9)), {
        Location: "/redirect-target?case=" + id,
      });
      response.end();
      return;
    }
    if (target.pathname === "/hold-report") {
      response.on("close", () => closed.push({ case: id, canceled: !response.writableFinished }));
      return;
    }
    response.writeHead(204, {
      "Access-Control-Allow-Origin": "*",
      "Set-Cookie": "report_response=" + id + "; Path=/; SameSite=Lax",
    });
    response.end();
    return;
  }
  requests.push({ url: request.url, referrer: request.headers.referer || "" });
  if (target.pathname === "/start") {
    response.writeHead(200, { "Content-Type": "text/html", "Referrer-Policy": "unsafe-url" });
    response.end("<!doctype html><body>owner</body>");
    return;
  }
  const headers = {
    "Content-Type": "text/html",
    "Cache-Control": "no-store",
    "Referrer-Policy": item.policy || "strict-origin-when-cross-origin",
  };
  if (item.csp) headers["Content-Security-Policy"] = expand(item.csp, id);
  if (item.reportOnly) headers["Content-Security-Policy-Report-Only"] = expand(item.reportOnly, id);
  if (item.endpoints) headers["Reporting-Endpoints"] = expand(item.endpoints, id);
  if (item.reportTo)
    headers["Report-To"] = JSON.stringify({
      group: "g" + id,
      max_age: 3600,
      endpoints: [{ url: scheme + "://127.0.0.1:" + server.address().port + "/report?case=" + id }],
    });
  response.writeHead(item.status || 200, headers);
  response.end(
    "<!doctype html><head>" +
      (item.meta
        ? '<meta http-equiv="Content-Security-Policy" content="' +
          escaped(expand(item.meta, id)) +
          '">'
        : "") +
      "</head><body></body>",
  );
});
const kill = (signal) => {
  if (!chrome?.pid || chrome.exitCode !== null || chrome.signalCode !== null) return;
  try {
    process.kill(-chrome.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
};
try {
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const origin = scheme + "://127.0.0.1:" + server.address().port,
    url = origin + "/";
  const flags = [
    "--remote-debugging-port=0",
    "--user-data-dir=" + profile,
    "--no-first-run",
    "--no-default-browser-check",
    ...(tls ? ["--ignore-certificate-errors-spki-list=" + process.env.TURBO_CSP_REPORT_SPKI] : []),
    ...chromeExtensionArgs({ url, disableExtensions: true }),
    "about:blank",
  ];
  chrome = spawn(
    process.env.TURBO_SURF_CHROME_PATH ||
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    flags,
    { detached: true, stdio: ["ignore", "ignore", "pipe"] },
  );
  let stderr = "",
    spawnError;
  chrome.once("error", (error) => {
    spawnError = error;
  });
  chrome.stderr.on("data", (chunk) => {
    if (stderr.length < 1048576) stderr += String(chunk);
  });
  const startup = Date.now() + 30000;
  let endpoint;
  while (!(endpoint = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr)?.[1])) {
    if (spawnError) throw spawnError;
    if (chrome.exitCode !== null || Date.now() > startup) throw Error("Chrome startup");
    await pause(50);
  }
  socket = new WebSocket(endpoint);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  const errors = [];
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.exceptionThrown") errors.push(event.params.exceptionDetails);
      if (event.method.startsWith("Network.reportingApiReport")) reportingEvents.push(event.params);
    },
    30000,
  );
  const version = await connection.call("Browser.getVersion");
  const open = async () => {
    const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await connection.call("Target.attachToTarget", {
      targetId,
      flatten: true,
    });
    await connection.call("Runtime.enable", {}, sessionId);
    await connection.call("Page.enable", {}, sessionId);
    await connection.call("Network.enable", {}, sessionId);
    await connection.call("Network.enableReportingApi", { enable: true }, sessionId);
    return sessionId;
  };
  const evaluate = async (sessionId, expression) => {
    const result = await connection.call(
      "Runtime.evaluate",
      { expression, returnByValue: true, awaitPromise: true, allowUnsafeEvalBlockedByCSP: false },
      sessionId,
    );
    if (result.exceptionDetails)
      throw Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const ready = async (sessionId, path) => {
    const deadline = Date.now() + 10000;
    while (
      await evaluate(
        sessionId,
        'document.readyState!=="complete"||location.pathname!==' + JSON.stringify(path),
      )
    ) {
      if (Date.now() > deadline) throw Error("Document incomplete");
      await pause(25);
    }
  };
  const main = await open();
  await connection.call(
    "Network.setCookie",
    { name: "source", value: "1", url: origin + "/", path: "/", sameSite: "Lax" },
    main,
  );
  await connection.call(
    "Network.setCookie",
    {
      name: "cross",
      value: "2",
      url: scheme + "://localhost:" + server.address().port + "/",
      path: "/",
      sameSite: "Lax",
    },
    main,
  );
  const result = [];
  for (let id = 0; id < cases.length; id++) {
    const item = cases[id],
      sessionId = item.modern ? await open() : main,
      target = origin + "/page?case=" + id + "&private=authored#fragment",
      input = { ...item, meta: null, id };
    await connection.call(
      "Page.navigate",
      { url: origin + "/start?case=" + id + "&source=authored#referrer" },
      sessionId,
    );
    await ready(sessionId, "/start");
    if (item.frame) {
      await evaluate(
        sessionId,
        '(()=>{const frame=document.createElement("iframe");frame.id="frame";frame.src=' +
          JSON.stringify(target) +
          ";document.body.append(frame);})()",
      );
      const deadline = Date.now() + 10000;
      while (
        await evaluate(
          sessionId,
          '!document.getElementById("frame").contentDocument?.body||document.getElementById("frame").contentDocument.readyState!=="complete"||document.getElementById("frame").contentWindow.location.pathname!=="/page"',
        )
      ) {
        if (Date.now() > deadline) throw Error("Frame incomplete");
        await pause(25);
      }
      await evaluate(
        sessionId,
        '(()=>{const child=document.getElementById("frame");child.contentDocument.body.setAttribute("style","color:red");child.remove();})()\n//# sourceURL=' +
          origin +
          "/csp-report-endpoint-probe.js",
      );
      await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
    } else {
      await evaluate(sessionId, "location.href=" + JSON.stringify(target));
      await ready(sessionId, "/page");
      await evaluate(
        sessionId,
        "(" +
          probe.trim().replace(/;$/, "") +
          ")(" +
          JSON.stringify(input) +
          ")\n//# sourceURL=" +
          origin +
          "/csp-report-endpoint-probe.js",
      );
    }
    await pause(500);
    result.push({
      name: item.name,
      eventCount: await evaluate(
        sessionId,
        "typeof reportEventCount === 'number' ? reportEventCount : null",
      ),
      cookie: await evaluate(
        sessionId,
        "(()=>{try{return document.cookie}catch(error){return error.name}})()",
      ),
    });
  }
  const deadline = Date.now() + 75000;
  while (
    Date.now() < deadline &&
    !cases.every(
      (item, id) =>
        !item.endpoints || reports.some((report) => report.case === id && report.method === "POST"),
    )
  )
    await pause(250);
  await pause(500);
  const units = (value) =>
    Array.from({ length: value.length }, (_, index) => value.charCodeAt(index));
  const normalize = (value) => {
    if (typeof value === "string") return value.replaceAll(":" + server.address().port, ":PORT");
    if (Array.isArray(value)) return value.map(normalize);
    if (value && typeof value === "object")
      return Object.fromEntries(
        Object.entries(value).map(([key, value]) => [key, normalize(value)]),
      );
    return value;
  };
  const wire = reports.map((report) => {
    const body = structuredClone(report.body),
      rows = Array.isArray(body) ? body : [body?.["csp-report"]];
    for (const row of rows) {
      const content = row?.body || row;
      if (!content) continue;
      for (const key of ["script-sample", "sample"]) {
        if (typeof content[key] === "string") {
          content[key + "Units"] = units(content[key]);
          delete content[key];
        }
      }
      if (row?.age !== undefined)
        row.age = { nonnegative: Number.isInteger(row.age) && row.age >= 0 };
      if (row?.user_agent)
        row.user_agent = { matchesWire: row.user_agent === report.headers["user-agent"] };
    }
    const names = [
      "content-type",
      "user-agent",
      "accept-language",
      "accept-encoding",
      "sec-ch-ua",
      "sec-ch-ua-mobile",
      "sec-ch-ua-platform",
      "priority",
      "access-control-request-method",
      "access-control-request-headers",
      "origin",
      "referer",
      "cookie",
      "sec-fetch-site",
      "sec-fetch-mode",
      "sec-fetch-dest",
      "cache-control",
      "pragma",
      "accept",
    ];
    return normalize({
      ...report,
      headers: Object.fromEntries(names.map((name) => [name, report.headers[name] || null])),
      body,
    });
  });
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    casesSha256: createHash("sha256").update(casesText).digest("hex"),
    probeSha256: createHash("sha256").update(probe).digest("hex"),
    collectorSha256: createHash("sha256")
      .update(readFileSync(new URL(import.meta.url)))
      .digest("hex"),
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    errors,
    result,
    wire,
    closed,
    reportingEvents: normalize(reportingEvents),
    requests: normalize(requests),
  };
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, JSON.stringify(control, null, 2) + "\n");
  if (errors.length) throw Error("Authored runtime exceptions");
} finally {
  connection?.close();
  socket?.close();
  kill("SIGTERM");
  if (chrome && chrome.exitCode === null && chrome.signalCode === null)
    await Promise.race([new Promise((done) => chrome.once("exit", done)), pause(1000)]);
  if (chrome?.exitCode === null && chrome.signalCode === null) kill("SIGKILL");
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true });
}
