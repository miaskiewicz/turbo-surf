import childProcess from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
// Fresh Chrome oracle; no cookies leave this process or feed the native attempt.
import { enrichProtocolHeaders } from "./protocol-headers.mjs";
import { enrichResponseTransport, responseNetworkEvents } from "./chrome-network-metadata.mjs";
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
import { collectorLifecycle, captureOwnedLaunch } from "./collector-lifecycle.mjs";
import {
  chromeExecutable,
  effectiveTypedControl,
  prepareTypedControl,
  requireEffectiveInput,
} from "./effective-input.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
const config = JSON.parse(readFileSync(process.argv[2], "utf8"));
const observationMs = observationMilliseconds(config);
const observationClock = { monotonic: () => performance.now(), wall: () => Date.now() };
let observationWindow = null;
const mainNavigation = createMainNavigationTracker();
if (config.chromeBackend !== "patchright") {
  await import("./chrome-cdp-trace.mjs");
  process.exit(process.exitCode || 0);
}
const require = createRequire(
  new URL("../../scripts/browser-sidecar/package.json", import.meta.url),
);
const { chromium } = require("patchright");
const out = resolve(process.argv[3]);
const instrumented = config.interfaces;
const inputStateSource = readFileSync(new URL("./input-state.js", import.meta.url), "utf8");
mkdirSync(out, { recursive: true });
const profile = mkdtempSync(join(tmpdir(), "turbo-search-oracle-"));
let context, networkSession, browserVersion, browserV8Version;
const executable = chromeExecutable(config);
let headless = false;
const requests = [],
  actions = [],
  dwell = [],
  responses = [],
  errors = [],
  collectorErrors = [],
  pages = [],
  pending = [];
const scripts = [],
  sockets = [],
  frames = [],
  probes = [];
const protocolNetwork = [];
let captureComplete = false,
  dismissal = null;
const lifecycle = collectorLifecycle({
  onAbort(signal) {
    captureComplete = false;
    collectorError({ phase: "interrupted", signal });
    process.exitCode = signal === "SIGINT" ? 130 : 143;
    writeTrace();
    void context?.close().catch(() => {});
  },
});
async function pageDwell(stage) {
  const span = { stage, requestedMs: config.waitMs || 0, startedMs: Date.now(), completed: false };
  dwell.push(span);
  const deadline = span.startedMs + span.requestedMs;
  while (Date.now() < deadline) await lifecycle.pause(deadline - Date.now());
  span.finishedMs = Date.now();
  span.completed = true;
}
function collectorError(error) {
  errors.push(error);
  collectorErrors.push(error);
}
const requestIds = new WeakMap();
const requestRows = new WeakMap();
const startedAt = new Date().toISOString();
let sequence = 0;
async function capture() {
  // Keyboard-driven authored controls opt into headless (loopback-only, turbo-surf#52).
  headless = chromeExtensionArgs({ url: config.url, headless: config.headless }).includes(
    "--headless=new",
  );
  context = await captureOwnedLaunch(
    childProcess,
    profile,
    () =>
      chromium.launchPersistentContext(profile, {
        ...(executable ? { executablePath: executable } : { channel: "chrome" }),
        handleSIGTERM: false,
        handleSIGINT: false,
        handleSIGHUP: false,
        headless,
        viewport: config.viewport || { width: 1280, height: 800 },
        screen: config.screen || { width: 1920, height: 1080 },
        deviceScaleFactor: config.deviceScaleFactor ?? 2,
        // Playwright's locale override narrows navigator.languages to a single entry.
        // Browser preferences retain the same ordered language list as the native profile.
        args: [
          `--lang=${config.locale || "en-US"}`,
          `--accept-lang=${(config.languages || ["en-US", "en"]).join(",")}`,
        ],
        timezoneId: config.timezone,
      }),
    (child) => lifecycle.own(child),
  );
  if (instrumented) await context.addInitScript({ path: config.interfaceScript });
  const page = context.pages()[0] || (await context.newPage());
  const windowSession = await context.newCDPSession(page);
  const actualVersion = await windowSession.send("Browser.getVersion");
  browserVersion = actualVersion.product;
  browserV8Version = actualVersion.jsVersion;
  const { windowId } = await windowSession.send("Browser.getWindowForTarget");
  const viewport = config.viewport || { width: 1280, height: 800 };
  await windowSession.send("Browser.setWindowBounds", {
    windowId,
    bounds: config.outerWindow || { width: viewport.width, height: viewport.height + 88 },
  });
  await windowSession.detach();
  // Optional intrusive source inventory includes inline/eval/dynamic scripts. Keep the
  // debugger off in baseline runs; it can alter the detector's verdict on its own.
  {
    const cdp = await context.newCDPSession(page);
    networkSession = cdp;
    const { frameTree } = await cdp.send("Page.getFrameTree");
    const mainFrameId = frameTree.frame.id;
    mainNavigation.setScope({ sessionId: "main", frameId: mainFrameId });
    cdp.on("Page.frameNavigated", ({ frame }) => mainNavigation.commit(frame, "main"));
    cdp.on("Network.requestWillBeSent", (params) => mainNavigation.request(params, "main"));
    cdp.on("Network.loadingFailed", (params) => mainNavigation.failed(params, "main"));
    await cdp.send("Page.enable");
    // Network metadata is collected in both passes. This does not enable the
    // Debugger or inject page JavaScript; it still observes through CDP.
    for (const event of responseNetworkEvents) {
      cdp.on(`Network.${event}`, (params) =>
        protocolNetwork.push({
          event,
          timeMs: Date.now(),
          ...(event === "requestWillBeSent"
            ? {
                targetScope:
                  params.frameId === mainFrameId && params.type === "Document"
                    ? "main-frame-navigation"
                    : "unverified-target",
              }
            : {}),
          params,
        }),
      );
    }
    await cdp.send("Network.enable");
    if (config.debugger) {
      cdp.on("Debugger.scriptParsed", (s) => {
        pending.push(
          cdp
            .send("Debugger.getScriptSource", { scriptId: s.scriptId })
            .then(({ scriptSource }) => {
              const hash = createHash("sha256").update(scriptSource).digest("hex");
              writeFileSync(join(out, `script-${hash}.js`), scriptSource, { mode: 0o600 });
              scripts.push({
                url: s.url,
                hash,
                executionContextId: s.executionContextId,
                startLine: s.startLine,
              });
            })
            .catch((e) => collectorError(`script source: ${e}`)),
        );
      });
      await cdp.send("Debugger.enable");
    }
  }
  page.on("framenavigated", (f) =>
    frames.push({ url: f.url(), name: f.name(), main: f === page.mainFrame() }),
  );
  context.on("requestfailed", (r) => errors.push({ url: r.url(), failure: r.failure() }));
  page.on("websocket", (ws) => {
    const row = { url: ws.url(), sent: [], received: [] };
    sockets.push(row);
    ws.on("framesent", (e) => row.sent.push(String(e.payload)));
    ws.on("framereceived", (e) => row.received.push(String(e.payload)));
  });
  context.on("request", (r) => {
    const id = ++sequence;
    requestIds.set(r, id);
    const row = {
      id,
      timeMs: Date.now(),
      timing: r.timing(),
      method: r.method(),
      url: r.url(),
      type: r.resourceType(),
      body: r.postData(),
      cdpTargetScope: "unverified-target",
    };
    try {
      row.cdpTargetScope =
        !r.serviceWorker?.() && r.frame() === page.mainFrame() && r.isNavigationRequest()
          ? "main-frame-navigation"
          : "unverified-target";
    } catch {
      // Dedicated-worker fetch can report its owning main Frame. Only actual
      // navigation requests have independently verified main-frame ownership.
    }
    requests.push(row);
    requestRows.set(r, row);
    pending.push(
      r
        .allHeaders()
        .then((h) => {
          row.headers = h;
        })
        .catch(() => {}),
    );
  });
  context.on("requestfinished", (r) => {
    const row = requestRows.get(r);
    if (row) row.timing = r.timing();
  });
  context.on("response", (r) => {
    const id = ++sequence;
    pending.push(
      (async () => {
        const headers = await r.allHeaders();
        const row = {
          id,
          requestId: requestIds.get(r.request()),
          timeMs: Date.now(),
          url: r.url(),
          status: r.status(),
          headers,
          type: r.request().resourceType(),
        };
        responses.push(row);
        if (["document", "script", "xhr", "fetch"].includes(row.type)) {
          try {
            const body = await r.body();
            row.bodyFile = `${id}-${row.type}.txt`;
            row.bytes = body.length;
            row.bodyHash = createHash("sha256").update(body).digest("hex");
            writeFileSync(join(out, row.bodyFile), body, { mode: 0o600 });
          } catch (e) {
            row.bodyError = String(e);
          }
        }
      })().catch((e) => collectorError(String(e))),
    );
  });
  page.on("pageerror", (e) => errors.push(String(e)));
  const snapshot = async (phase) => {
    const mainNavigationBeforeSnapshot = mainNavigation.snapshot();
    const identity = async () => {
      const { frameTree } = await networkSession.send("Page.getFrameTree");
      return `${frameTree.frame.id}/${frameTree.frame.loaderId}`;
    };
    const { value, retries } = await captureStableDocument(identity, async () => {
      const documentState = JSON.parse(
        await page.evaluate(
          `JSON.stringify({url:location.href,readyState:document.readyState,...(${documentSnapshotState.toString()})(document,${JSON.stringify(config.successSelector || "#rso a h3, #search a h3")},doctype=>new XMLSerializer().serializeToString(doctype))})`,
          undefined,
          false,
        ),
      );
      // Match page.content's doctype serialization in the same DOM read.
      const html = documentState.html;
      writeFileSync(join(out, `${phase}.html`), html, { mode: 0o600 });
      const url = documentState.url;
      const { frameTree } = await networkSession.send("Page.getFrameTree");
      const frame = frameTree.frame;
      const documentResponse = frame.loaderId
        ? protocolNetwork.findLast(
            (row) =>
              row.event === "responseReceived" &&
              row.params.type === "Document" &&
              row.params.frameId === frame.id &&
              row.params.loaderId === frame.loaderId,
          )?.params.response
        : null;
      const childFrames = [];
      if (instrumented)
        for (const frame of page.frames().filter((f) => f !== page.mainFrame())) {
          try {
            childFrames.push({
              url: frame.url(),
              interfaces: await frame.evaluate(
                () => globalThis[Symbol.for("turbo-surf.interface-trace")]?.snapshot(),
                undefined,
                false,
              ),
            });
          } catch (e) {
            childFrames.push({ url: frame.url(), error: String(e) });
          }
        }
      return {
        phase,
        childFrames,
        browserFixture: await page
          .evaluate(
            () => JSON.parse(JSON.stringify(globalThis.__browserFixture || null)),
            undefined,
            false,
          )
          .catch(() => null),
        inputState: await page.evaluate(
          `(()=>{const captureInputState=${inputStateSource}\nreturn captureInputState(document,${JSON.stringify(config.dismissSelector || "#W0wltc")});})()`,
          undefined,
          false,
        ),
        environment: await page
          .evaluate(
            () => ({
              languages: navigator.languages,
              language: navigator.language,
              locale: Intl.DateTimeFormat().resolvedOptions().locale,
              timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
              timeZoneOffset: new Date().getTimezoneOffset(),
              viewport: { width: innerWidth, height: innerHeight },
              outerWindow: { width: outerWidth, height: outerHeight },
              deviceScaleFactor: devicePixelRatio,
              screen: {
                width: screen.width,
                height: screen.height,
                availWidth: screen.availWidth,
                availHeight: screen.availHeight,
              },
            }),
            undefined,
            false,
          )
          .catch((e) => ({ error: String(e) })),
        url,
        status: documentResponse?.status ?? null,
        statusSource: documentResponse ? "cdp-main-document-loader" : "unavailable",
        statusDocumentUrl: documentResponse?.url ?? null,
        readyState: documentState.readyState,
        documentElementPresent: documentState.documentElementPresent,
        bytes: Buffer.byteLength(html),
        results: documentState.results,
        cookies: (await context.cookies()).map((c) => ({ name: c.name, domain: c.domain })),
        interfaces: instrumented
          ? await page
              .evaluate(
                () => globalThis[Symbol.for("turbo-surf.interface-trace")]?.snapshot(),
                undefined,
                false,
              )
              .catch((e) => ({ error: String(e) }))
          : null,
      };
    });
    value.navigationSnapshotRetries = retries;
    value.mainNavigationBeforeSnapshot = mainNavigationBeforeSnapshot;
    pages.push(value);
  };
  await page.goto(config.url, {
    waitUntil: config.waitUntil || "domcontentloaded",
    timeout: 30000,
  });
  if (config.readySelector)
    await page.locator(config.readySelector).waitFor({
      state: "attached",
      timeout: config.readyTimeoutMs ?? 5000,
    });
  await pageDwell("home");
  await snapshot("home");
  if (config.dismissSelector) {
    const present = !!(await page.locator(config.dismissSelector).count());
    dismissal = { selector: config.dismissSelector, present, clicked: false };
    if (present) {
      await page.locator(config.dismissSelector).first().hover();
      await page.locator(config.dismissSelector).first().click();
      dismissal.clicked = true;
      await pageDwell("dismiss");
      await snapshot("dismiss");
    }
  }
  let focusSelector = null,
    typedInput = null;
  const controlCall = (handle, fn, value) => handle.evaluate(fn, value, false);
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
  for (const [i, step] of config.steps.entries()) {
    if (i === observationIndex)
      observationWindow = startObservation(observationMs, i, observationClock);
    lifecycle.throwIfAborted();
    const startedMs = Date.now();
    let inputEvidence;
    if (step.move) await page.locator(step.move.selector).first().hover();
    if (step.click) await page.locator(step.click).first().click();
    if (step.focus) {
      focusSelector = step.focus;
      await page.locator(step.focus).first().focus();
      if (
        !(await page.evaluate(
          (selector) => document.activeElement === document.querySelector(selector),
          step.focus,
        ))
      )
        throw new Error("Focus selector did not become active");
    }
    if (step.type !== undefined) {
      await typedInput?.handle.dispose();
      const handle = (
        await page.evaluateHandle(
          focusSelector
            ? `document.querySelector(${JSON.stringify(focusSelector)})`
            : "document.activeElement",
          undefined,
          false,
        )
      ).asElement();
      if (!handle) throw new Error("Typing control is unavailable");
      typedInput = {
        handle,
        ...(await controlCall(handle, prepareTypedControl, String(step.type))),
      };
      await page.keyboard.type(step.type, { delay: config.inputCadenceMs ?? 110 });
      inputEvidence = await verifyTypedInput();
    }
    if (step.press) {
      if (step.press === "Enter" && typedInput) await verifyTypedInput();
      await page.keyboard.press(step.press);
    }
    if (step.wait) await page.waitForTimeout(step.wait);
    actions.push({
      index: i,
      action: Object.keys(step)[0],
      ...(inputEvidence ? { inputEvidence } : {}),
      completed: true,
      startedMs,
      finishedMs: Date.now(),
    });
    await snapshot(`step-${i}`);
  }
  if (observationWindow)
    await waitForObservation(observationWindow, observationClock, (milliseconds) =>
      page.waitForTimeout(milliseconds),
    );
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
      const raw = await page.evaluate(readFileSync(path, "utf8"), undefined, false);
      probes.push({ path, result: JSON.parse(raw) });
    } catch (e) {
      probes.push({ path, error: String(e) });
    }
  }
  lifecycle.throwIfAborted();
  captureComplete = true;
}
try {
  await lifecycle.race(capture());
} catch (e) {
  if (observationWindow) finishObservation(observationWindow, observationClock, false);
  collectorError({ phase: "capture", error: String(e) });
  process.exitCode = 1;
} finally {
  try {
    // Close only after draining the response/source tasks; closing first loses bodies.
    for (let i = 0; i < pending.length; ) {
      const batch = pending.slice(i);
      i = pending.length;
      if (lifecycle.signal.aborted) break;
      await lifecycle.race(Promise.allSettled(batch)).catch(() => {});
    }
    writeTrace();
    console.log(
      JSON.stringify({
        out,
        instrumented,
        requests: requests.length,
        final: {
          path: new URL(pages.at(-1)?.url || "about:blank").pathname,
          results: pages.at(-1)?.results,
        },
        errors: errors.length,
      }),
    );
  } finally {
    await Promise.race([
      context?.close().catch(() => {}),
      new Promise((done) => setTimeout(done, 500)),
    ]);
    await lifecycle.stopBrowser();
    lifecycle.dispose();
    rmSync(profile, { recursive: true, force: true });
  }
}

function writeTrace() {
  enrichProtocolHeaders(requests, protocolNetwork);
  enrichResponseTransport(requests, responses, protocolNetwork);
  writeFileSync(
    join(out, "trace.json"),
    JSON.stringify(
      {
        runtime: "chrome",
        backend: "patchright",
        instrumented,
        captureComplete: captureComplete && collectorErrors.length === 0,
        collectorErrors,
        actions,
        dwell,
        observation: observationWindow?.record ?? null,
        dismissal,
        startedAt,
        browserVersion,
        browserV8Version,
        headless,
        executable: executable || "installed-channel:chrome",
        config,
        pages,
        requests,
        responses: responses.sort((a, b) => a.id - b.id),
        scripts,
        protocolNetwork,
        networkCoverage: {
          source: "cdp-network",
          scope: "main-page-session",
          targets: "main page CDP session; child targets are not independently attached",
          responseAssociation:
            "verified main-frame navigation scopes with one-to-one unique URL/method; subresources and other responses stay unavailable",
          enabledInBaseline: true,
          packetCapture: false,
          responseMetadata:
            "protocol, securityDetails, timing, connection and cache state; unavailable when no unambiguous CDP response matches",
        },
        sockets,
        frames,
        probes,
        errors,
      },
      null,
      2,
    ),
    { mode: 0o600 },
  );
}
