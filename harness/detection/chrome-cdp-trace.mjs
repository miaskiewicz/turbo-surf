// Independent installed-Chrome oracle, including child frames and worker targets.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
import { cdpConnection } from "./cdp-connection.mjs";
import { enrichProtocolHeaders } from "./protocol-headers.mjs";
import { responseNetworkEvents, responseTransport } from "./chrome-network-metadata.mjs";

import {
  captureStableDocument,
  documentSnapshotState,
  finishObservation,
  observationMilliseconds,
  observationTriggerIndex,
  startObservation,
  waitForObservation,
} from "./observation-window.mjs";
import { createMainNavigationTracker } from "./main-navigation.mjs";
import { callWithLifecycle, collectorLifecycle } from "./collector-lifecycle.mjs";
import {
  effectiveTypedControl,
  prepareTypedControl,
  requireEffectiveInput,
} from "./effective-input.mjs";
const config = JSON.parse(readFileSync(process.argv[2], "utf8"));
const observationMs = observationMilliseconds(config);
const observationClock = { monotonic: () => performance.now(), wall: () => Date.now() };
let observationWindow = null;
const mainNavigation = createMainNavigationTracker();
const out = resolve(process.argv[3]);
mkdirSync(out, { recursive: true, mode: 0o700 });
const profile = mkdtempSync(join(tmpdir(), "turbo-cdp-oracle-"));
const executable =
  config.chromePath ||
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const instrumented = !!config.interfaces,
  startedAt = new Date().toISOString();
const inputStateSource = readFileSync(new URL("./input-state.js", import.meta.url), "utf8");
const requests = [],
  actions = [],
  dwell = [],
  responses = [],
  pages = [],
  scripts = [],
  frames = [],
  sockets = [],
  probes = [],
  errors = [],
  collectorErrors = [],
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
  chromeStderr = "",
  captureComplete = false,
  dismissal = null;
let effectiveExtensionFlags = [];
let browserContext = null;
const handlers = new Map();
const targetSessions = new Map();
const networkKey = (session, id) => `${session || "browser"}:${id}`;
const lifecycle = collectorLifecycle({
  onAbort(signal) {
    captureComplete = false;
    collectorError({ phase: "interrupted", signal });
    process.exitCode = signal === "SIGINT" ? 130 : 143;
    writeTrace();
    connection?.close();
    socket?.close();
  },
});
const pause = (ms) => lifecycle.pause(ms);
async function pageDwell(stage) {
  const span = { stage, requestedMs: config.waitMs || 0, startedMs: Date.now(), completed: false };
  dwell.push(span);
  const deadline = span.startedMs + span.requestedMs;
  while (Date.now() < deadline) await pause(deadline - Date.now());
  span.finishedMs = Date.now();
  span.completed = true;
}
const save = (name, data) => writeFileSync(join(out, name), data, { mode: 0o600 });
const on = (event, callback) => handlers.set(event, [...(handlers.get(event) || []), callback]);
function collectorError(error) {
  errors.push(error);
  collectorErrors.push(error);
}
function call(method, params = {}, sessionId = mainSession) {
  return callWithLifecycle(lifecycle, connection, method, params, sessionId);
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
  const mainNavigationBeforeSnapshot = mainNavigation.snapshot();
  const identity = async () => {
    const { frameTree } = await call("Page.getFrameTree");
    const frame = frameTree.frame;
    if (mainFrame !== frame.id)
      mainNavigation.setScope({ sessionId: mainSession, frameId: frame.id });
    mainFrame = frame.id;
    frameRows.set(frame.id, frame);
    return `${mainSession}/${frame.id}/${frame.loaderId}`;
  };
  const { value, retries } = await captureStableDocument(identity, async () => {
    const state = await evaluate(
      `JSON.stringify({url:location.href,readyState:document.readyState,...(${documentSnapshotState.toString()})(document,${JSON.stringify(config.successSelector || "#rso a h3, #search a h3")}),browserFixture:globalThis.__browserFixture||null,inputState:(()=>{const captureInputState=${inputStateSource}\nreturn captureInputState(document,${JSON.stringify(config.dismissSelector || "#W0wltc")});})(),environment:{webdriver:navigator.webdriver,userAgent:navigator.userAgent,languages:navigator.languages,language:navigator.language,locale:Intl.DateTimeFormat().resolvedOptions().locale,timezone:Intl.DateTimeFormat().resolvedOptions().timeZone,timeZoneOffset:new Date().getTimezoneOffset(),viewport:{width:innerWidth,height:innerHeight},outerWindow:{width:outerWidth,height:outerHeight},deviceScaleFactor:devicePixelRatio,screen:{width:screen.width,height:screen.height,availWidth:screen.availWidth,availHeight:screen.availHeight}},interfaces:globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot()||null})`,
    );
    const row = JSON.parse(state),
      html = row.html;
    delete row.html;
    const loaderId = frameRows.get(mainFrame)?.loaderId;
    const documentResponse = loaderId
      ? responses.findLast(
          (response) =>
            response.sessionId === mainSession &&
            response.frameId === mainFrame &&
            response.loaderId === loaderId &&
            response.type === "document",
        )
      : null;
    row.status = documentResponse?.status ?? null;
    row.statusSource = documentResponse ? "cdp-main-document-loader" : "unavailable";
    row.statusDocumentUrl = documentResponse?.url ?? null;
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
    return { phase, ...row, bytes: Buffer.byteLength(html), cookies, childFrames, workers };
  });
  value.navigationSnapshotRetries = retries;
  value.mainNavigationBeforeSnapshot = mainNavigationBeforeSnapshot;
  pages.push(value);
}
async function point(selector) {
  // Headed oracle processes may share the desktop with another collector.
  // Activate this target before trusted input rather than relying on launch focus.
  await call("Page.bringToFront");
  return JSON.parse(
    await evaluate(
      `(()=>{const node=document.querySelector(${JSON.stringify(selector)});if(!node)throw new Error('Input selector is absent');node.scrollIntoView({block:'center',inline:'center'});const r=node.getBoundingClientRect();if(!r.width||!r.height)throw new Error('Input selector has no rendered box');const x=r.x+r.width/2,y=r.y+r.height/2,hit=document.elementFromPoint(x,y);if(!hit||!node.contains(hit))throw new Error('Input selector is obscured');return JSON.stringify({x,y});})()`,
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
async function capture() {
  effectiveExtensionFlags = chromeExtensionArgs(config);
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
      ...effectiveExtensionFlags,
      `--lang=${config.locale || "en-US"}`,
      `--accept-lang=${(config.languages || ["en-US", "en"]).join(",")}`,
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"], detached: process.platform !== "win32" },
  );
  lifecycle.own(chrome);
  let spawnError;
  chrome.on("error", (error) => {
    spawnError = error;
  });
  chrome.stderr.on("data", (chunk) => {
    if (chromeStderr.length < 1_048_576) chromeStderr += chunk;
  });
  await until(() => {
    if (spawnError) throw spawnError;
    if (chrome.exitCode !== null)
      throw new Error(`Chrome exited during startup (${chrome.exitCode})`);
    return chromeStderr.includes(`DevTools listening on ws://127.0.0.1:${port}/`);
  }, 30000);
  stage = "connect";
  const version = await (
    await fetch(`http://127.0.0.1:${port}/json/version`, { signal: lifecycle.signal })
  ).json();
  browserVersion = version.Browser;
  browserV8Version = version["V8-Version"];
  const tabs = await (
    await fetch(`http://127.0.0.1:${port}/json/list`, { signal: lifecycle.signal })
  ).json();
  let tab = tabs.find((tab) => tab.type === "page");
  socket = new WebSocket(version.webSocketDebuggerUrl);
  await lifecycle.race(
    new Promise((done, fail) => {
      socket.onopen = done;
      socket.onerror = fail;
      socket.onclose = () => fail(new Error("Chrome websocket closed during startup"));
    }),
  );
  connection = cdpConnection(socket, (data) => {
    if (responseNetworkEvents.some((event) => data.method === `Network.${event}`))
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
        collectorError(String(error));
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
    if (!frame.parentId && sessionId === mainSession) {
      if (mainFrame !== frame.id)
        mainNavigation.setScope({ sessionId: mainSession, frameId: frame.id });
      mainFrame = frame.id;
    }
    mainNavigation.commit(frame, sessionId);
    frameRows.set(frame.id, frame);
    frames.push({
      url: frame.url,
      name: frame.name || "",
      main: frame.id === mainFrame,
      sessionId,
    });
  });
  on("Network.requestWillBeSent", (event, sessionId) => {
    mainNavigation.request(event, sessionId);
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
        transport: responseTransport(event.redirectResponse),
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
        frameId: event.frameId,
        loaderId: event.loaderId,
        requestId: requestRows.get(networkKey(sessionId, event.requestId))?.id,
        timeMs: Date.now(),
        url: response.url,
        status: response.status,
        headers: response.headers,
        type: event.type.toLowerCase(),
        transport: responseTransport(response),
      };
    responses.push(row);
  });
  on("Network.loadingFailed", (event, sessionId) => {
    mainNavigation.failed(event, sessionId);
    errors.push({
      url: requestRows.get(networkKey(sessionId, event.requestId))?.url,
      failure: { errorText: event.errorText },
    });
  });
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
      collectorError({
        phase: "target setup",
        targetType: targetInfo.type,
        url: targetInfo.url,
        error: String(error),
      });
    } finally {
      await call("Runtime.runIfWaitingForDebugger", {}, sessionId).catch((error) =>
        collectorError(String(error)),
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
  let createdBrowserContext;
  if (config.chromeIncognito) {
    ({ browserContextId: createdBrowserContext } = await call(
      "Target.createBrowserContext",
      { disposeOnDetach: true },
      null,
    ));
    const { targetId } = await call(
      "Target.createTarget",
      { url: "about:blank", browserContextId: createdBrowserContext },
      null,
    );
    await call("Target.closeTarget", { targetId: tab.id }, null);
    tab = { id: targetId };
  }
  ({ sessionId: mainSession } = await call(
    "Target.attachToTarget",
    { targetId: tab.id, flatten: true },
    null,
  ));
  // Explicit attachment emits the same event as automatic child attachment.
  await childSetup.get(mainSession);
  const contextList = await call("Target.getBrowserContexts", {}, null);
  const targetState = await call("Target.getTargetInfo", { targetId: tab.id }, null);
  const nonDefaultContext = (contextList.browserContextIds || []).includes(
    targetState.targetInfo.browserContextId,
  );
  browserContext = {
    requestedIncognito: !!config.chromeIncognito,
    nonDefaultContext,
    createdContextVerified:
      !!createdBrowserContext && targetState.targetInfo.browserContextId === createdBrowserContext,
    isolation: "temporary-user-data-directory",
    source: config.chromeIncognito
      ? "cdp-created-context-target-and-browser-context-list"
      : "cdp-target-and-browser-context-list",
  };
  if (config.chromeIncognito && (!nonDefaultContext || !browserContext.createdContextVerified))
    throw new Error("Requested Incognito target was not verified in a non-default browser context");
  await call("Page.enable");
  const { frameTree: initialFrameTree } = await call("Page.getFrameTree");
  mainFrame = initialFrameTree.frame.id;
  frameRows.set(mainFrame, initialFrameTree.frame);
  mainNavigation.setScope({ sessionId: mainSession, frameId: mainFrame });
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
          .catch((error) => collectorError(`script source: ${error}`)),
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
    await until(
      () => evaluate(`!!document.querySelector(${JSON.stringify(config.readySelector)})`),
      config.readyTimeoutMs ?? 5000,
    );
  stage = "snapshots and input";
  await pageDwell("home");
  await snapshot("home");
  if (config.dismissSelector) {
    const present = await evaluate(
      `!!document.querySelector(${JSON.stringify(config.dismissSelector)})`,
    );
    dismissal = { selector: config.dismissSelector, present, clicked: false };
    if (present) {
      const coordinates = await point(config.dismissSelector);
      await call("Input.dispatchMouseEvent", { type: "mouseMoved", ...coordinates });
      await call("Input.dispatchMouseEvent", {
        type: "mousePressed",
        button: "left",
        buttons: 1,
        clickCount: 1,
        ...coordinates,
      });
      await call("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        button: "left",
        buttons: 0,
        clickCount: 1,
        ...coordinates,
      });
      dismissal.clicked = true;
      await pageDwell("dismiss");
      await snapshot("dismiss");
    }
  }
  let focusSelector = null,
    typedInput = null;
  const controlCall = async (handle, fn, value) => {
    const result = await call("Runtime.callFunctionOn", {
      objectId: handle,
      functionDeclaration: `function(value){return (${fn.toString()})(this,value);}`,
      arguments: [{ value }],
      returnByValue: true,
    });
    if (result.exceptionDetails)
      throw new Error(
        result.exceptionDetails.exception?.description || result.exceptionDetails.text,
      );
    return result.result.value;
  };
  const verifyTypedInput = async () => {
    const evidence = await controlCall(
      typedInput.handle,
      effectiveTypedControl,
      typedInput.expected,
    );
    try {
      return requireEffectiveInput(evidence);
    } catch (error) {
      collectorError({ phase: "effective-input", evidence });
      throw error;
    }
  };
  const observationIndex = observationTriggerIndex(config.steps || []);
  let pressed = false;
  for (const [index, step] of (config.steps || []).entries()) {
    if (index === observationIndex)
      observationWindow = startObservation(observationMs, index, observationClock);
    const startedMs = Date.now();
    let inputEvidence;
    if (step.move) {
      const coordinates = await point(step.move.selector);
      await call("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        ...coordinates,
        ...(pressed ? { button: "left", buttons: 1, force: 0.5 } : {}),
      });
    }
    // A held press (down … up) mirrors the host driver's drag steps; a physical mouse
    // reports Chrome's 0.5 pressure while pressed, which CDP needs as an explicit force.
    if (step.down || step.up) {
      pressed = Boolean(step.down);
      await call("Input.dispatchMouseEvent", {
        type: pressed ? "mousePressed" : "mouseReleased",
        button: "left",
        buttons: pressed ? 1 : 0,
        force: pressed ? 0.5 : 0,
        clickCount: 1,
        ...(await point(step.down || step.up)),
      });
    }
    if (step.click) {
      const coordinates = await point(step.click);
      await call("Input.dispatchMouseEvent", {
        type: "mousePressed",
        button: "left",
        buttons: 1,
        clickCount: 1,
        ...coordinates,
      });
      await call("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        button: "left",
        buttons: 0,
        clickCount: 1,
        ...coordinates,
      });
    }
    if (step.focus) {
      focusSelector = step.focus;
      await evaluate(
        `(()=>{const control=document.querySelector(${JSON.stringify(step.focus)});if(!control)throw new Error('Focus selector is absent');control.focus();if(document.activeElement!==control)throw new Error('Focus selector did not become active');})()`,
      );
    }
    if (step.type !== undefined) {
      if (typedInput) await call("Runtime.releaseObject", { objectId: typedInput.handle });
      const control = await call("Runtime.evaluate", {
        expression: focusSelector
          ? `document.querySelector(${JSON.stringify(focusSelector)})`
          : "document.activeElement",
        returnByValue: false,
      });
      if (!control.result.objectId) throw new Error("Typing control is unavailable");
      typedInput = {
        handle: control.result.objectId,
        ...(await controlCall(control.result.objectId, prepareTypedControl, String(step.type))),
      };
      for (const character of String(step.type)) {
        await key(character, "keyDown", character);
        await pause(config.inputCadenceMs ?? 110);
        await key(character, "keyUp");
      }
      inputEvidence = await verifyTypedInput();
    }
    if (step.press) {
      if (step.press === "Enter" && typedInput) await verifyTypedInput();
      await key(step.press, "keyDown", step.press === "Enter" ? "\r" : undefined);
      await key(step.press, "keyUp");
    }
    if (step.wait) await pause(step.wait);
    actions.push({
      index,
      action: Object.keys(step)[0],
      ...(inputEvidence ? { inputEvidence } : {}),
      completed: true,
      startedMs,
      finishedMs: Date.now(),
    });
    await snapshot(`step-${index}`);
  }
  if (observationWindow) await waitForObservation(observationWindow, observationClock, pause);
  else await pageDwell("search");
  const snapshotStarted = observationClock.monotonic();
  if (observationWindow) observationWindow.record.snapshotStartedMs = Date.now();
  await snapshot("search");
  if (observationWindow) {
    const navigation = mainNavigation.snapshot();
    finishObservation(
      observationWindow,
      observationClock,
      !navigation.pendingNavigation &&
        !navigation.navigationFailed &&
        pages.at(-1).documentElementPresent === true,
      {
        ...navigation,
        documentElementPresent: pages.at(-1).documentElementPresent,
        snapshotElapsedMs: observationClock.monotonic() - snapshotStarted,
      },
    );
  }
  for (const path of config.probes || []) {
    try {
      const raw = await evaluate(readFileSync(path, "utf8"));
      probes.push({ path, result: typeof raw === "string" ? JSON.parse(raw) : raw });
    } catch (error) {
      probes.push({ path, error: String(error) });
    }
  }
  lifecycle.throwIfAborted();
  captureComplete = true;
}
try {
  await lifecycle.race(capture());
} catch (error) {
  if (observationWindow) finishObservation(observationWindow, observationClock, false);
  collectorError({ phase: "capture", stage, error: String(error) });
  process.exitCode = 1;
} finally {
  try {
    for (let index = 0; index < tasks.length; ) {
      const batch = tasks.slice(index);
      index = tasks.length;
      if (lifecycle.signal.aborted) break;
      await lifecycle.race(Promise.allSettled(batch)).catch(() => {});
    }
    writeTrace();
  } finally {
    if (!lifecycle.signal.aborted && connection)
      await Promise.race([
        call("Browser.close", {}, null).catch(() => {}),
        new Promise((done) => setTimeout(done, 500)),
      ]);
    await lifecycle.stopBrowser();
    connection?.close();
    socket?.close();
    lifecycle.dispose();
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
}

function writeTrace() {
  enrichProtocolHeaders(requests, protocolNetwork);
  save(
    "trace.json",
    JSON.stringify(
      {
        runtime: "chrome",
        backend: "raw-cdp",
        instrumented,
        captureComplete: captureComplete && collectorErrors.length === 0,
        collectorErrors,
        actions,
        dwell,
        observation: observationWindow?.record ?? null,
        dismissal,
        browserContext,
        startedAt,
        browserVersion,
        browserV8Version,
        executable,
        effectiveExtensionFlags,
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
        networkCoverage: {
          source: "cdp-network",
          scope: "attached-target-sessions",
          targets: "attached page, iframe and worker sessions",
          enabledInBaseline: true,
          packetCapture: false,
          responseMetadata:
            "protocol, securityDetails, timing, connection and cache state; null when CDP omits a field",
        },
        probes,
        errors,
      },
      null,
      2,
    ),
  );
  save("chrome-stderr.log", chromeStderr);
}
