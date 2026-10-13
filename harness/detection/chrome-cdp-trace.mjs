// Independent installed-Chrome oracle, including child frames and worker targets.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { enrichProtocolHeaders } from "./protocol-headers.mjs";

const config = JSON.parse(readFileSync(process.argv[2], "utf8"));
const out = resolve(process.argv[3]);
mkdirSync(out, { recursive: true, mode: 0o700 });
const profile = mkdtempSync(join(tmpdir(), "turbo-cdp-oracle-"));
const executable =
  config.chromePath ||
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const instrumented = !!config.interfaces,
  startedAt = new Date().toISOString();
const requests = [],
  responses = [],
  pages = [],
  scripts = [],
  frames = [],
  sockets = [],
  probes = [],
  errors = [],
  protocolNetwork = [],
  targets = [],
  consoles = [];
const tasks = [],
  requestRows = new Map(),
  frameRows = new Map(),
  contexts = new Map();
let chrome,
  socket,
  browserVersion,
  browserV8Version,
  connection,
  mainSession,
  sequence = 0,
  mainFrame,
  domReady = false;
let stage = "launch",
  chromeStderr = "";
const handlers = new Map();
const targetSessions = new Map();
const networkKey = (session, id) => `${session || "browser"}:${id}`;
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const save = (name, data) => writeFileSync(join(out, name), data, { mode: 0o600 });
const on = (event, callback) => handlers.set(event, [...(handlers.get(event) || []), callback]);
function call(method, params = {}, sessionId = mainSession) {
  return connection.call(method, params, sessionId);
}
async function evaluate(expression, contextId, sessionId = mainSession) {
  const result = await call(
    "Runtime.evaluate",
    {
      expression: expression + "\n//# sourceURL=<turbo-cdp-host>",
      returnByValue: true,
      awaitPromise: true,
      ...(contextId ? { contextId } : {}),
    },
    sessionId,
  );
  if (result.exceptionDetails)
    throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
  return result.result.value;
}
async function until(callback, timeout = 10000) {
  const deadline = Date.now() + timeout;
  do {
    if (await callback()) return;
    await pause(25);
  } while (Date.now() < deadline);
  throw new Error("Chrome fixture/navigation readiness timed out");
}
async function debugPort() {
  const server = createServer();
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const port = server.address().port;
  await new Promise((done, fail) => server.close((error) => (error ? fail(error) : done())));
  return port;
}
async function snapshot(phase) {
  const state = await evaluate(
    `JSON.stringify({url:location.href,html:(document.doctype?'<!DOCTYPE '+document.doctype.name+'>':'')+document.documentElement.outerHTML,browserFixture:globalThis.__browserFixture||null,inputState:(()=>{const input=document.querySelector('textarea[name=q],input[name=q]');return {value:input?.value??null,activeTag:document.activeElement?.tagName??null,activeName:document.activeElement?.getAttribute('name')??null,disabled:input?.disabled??null,readOnly:input?.readOnly??null,consentPresent:!!document.querySelector(${JSON.stringify(config.dismissSelector || "#W0wltc")})};})(),results:document.querySelectorAll(${JSON.stringify(config.successSelector || "#rso a h3, #search a h3")}).length,environment:{webdriver:navigator.webdriver,userAgent:navigator.userAgent,languages:navigator.languages,language:navigator.language,locale:Intl.DateTimeFormat().resolvedOptions().locale,viewport:{width:innerWidth,height:innerHeight},outerWindow:{width:outerWidth,height:outerHeight},deviceScaleFactor:devicePixelRatio,screen:{width:screen.width,height:screen.height,availWidth:screen.availWidth,availHeight:screen.availHeight}},interfaces:globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot()||null})`,
  );
  const row = JSON.parse(state),
    html = row.html;
  delete row.html;
  save(phase + ".html", html);
  const cookies = (await call("Network.getCookies")).cookies.map(({ name, domain }) => ({
    name,
    domain,
  }));
  const childFrames = [];
  if (instrumented)
    for (const [frameId, { contextId, sessionId }] of contexts)
      if (frameId !== mainFrame) {
        try {
          childFrames.push({
            url: frameRows.get(frameId)?.url,
            interfaces: await evaluate(
              `globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot()||null`,
              contextId,
              sessionId,
            ),
          });
        } catch (error) {
          childFrames.push({ url: frameRows.get(frameId)?.url, error: String(error) });
        }
      }
  const workers = [];
  if (instrumented)
    for (const [sessionId, target] of targetSessions) {
      if (!["worker", "shared_worker", "service_worker"].includes(target.type)) continue;
      try {
        workers.push({
          type: target.type,
          url: target.url,
          interfaces: await evaluate(
            `globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot()||null`,
            undefined,
            sessionId,
          ),
        });
      } catch (error) {
        workers.push({ type: target.type, url: target.url, error: String(error) });
      }
    }
  pages.push({ phase, ...row, bytes: Buffer.byteLength(html), cookies, childFrames, workers });
}
async function point(selector) {
  return JSON.parse(
    await evaluate(
      `(()=>{const node=document.querySelector(${JSON.stringify(selector)});if(!node)throw new Error('Input selector is absent');const r=node.getBoundingClientRect();return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2});})()`,
    ),
  );
}
async function key(key, type, text) {
  const code =
    key === "Enter"
      ? "Enter"
      : key === " "
        ? "Space"
        : /^[0-9]$/.test(key)
          ? "Digit" + key
          : key.length === 1 && /[a-z]/i.test(key)
            ? "Key" + key.toUpperCase()
            : key;
  const virtual = key === "Enter" ? 13 : key.length === 1 ? key.toUpperCase().charCodeAt(0) : 0;
  await call("Input.dispatchKeyEvent", {
    type,
    key,
    code,
    windowsVirtualKeyCode: virtual,
    ...(text ? { text, unmodifiedText: text } : {}),
  });
}
try {
  if (!existsSync(executable) || typeof WebSocket === "undefined")
    throw new Error("Installed Chrome and Node.js 22+ are required");
  const port = await debugPort();
  chrome = spawn(
    executable,
    [
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      `--lang=${config.locale || "en-US"}`,
      `--accept-lang=${(config.languages || ["en-US", "en"]).join(",")}`,
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"], detached: process.platform !== "win32" },
  );
  chrome.stderr.on("data", (chunk) => {
    if (chromeStderr.length < 1_048_576) chromeStderr += chunk;
  });
  await until(() => {
    if (chrome.exitCode !== null)
      throw new Error(`Chrome exited during startup (${chrome.exitCode})`);
    return chromeStderr.includes(`DevTools listening on ws://127.0.0.1:${port}/`);
  }, 30000);
  stage = "connect";
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  browserVersion = version.Browser;
  browserV8Version = version["V8-Version"];
  const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const tab = tabs.find((tab) => tab.type === "page");
  socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  connection = cdpConnection(socket, (data) => {
    if (
      [
        "Network.requestWillBeSent",
        "Network.requestWillBeSentExtraInfo",
        "Network.responseReceivedExtraInfo",
        "Network.loadingFailed",
      ].includes(data.method)
    )
      protocolNetwork.push({
        event: data.method.slice(8),
        timeMs: Date.now(),
        sessionId: data.sessionId,
        params: data.params,
      });
    for (const callback of handlers.get(data.method) || []) {
      try {
        callback(data.params, data.sessionId);
      } catch (error) {
        errors.push(String(error));
      }
    }
  });
  on("Runtime.executionContextCreated", ({ context }, sessionId) => {
    if (context.auxData?.isDefault && context.auxData.frameId)
      contexts.set(context.auxData.frameId, { contextId: context.id, sessionId });
  });
  on("Runtime.executionContextDestroyed", ({ executionContextId }, sessionId) => {
    for (const [frame, entry] of contexts)
      if (entry.contextId === executionContextId && entry.sessionId === sessionId)
        contexts.delete(frame);
  });
  on("Runtime.executionContextsCleared", (_, sessionId) => {
    for (const [frame, entry] of contexts)
      if (entry.sessionId === sessionId) contexts.delete(frame);
  });
  on("Runtime.consoleAPICalled", (params, sessionId) => consoles.push({ sessionId, ...params }));
  on("Runtime.exceptionThrown", ({ exceptionDetails }) =>
    errors.push(exceptionDetails.exception?.description || exceptionDetails.text),
  );
  on("Page.domContentEventFired", (_, sessionId) => {
    if (sessionId === mainSession) domReady = true;
  });
  on("Page.frameNavigated", ({ frame }, sessionId) => {
    if (!frame.parentId && sessionId === mainSession) mainFrame = frame.id;
    frameRows.set(frame.id, frame);
    frames.push({
      url: frame.url,
      name: frame.name || "",
      main: frame.id === mainFrame,
      sessionId,
    });
  });
  on("Network.requestWillBeSent", (event, sessionId) => {
    const requestKey = networkKey(sessionId, event.requestId);
    if (event.redirectResponse) {
      const previous = requestRows.get(networkKey(sessionId, event.requestId));
      responses.push({
        id: ++sequence,
        protocolRequestId: event.requestId,
        sessionId,
        requestId: previous?.id,
        timeMs: Date.now(),
        url: event.redirectResponse.url,
        status: event.redirectResponse.status,
        headers: event.redirectResponse.headers,
        type: previous?.type || event.type.toLowerCase(),
        redirect: true,
      });
    }
    const row = {
      id: ++sequence,
      protocolRequestId: event.requestId,
      sessionId,
      timeMs: Date.now(),
      method: event.request.method,
      url: event.request.url,
      type: event.type.toLowerCase(),
      body: event.request.postData || null,
      headers: event.request.headers,
      initiator: event.initiator,
    };
    requests.push(row);
    requestRows.set(requestKey, row);
    if (event.request.hasPostData && event.request.postData === undefined)
      tasks.push(
        call("Network.getRequestPostData", { requestId: event.requestId }, sessionId)
          .then(({ postData }) => {
            row.body = postData;
          })
          .catch((error) => {
            row.bodyError = String(error);
          }),
      );
  });
  on("Network.responseReceived", (event, sessionId) => {
    const response = event.response,
      row = {
        id: ++sequence,
        protocolRequestId: event.requestId,
        sessionId,
        requestId: requestRows.get(networkKey(sessionId, event.requestId))?.id,
        timeMs: Date.now(),
        url: response.url,
        status: response.status,
        headers: response.headers,
        type: event.type.toLowerCase(),
      };
    responses.push(row);
  });
  on("Network.loadingFailed", (event, sessionId) =>
    errors.push({
      url: requestRows.get(networkKey(sessionId, event.requestId))?.url,
      failure: { errorText: event.errorText },
    }),
  );
  on("Network.loadingFinished", (event, sessionId) => {
    const row = responses.findLast(
      (response) =>
        response.protocolRequestId === event.requestId && response.sessionId === sessionId,
    );
    if (row && ["document", "script", "xhr", "fetch"].includes(row.type))
      tasks.push(
        call("Network.getResponseBody", { requestId: event.requestId }, sessionId)
          .then(({ body, base64Encoded }) => {
            const bytes = Buffer.from(body, base64Encoded ? "base64" : "utf8");
            row.bodyFile = `${row.id}-${row.type}.txt`;
            row.bytes = bytes.length;
            row.bodyHash = createHash("sha256").update(bytes).digest("hex");
            save(row.bodyFile, bytes);
          })
          .catch((error) => {
            row.bodyError = String(error);
          }),
      );
  });
  const websocketRows = new Map();
  on("Network.webSocketCreated", (event, sessionId) => {
    const row = { url: event.url, sessionId, sent: [], received: [] };
    sockets.push(row);
    websocketRows.set(networkKey(sessionId, event.requestId), row);
  });
  on("Network.webSocketFrameSent", (event, sessionId) =>
    websocketRows
      .get(networkKey(sessionId, event.requestId))
      ?.sent.push(event.response.payloadData),
  );
  on("Network.webSocketFrameReceived", (event, sessionId) =>
    websocketRows
      .get(networkKey(sessionId, event.requestId))
      ?.received.push(event.response.payloadData),
  );
  const childSetup = new Map();
  const autoAttach = { autoAttach: true, waitForDebuggerOnStart: true, flatten: true };
  async function configureTarget(sessionId, targetInfo) {
    targetSessions.set(sessionId, targetInfo);
    targets.push({ sessionId, ...targetInfo });
    try {
      await call("Network.enable", {}, sessionId);
      await call("Runtime.enable", {}, sessionId);
      await call("Target.setAutoAttach", autoAttach, sessionId);
      if (["iframe", "page"].includes(targetInfo.type)) {
        await call("Page.enable", {}, sessionId);
        if (instrumented)
          await call(
            "Page.addScriptToEvaluateOnNewDocument",
            { source: readFileSync(config.interfaceScript, "utf8") },
            sessionId,
          );
      } else if (instrumented) {
        await evaluate(readFileSync(config.interfaceScript, "utf8"), undefined, sessionId);
      }
      if (config.debugger) {
        await call("Debugger.enable", {}, sessionId);
        await call("Debugger.setSkipAllPauses", { skip: true }, sessionId);
      }
    } catch (error) {
      errors.push({
        phase: "target setup",
        targetType: targetInfo.type,
        url: targetInfo.url,
        error: String(error),
      });
    } finally {
      await call("Runtime.runIfWaitingForDebugger", {}, sessionId).catch((error) =>
        errors.push(String(error)),
      );
    }
  }
  on("Target.attachedToTarget", ({ sessionId, targetInfo }) => {
    if (childSetup.has(sessionId)) return;
    const task = configureTarget(sessionId, targetInfo);
    childSetup.set(sessionId, task);
    tasks.push(task);
  });
  on("Target.detachedFromTarget", ({ sessionId }) => {
    targetSessions.delete(sessionId);
    for (const [frame, entry] of contexts)
      if (entry.sessionId === sessionId) contexts.delete(frame);
  });
  ({ sessionId: mainSession } = await call(
    "Target.attachToTarget",
    { targetId: tab.id, flatten: true },
    null,
  ));
  // Explicit attachment emits the same event as automatic child attachment.
  await childSetup.get(mainSession);
  await call("Page.enable");
  await call("Runtime.enable");
  if (config.localCertificate) {
    const target = new URL(config.url);
    if (target.protocol !== "https:" || !["127.0.0.1", "localhost"].includes(target.hostname))
      throw new Error("The certificate fixture override is restricted to localhost HTTPS");
    await call("Security.setIgnoreCertificateErrors", { ignore: true });
  }
  const viewport = config.viewport || { width: 1280, height: 800 },
    screen = config.screen || { width: 1920, height: 1080 };
  await call("Emulation.setDeviceMetricsOverride", {
    ...viewport,
    deviceScaleFactor: config.deviceScaleFactor ?? 2,
    mobile: false,
    screenWidth: screen.width,
    screenHeight: screen.height,
  });
  await call("Emulation.setLocaleOverride", { locale: config.locale || "en-US" });
  if (config.timezone) await call("Emulation.setTimezoneOverride", { timezoneId: config.timezone });
  const { windowId } = await call("Browser.getWindowForTarget", { targetId: tab.id });
  await call("Browser.setWindowBounds", {
    windowId,
    bounds: config.outerWindow || { width: viewport.width, height: viewport.height + 88 },
  });

  if (config.debugger) {
    on("Debugger.scriptParsed", (event, sessionId) =>
      tasks.push(
        call("Debugger.getScriptSource", { scriptId: event.scriptId }, sessionId)
          .then(({ scriptSource }) => {
            const hash = createHash("sha256").update(scriptSource).digest("hex");
            save(`script-${hash}.js`, scriptSource);
            scripts.push({
              url: event.url,
              hash,
              executionContextId: event.executionContextId,
              sessionId,
              startLine: event.startLine,
            });
          })
          .catch((error) => errors.push(`script source: ${error}`)),
      ),
    );
    await call("Debugger.enable");
    await call("Debugger.setSkipAllPauses", { skip: true });
  }
  await call(
    "Target.setAutoAttach",
    {
      ...autoAttach,
      filter: [
        { type: "service_worker", exclude: false },
        { type: "shared_worker", exclude: false },
        { exclude: true },
      ],
    },
    null,
  );
  stage = "navigation";
  domReady = false;
  const navigation = await call("Page.navigate", { url: config.url });
  if (navigation.errorText) throw new Error(navigation.errorText);
  await until(() => domReady, 30000);
  stage = "fixture readiness";
  if (config.readySelector)
    await until(() =>
      evaluate(`!!document.querySelector(${JSON.stringify(config.readySelector)})`),
    );
  stage = "snapshots and input";
  await pause(config.waitMs || 0);
  await snapshot("home");
  if (config.dismissSelector) {
    const present = await evaluate(
      `!!document.querySelector(${JSON.stringify(config.dismissSelector)})`,
    );
    if (present) {
      const coordinates = await point(config.dismissSelector);
      await call("Input.dispatchMouseEvent", {
        type: "mousePressed",
        button: "left",
        clickCount: 1,
        ...coordinates,
      });
      await call("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        button: "left",
        clickCount: 1,
        ...coordinates,
      });
      await pause(config.waitMs || 0);
      await snapshot("dismiss");
    }
  }
  for (const [index, step] of (config.steps || []).entries()) {
    if (step.move) {
      const coordinates = await point(step.move.selector);
      await call("Input.dispatchMouseEvent", { type: "mouseMoved", ...coordinates });
    }
    if (step.click) {
      const coordinates = await point(step.click);
      await call("Input.dispatchMouseEvent", {
        type: "mousePressed",
        button: "left",
        clickCount: 1,
        ...coordinates,
      });
      await call("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        button: "left",
        clickCount: 1,
        ...coordinates,
      });
    }
    if (step.focus) await evaluate(`document.querySelector(${JSON.stringify(step.focus)}).focus()`);
    if (step.type !== undefined)
      for (const character of String(step.type)) {
        await key(character, "keyDown", character);
        await pause(110);
        await key(character, "keyUp");
      }
    if (step.press) {
      await key(step.press, "keyDown", step.press === "Enter" ? "\r" : undefined);
      await key(step.press, "keyUp");
    }
    if (step.wait) await pause(step.wait);
    await snapshot(`step-${index}`);
  }
  await pause(config.waitMs || 0);
  await snapshot("search");
  for (const path of config.probes || []) {
    try {
      const raw = await evaluate(readFileSync(path, "utf8"));
      probes.push({ path, result: typeof raw === "string" ? JSON.parse(raw) : raw });
    } catch (error) {
      probes.push({ path, error: String(error) });
    }
  }
} catch (error) {
  errors.push({ phase: "capture", stage, error: String(error) });
  process.exitCode = 1;
} finally {
  for (let index = 0; index < tasks.length; ) {
    const batch = tasks.slice(index);
    index = tasks.length;
    await Promise.allSettled(batch);
  }
  enrichProtocolHeaders(requests, protocolNetwork);
  save(
    "trace.json",
    JSON.stringify(
      {
        runtime: "chrome",
        backend: "raw-cdp",
        instrumented,
        startedAt,
        browserVersion,
        browserV8Version,
        executable,
        config,
        pages,
        requests,
        responses,
        scripts,
        protocolNetwork,
        sockets,
        frames,
        targets,
        consoles,
        targetCoverage: "page, out-of-process iframe, dedicated/shared/service worker CDP sessions",
        probes,
        errors,
      },
      null,
      2,
    ),
  );
  save("chrome-stderr.log", chromeStderr);
  if (chrome) {
    const running = () => chrome.exitCode === null && chrome.signalCode === null;
    const exited = running() ? new Promise((done) => chrome.once("exit", done)) : Promise.resolve();
    if (connection)
      await Promise.race([call("Browser.close", {}, null).catch(() => {}), pause(1000)]);
    const stop = (signal) => {
      try {
        if (process.platform === "win32") {
          if (running()) chrome.kill(signal);
        } else process.kill(-chrome.pid, signal);
      } catch {}
    };
    stop("SIGTERM");
    await Promise.race([exited, pause(2000)]);
    // Helpers can outlive the browser process and still write its owned profile.
    stop("SIGKILL");
  }
  connection?.close();
  socket?.close();
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  console.log(
    JSON.stringify({
      out,
      backend: "raw-cdp",
      instrumented,
      requests: requests.length,
      final: {
        path: new URL(pages.at(-1)?.url || "about:blank").pathname,
        results: pages.at(-1)?.results,
      },
      errors: errors.length,
    }),
  );
}
