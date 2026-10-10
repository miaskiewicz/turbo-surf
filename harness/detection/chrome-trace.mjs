// Fresh Chrome oracle; no cookies leave this process or feed the native attempt.
import { enrichProtocolHeaders } from "./protocol-headers.mjs";
import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
const require = createRequire(
  new URL("../../scripts/browser-sidecar/package.json", import.meta.url),
);
const { chromium } = require("patchright");
const config = JSON.parse(readFileSync(process.argv[2], "utf8"));
const out = resolve(process.argv[3]);
const instrumented = config.interfaces;
mkdirSync(out, { recursive: true });
const profile = mkdtempSync(join(tmpdir(), "turbo-search-oracle-"));
let context;
const requests = [],
  responses = [],
  errors = [],
  pages = [],
  pending = [];
const scripts = [],
  sockets = [],
  frames = [],
  probes = [];
const protocolNetwork = [];
const requestIds = new WeakMap();
const requestRows = new WeakMap();
const startedAt = new Date().toISOString();
let sequence = 0;
try {
  context = await chromium.launchPersistentContext(profile, {
    channel: "chrome",
    headless: false,
    viewport: null,
    locale: config.locale,
    timezoneId: config.timezone,
  });
  if (instrumented) await context.addInitScript({ path: config.interfaceScript });
  const page = context.pages()[0] || (await context.newPage());
  // Optional intrusive source inventory includes inline/eval/dynamic scripts. Keep the
  // debugger off in baseline runs; it can alter the detector's verdict on its own.
  if (instrumented || config.debugger) {
    const cdp = await context.newCDPSession(page);
    if (instrumented) {
      for (const event of [
        "requestWillBeSent",
        "requestWillBeSentExtraInfo",
        "responseReceivedExtraInfo",
        "loadingFailed",
      ]) {
        cdp.on(`Network.${event}`, (params) =>
          protocolNetwork.push({ event, timeMs: Date.now(), params }),
        );
      }
      await cdp.send("Network.enable");
    }
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
            .catch((e) => errors.push(`script source: ${e}`)),
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
    };
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
      })().catch((e) => errors.push(String(e))),
    );
  });
  page.on("pageerror", (e) => errors.push(String(e)));
  const snapshot = async (phase) => {
    let html;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        html = await page.content();
        break;
      } catch (e) {
        if (attempt === 2) {
          pages.push({ phase, url: page.url(), error: String(e) });
          return;
        }
        await page.waitForTimeout(200);
      }
    }
    writeFileSync(join(out, `${phase}.html`), html, { mode: 0o600 });
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
    pages.push({
      phase,
      childFrames,
      url: page.url(),
      bytes: Buffer.byteLength(html),
      results: await page
        .locator(config.successSelector || "#rso a h3, #search a h3")
        .count()
        .catch(() => null),
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
    });
  };
  await page.goto(config.url, { waitUntil: "domcontentloaded", timeout: 30000 });
  await page.waitForTimeout(config.waitMs);
  await snapshot("home");
  for (const [i, step] of config.steps.entries()) {
    if (step.move) await page.locator(step.move.selector).first().hover();
    if (step.click) await page.locator(step.click).first().click();
    if (step.focus) await page.locator(step.focus).first().focus();
    if (step.type !== undefined) await page.keyboard.type(step.type, { delay: 110 });
    if (step.press) await page.keyboard.press(step.press);
    if (step.wait) await page.waitForTimeout(step.wait);
    await snapshot(`step-${i}`);
  }
  await page.waitForTimeout(config.waitMs);
  await snapshot("search");
  for (const path of config.probes || []) {
    try {
      const raw = await page.evaluate(readFileSync(path, "utf8"), undefined, false);
      probes.push({ path, result: JSON.parse(raw) });
    } catch (e) {
      probes.push({ path, error: String(e) });
    }
  }
} catch (e) {
  errors.push({ phase: "capture", error: String(e) });
  process.exitCode = 1;
} finally {
  // Close only after draining the response/source tasks; closing first loses bodies.
  for (let i = 0; i < pending.length; ) {
    const batch = pending.slice(i);
    i = pending.length;
    await Promise.allSettled(batch);
  }
  enrichProtocolHeaders(requests, protocolNetwork);
  writeFileSync(
    join(out, "trace.json"),
    JSON.stringify(
      {
        runtime: "chrome",
        instrumented,
        startedAt,
        browserVersion: context?.browser()?.version(),
        config,
        pages,
        requests,
        responses: responses.sort((a, b) => a.id - b.id),
        scripts,
        protocolNetwork,
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
  await context?.close();
  rmSync(profile, { recursive: true, force: true });
}
