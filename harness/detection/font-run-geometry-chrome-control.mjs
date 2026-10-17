// Same-font geometry in a fresh normal installed Chrome, without automation font flags.
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
import { waitForWebSocketOpen } from "./websocket-open.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const { values } = parseArgs({
  options: {
    producer: { type: "string" },
    out: { type: "string" },
    chrome: { type: "string" },
    headless: { type: "boolean", default: false },
    probe: { type: "string", default: "font-runs" },
  },
});
if (!values.producer || !values.out)
  throw Error("Pass --producer /path/to/turbo-html2pdf --out /new/oracle.json; default is headed");
if (typeof WebSocket !== "function") throw Error("Use Node22 or newer for the CDP WebSocket");
const output = resolve(values.out);
if (existsSync(output)) throw Error("Preserve previous captures; choose a new output file");
const producer = resolve(values.producer),
  producerCommit = "2eac7f64142d8e6bde37f7ae46006c991b6f2347";
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const fonts = [
  ["IBM Plex Mono", "ibm-plex-mono/IBMPlexMono-Regular.ttf"],
  ["Fira Code", "fira-code/FiraCode-Regular.ttf"],
].map(([family, relative], index) => {
  const path = `crates/turbo-html2pdf-core/assets/fonts/${relative}`,
    bytes = readFileSync(join(producer, path)),
    pinned = execFileSync("git", ["show", `${producerCommit}:${path}`], {
      cwd: producer,
      maxBuffer: 8 * 1024 * 1024,
    });
  if (!bytes.equals(pinned)) throw Error(`Font differs from public producer pin: ${path}`);
  return { family, path, route: `/font-${index}.ttf`, bytes, sha256: sha(bytes) };
});
const probes = {
  "font-runs": {
    name: "font-run-geometry",
    dprs: [null, 1, 2],
    families: [...fonts, { family: "monospace" }],
  },
  "range-transforms": { name: "range-transform-stages", dprs: [null], families: fonts, cases: 11 },
  "leading-spaces": {
    name: "leading-space-geometry",
    dprs: [null],
    families: fonts.slice(0, 1),
    cases: 19,
  },
};
const selectedProbe = probes[values.probe];
if (!Object.hasOwn(probes, values.probe)) throw Error("Unknown font geometry probe");
const probePath = join(root, `harness/detection/${selectedProbe.name}-probe.js`),
  probe = readFileSync(probePath, "utf8"),
  collectorPath = fileURLToPath(import.meta.url),
  collectorSource = readFileSync(collectorPath),
  socketHelperPath = join(root, "harness/detection/websocket-open.mjs"),
  socketHelperSource = readFileSync(socketHelperPath),
  html = "<!doctype html><html><head></head><body></body></html>",
  fontRequests = [],
  errors = [];
const server = createServer((request, response) => {
  const font = fonts.find((item) => request.url === item.route);
  if (font) {
    fontRequests.push({ family: font.family, bytes: font.bytes.length, sha256: font.sha256 });
    response.writeHead(200, { "content-type": "font/ttf", "cache-control": "no-store" });
    response.end(font.bytes);
  } else {
    response.writeHead(200, { "content-type": "text/html", "cache-control": "no-store" });
    response.end(html);
  }
});
const profile = mkdtempSync(join(tmpdir(), "turbo-font-run-oracle-"));
let chrome, socket, connection;
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
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
  const base = `http://127.0.0.1:${server.address().port}`,
    url = `${base}/`;
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    ...chromeExtensionArgs({ url, disableExtensions: true }),
    ...(values.headless ? ["--headless=new"] : []),
    "about:blank",
  ];
  chrome = spawn(
    values.chrome ||
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
  const deadline = Date.now() + 30000;
  let endpoint;
  while (!(endpoint = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr)?.[1])) {
    if (spawnError) throw spawnError;
    if (chrome.exitCode !== null || Date.now() > deadline) throw Error("Chrome startup failed");
    await pause(50);
  }
  socket = new WebSocket(endpoint);
  await waitForWebSocketOpen(socket);
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.exceptionThrown") errors.push(event.params.exceptionDetails);
    },
    30000,
  );
  const version = await connection.call("Browser.getVersion"),
    result = [];
  for (const requestedDpr of selectedProbe.dprs) {
    for (const font of selectedProbe.families) {
      const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" }),
        { sessionId } = await connection.call("Target.attachToTarget", { targetId, flatten: true });
      await connection.call("Runtime.enable", {}, sessionId);
      await connection.call("Page.enable", {}, sessionId);
      await connection.call("DOM.enable", {}, sessionId);
      await connection.call("CSS.enable", {}, sessionId);
      if (requestedDpr !== null)
        await connection.call(
          "Emulation.setDeviceMetricsOverride",
          { width: 1280, height: 800, deviceScaleFactor: requestedDpr, mobile: false },
          sessionId,
        );
      const evaluate = async (expression) => {
        const reply = await connection.call(
          "Runtime.evaluate",
          { expression, returnByValue: true, awaitPromise: true },
          sessionId,
        );
        if (reply.exceptionDetails)
          throw Error(reply.exceptionDetails.exception?.description || reply.exceptionDetails.text);
        return reply.result.value;
      };
      await connection.call("Page.navigate", { url }, sessionId);
      const readyDeadline = Date.now() + 10000;
      while (
        !(await evaluate(
          `location.href===${JSON.stringify(url)}&&document.readyState==='complete'`,
        ))
      ) {
        if (Date.now() > readyDeadline) throw Error("Font fixture failed to load");
        await pause(25);
      }
      const fontSetup = await evaluate(
        `(async()=>{const family=${JSON.stringify(font.family)},fontUrl=${JSON.stringify(font.route ? base + font.route : null)};let face;if(fontUrl){face=new FontFace(family,'url('+JSON.stringify(fontUrl)+')');await face.load();document.fonts.add(face);await document.fonts.load('16px '+JSON.stringify(family),'abcz');}await document.fonts.ready;return{faceStatus:face?.status??null,fontAvailable:document.fonts.check('16px '+JSON.stringify(family),'abcz')}})()`,
      );
      const argument = JSON.stringify({ family: font.family }),
        geometry = await evaluate(`(${probe.trim().replace(/;$/, "")})(${argument})`);
      if (
        !geometry ||
        geometry.family !== font.family ||
        (selectedProbe.cases
          ? geometry.cases?.length !== selectedProbe.cases
          : Object.keys(geometry.decoration.checks).length !== 7)
      )
        throw Error("Incomplete font geometry result");
      const { root: documentNode } = await connection.call("DOM.getDocument", {}, sessionId),
        { nodeId } = await connection.call(
          "DOM.querySelector",
          { nodeId: documentNode.nodeId, selector: "#a" },
          sessionId,
        ),
        { fonts: platformFonts } = await connection.call(
          "CSS.getPlatformFontsForNode",
          { nodeId },
          sessionId,
        );
      if (
        font.route &&
        !platformFonts.some((item) => item.isCustomFont && item.familyName === font.family)
      )
        throw Error("The pinned webfont did not render the diagnostic text");
      const canvas = await evaluate(
        `(()=>{const c=document.createElement('canvas').getContext('2d');c.font='16px '+${JSON.stringify(font.family === "monospace" ? font.family : JSON.stringify(font.family))};return ${JSON.stringify(["a", "ab", "abc", "abcd", "abcde", "abcdefgh", "AV", "fi", "a b", "a b c"])}.map(text=>({text,width:c.measureText(text).width}))})()`,
      );
      result.push({
        viewportMode: requestedDpr === null ? "natural" : "emulated",
        requestedDpr,
        fontSetup,
        platformFonts,
        canvas,
        geometry,
      });
      await connection.call("Target.closeTarget", { targetId });
    }
  }
  for (const font of fonts)
    if (sha(readFileSync(join(producer, font.path))) !== font.sha256)
      throw Error("Font changed during capture");
  if (
    readFileSync(probePath, "utf8") !== probe ||
    !readFileSync(collectorPath).equals(collectorSource) ||
    !readFileSync(socketHelperPath).equals(socketHelperSource)
  )
    throw Error("Oracle sources changed during capture");
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    collectorNode: process.version,
    mode: values.headless ? "headless-direct-cdp" : "headed-direct-cdp",
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    recordedAt: new Date().toISOString(),
    producer: { repository: "miaskiewicz/turbo-html2pdf", commit: producerCommit },
    probeKind: values.probe,
    probeSha256: sha(probe),
    socketHelperSha256: sha(socketHelperSource),
    collectorSha256: sha(collectorSource),
    htmlSha256: sha(html),
    fonts: fonts.map(({ family, path, sha256, bytes }) => ({
      family,
      path,
      sha256,
      bytes: bytes.length,
      publicPinnedBytesIdentical: true,
    })),
    fontRequests,
    errors,
    result,
  };
  writeFileSync(output, `${JSON.stringify(control, null, 2)}\n`);
  console.log(
    JSON.stringify({
      browser: control.browser,
      mode: control.mode,
      rows: result.length,
      errors: errors.length,
    }),
  );
  if (errors.length) throw Error("Chrome reported unexpected runtime exceptions");
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
