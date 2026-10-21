import { test } from "node:test";
import { superviseTestChild } from "./child-supervision.mjs";
import assert from "node:assert/strict";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { inputCadenceMs, searchSteps } from "./search-plan.mjs";
import { chromeSearchOutcome, observationSummary } from "./compare.mjs";

const binary = process.env.TURBO_SURF_DETECTION_BINARY;
const chrome =
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const here = fileURLToPath(new URL(".", import.meta.url));

async function collect(t, runtime, config, directory) {
  const configPath = join(directory, "config.json");
  writeFileSync(configPath, JSON.stringify(config));
  for (const name of ["stdout", "stderr", "collector"])
    writeFileSync(join(directory, `${name}.log`), "", { mode: 0o600 });
  const result = await superviseTestChild(
    t,
    runtime === "chrome" ? process.execPath : binary,
    runtime === "chrome"
      ? [join(here, "chrome-cdp-trace.mjs"), configPath, directory]
      : [configPath, directory],
    {
      timeoutMs: 45000,
      spawnOptions: {
        env: { ...process.env, TURBO_SURF_BROWSER_CMD: "", TURBO_SURF_BROWSER_FETCH_CMD: "" },
      },
      onOutput: (name, bytes) => {
        appendFileSync(join(directory, `${name}.log`), bytes);
        appendFileSync(join(directory, "collector.log"), bytes);
      },
    },
  );
  if (result.code !== 0 || result.error || result.timedOut || result.aborted)
    throw new Error(`${runtime} ${result.code}/${result.signal}: ${JSON.stringify(result)}`);
  return JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
}

function assertDocumentResponse(runtime, trace, output, final, label) {
  if (runtime === "chrome") {
    assert.equal(final.statusSource, "cdp-main-document-loader", label);
    assert.equal(final.statusDocumentUrl, final.url, label);
    assert(
      trace.responses.some(
        (response) =>
          response.type === "document" &&
          response.url === final.url &&
          response.status === final.status,
      ),
      label,
    );
  } else {
    const network = readFileSync(join(output, "network.jsonl"), "utf8")
      .trim()
      .split("\n")
      .map(JSON.parse);
    assert(
      network.some(
        (response) =>
          response.event === "response" &&
          (response.final_url || response.url) === final.url &&
          response.status === final.status &&
          network.some(
            (request) =>
              request.event === "request" &&
              request.id === response.id &&
              request.explicit_headers?.["sec-fetch-dest"] === "document",
          ),
      ),
      label,
    );
  }
}

function assertRetryOutcome(runtime, fixture, trace, output, seen, label) {
  const observation = trace.observation;
  const final = trace.pages.at(-1);
  const finalUrl = new URL(final.url);
  const unresolved = !!(
    observation.pendingNavigation ||
    observation.navigationFailed ||
    observation.navigationWaitTimedOut ||
    observation.navigationHopLimit
  );
  assert([200, 429].includes(final.status), label);
  assert.equal(final.results, 0, label);
  assert.equal(chromeSearchOutcome(trace).accepted, false, label);
  if (unresolved) {
    assert.equal(observation.completed, false, label);
    assert.equal(observationSummary(trace).complete, false, label);
  } else assert.equal(observation.completed, true, label);
  assert.equal(observationSummary(trace).complete, !unresolved, label);
  assertDocumentResponse(runtime, trace, output, final, label);
  const terminalEvents = seen.filter((event) => event.path === "/terminal");
  for (const event of terminalEvents) {
    const clock = event.timerEvidence;
    assert.equal(clock.delayMs, fixture.delay, label);
    for (const source of ["Date", "Performance"]) {
      assert(Number.isFinite(clock[`timerCreated${source}`]), label);
      assert(Number.isFinite(clock[`timerFired${source}`]), label);
      assert(
        clock[`timerFired${source}`] - clock[`timerCreated${source}`] >= fixture.delay,
        `${label}: ${source} timer fired before ${fixture.delay} ms`,
      );
    }
  }
  const clock = final.browserFixture;
  assert.equal(clock?.delayMs, fixture.delay, label);
  assert(Number.isFinite(clock.timerCreatedDate), label);
  assert(Number.isFinite(clock.timerCreatedPerformance), label);
  if (final.status === 429) {
    assert.equal(finalUrl.pathname, "/terminal", label);
    const terminal = terminalEvents.find(
      (event) => event.requestUrl === finalUrl.pathname + finalUrl.search,
    );
    assert(terminal, label);
    assert.equal(terminal.status, 429, label);
    assert.deepEqual(clock, terminal.timerEvidence, label);
  } else {
    assert.equal(finalUrl.pathname, "/search", label);
    assert(
      seen.some(
        (event) => event.requestUrl === finalUrl.pathname + finalUrl.search && event.status === 200,
      ),
      label,
    );
    // A server response alone is not a committed document. But a known earlier
    // navigation must either be reflected in the final document or be pending.
    const earlierTerminal = terminalEvents.some(
      (event) => event.timeMs < observation.snapshotStartedMs,
    );
    const recordedFire = Number.isFinite(clock.timerFiredDate);
    if (earlierTerminal || recordedFire)
      assert(unresolved, `${label}: untracked terminal navigation`);
  }
  console.log(
    "Retry observation",
    JSON.stringify({
      fixture: fixture.name,
      runtime,
      status: final.status,
      completed: observation.completed,
      requestedMs: observation.requestedMs,
      deadlineMs: observation.deadlineMs,
      elapsedMs: observation.elapsedMs,
      overrunMs: observation.overrunMs,
      pendingNavigation: !!observation.pendingNavigation,
      timerCreatedAfterEnterMs: clock.timerCreatedDate - trace.actions[5].startedMs,
      timerFiredAfterCreationMs: Number.isFinite(clock.timerFiredDate)
        ? clock.timerFiredDate - clock.timerCreatedDate
        : null,
      clock,
    }),
  );
}

test("retry evidence rejects stale documents, fabricated terminals and early timers", () => {
  const fixture = { name: "causal-control", delay: 600 };
  const clock = { delayMs: 600, timerCreatedDate: 1000, timerCreatedPerformance: 0 };
  const base = {
    captureComplete: true,
    collectorErrors: [],
    errors: [],
    config: { url: "http://fixture.test/", observationMs: 1000 },
    actions: Array.from({ length: 6 }, () => ({ startedMs: 900 })),
    observation: {
      policy: "fixed-deadline-before-enter",
      requestedMs: 1000,
      completed: true,
      elapsedMs: 1000,
      overrunMs: 0,
      deadlineMs: 1900,
      snapshotStartedMs: 1900,
      pendingNavigation: false,
    },
    pages: [
      {
        phase: "search",
        url: "http://fixture.test/search?q=fixture",
        status: 200,
        results: 0,
        readyState: "complete",
        statusSource: "cdp-main-document-loader",
        statusDocumentUrl: "http://fixture.test/search?q=fixture",
        browserFixture: clock,
      },
    ],
    responses: [{ type: "document", url: "http://fixture.test/search?q=fixture", status: 200 }],
  };
  const shell = { path: "/search", requestUrl: "/search?q=fixture", status: 200 };
  const fired = { ...clock, timerFiredDate: 1600, timerFiredPerformance: 600 };
  const terminal = {
    path: "/terminal",
    requestUrl: "/terminal?timer=record",
    status: 429,
    timeMs: 1700,
    timerEvidence: fired,
  };
  const check = (trace, events = [shell]) =>
    assertRetryOutcome("chrome", fixture, trace, "", events, fixture.name);
  check(structuredClone(base));
  const pending = structuredClone(base);
  pending.observation.pendingNavigation = true;
  pending.observation.completed = false;
  pending.pages[0].browserFixture = fired;
  check(pending, [shell, terminal]);
  assert.throws(() => check(structuredClone(base), [shell, terminal]), /untracked terminal/);
  for (const mutate of [
    (trace) => {
      trace.pages[0].results = 1;
    },
    (trace) => {
      trace.collectorErrors.push({ message: "collector failure" });
    },
    (trace) => {
      trace.observation.completed = false;
    },
    (trace) => {
      trace.pages[0].statusDocumentUrl = "http://fixture.test/old";
    },
    (trace) => {
      trace.observation.pendingNavigation = true;
    },
    (trace) => {
      trace.observation.navigationFailed = true;
    },
  ]) {
    const trace = structuredClone(base);
    mutate(trace);
    assert.throws(() => check(trace));
  }
  const committed = structuredClone(base);
  Object.assign(committed.pages[0], {
    url: "http://fixture.test/terminal?timer=record",
    status: 429,
    statusDocumentUrl: "http://fixture.test/terminal?timer=record",
    browserFixture: fired,
  });
  committed.responses.push({ type: "document", url: committed.pages[0].url, status: 429 });
  check(committed, [shell, terminal]);
  assert.throws(() => check(committed, [shell]));
  const missingResponse = structuredClone(committed);
  missingResponse.responses.pop();
  assert.throws(() => check(missingResponse, [shell, terminal]));
  const missingClock = structuredClone(committed);
  delete missingClock.pages[0].browserFixture.timerFiredDate;
  assert.throws(() => check(missingClock, [shell, terminal]));
  assert.throws(
    () =>
      check(committed, [
        shell,
        { ...terminal, timerEvidence: { ...fired, timerFiredPerformance: 599 } },
      ]),
    /timer fired before/,
  );
});

test(
  "both collectors use a fixed Enter window for slow forms and delayed retries",
  {
    skip: !binary || !existsSync(binary) || !existsSync(chrome) || typeof WebSocket === "undefined",
    timeout: 240000,
  },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "detection-navigation-observation-"));
    const events = [];
    const timers = new Set();
    const respondLater = (response, milliseconds, callback) => {
      const timer = setTimeout(() => {
        timers.delete(timer);
        callback();
      }, milliseconds);
      timers.add(timer);
      response.once("close", () => {
        clearTimeout(timer);
        timers.delete(timer);
      });
    };
    let active;
    let complete = false;
    const server = createServer((request, response) => {
      const url = new URL(request.url, "http://127.0.0.1");
      const fixture = active;
      const event = {
        fixture,
        path: url.pathname,
        requestUrl: request.url,
        timeMs: Date.now(),
        query: url.searchParams.get("q"),
      };
      events.push(event);
      response.on("finish", () => {
        event.finishedMs = Date.now();
        event.status = response.statusCode;
      });
      if (url.pathname === "/consent") {
        response.writeHead(204, { "Set-Cookie": "consent=fixture; Path=/" });
        response.end();
        return;
      }
      if (url.pathname === "/audit") {
        event.audit = JSON.parse(url.searchParams.get("state"));
        response.writeHead(204);
        response.end();
        return;
      }
      response.setHeader("Content-Type", "text/html; charset=utf-8");
      if (url.pathname.startsWith("/background-")) {
        respondLater(response, 1000, () => response.end("<!doctype html><p>Background</p>"));
      } else if (url.pathname === "/terminal") {
        response.statusCode = 429;
        event.timerEvidence = url.searchParams.has("timer")
          ? JSON.parse(url.searchParams.get("timer"))
          : null;
        response.end(
          `<!doctype html><body>Terminal fixture<script>globalThis.__browserFixture=${JSON.stringify(event.timerEvidence).replaceAll("<", "\\u003c")}</script>`,
        );
      } else if (url.pathname === "/search") {
        if (fixture.kind === "redirect") {
          response.writeHead(302, { Location: "/terminal" });
          response.end();
        } else if (fixture.kind.startsWith("slow"))
          respondLater(response, 600, () => {
            response.statusCode = fixture.kind === "slow-challenge" ? 429 : 200;
            response.end(
              fixture.kind === "slow-challenge"
                ? "<!doctype html><p>Challenge</p>"
                : '<!doctype html><div id="rso"><a><h3>Result</h3></a></div>',
            );
          });
        else if (fixture.kind === "background")
          response.end(`<!doctype html>
          <div id="rso"><a><h3>Result</h3></a></div><iframe src="/background-frame"></iframe>
          <script>fetch('/background-xhr')</script>`);
        else
          response.end(
            `<!doctype html><body>Shell<script>
              const state=()=>({visibility:document.visibilityState??null,hidden:document.hidden??null,
                focused:typeof document.hasFocus==='function'?document.hasFocus():null});
              const clock=globalThis.__browserFixture={delayMs:${fixture.delay},timerCreatedDate:Date.now(),
                timerCreatedPerformance:performance.now(),createdState:state()};
              setTimeout(()=>{clock.timerFiredDate=Date.now();clock.timerFiredPerformance=performance.now();
                clock.firedState=state();location.href='/terminal?timer='+encodeURIComponent(JSON.stringify(clock));},${fixture.delay});
            </script>`,
          );
      } else
        response.end(`<!doctype html><body data-input-ready>
      ${fixture.oldResult ? '<div id="rso"><a><h3>Old result</h3></a></div>' : ""}
      <button id="dismiss">Reject</button><input name="q" type="hidden" value="hidden">
      <form action="/search"><textarea name="q"></textarea><button>Search</button></form>
      <script>
        const form=document.querySelector('form'),field=document.querySelector('textarea');
        const keys=[];let trustedInputs=0;
        const record=e=>keys.push({key:e.key,type:e.type,trusted:e.isTrusted,
          timeStamp:e.timeStamp,performanceNow:performance.now(),dateNow:Date.now()});
        field.addEventListener('keydown',record);field.addEventListener('keyup',record);
        field.oninput=e=>{if(e.isTrusted)trustedInputs++;};
        field.onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();if(${!!fixture.scriptEnter}){form.onsubmit(e);location.href='/search?q='+encodeURIComponent(field.value);}else form.requestSubmit();}};
        document.querySelector('#dismiss').onclick=async e=>{
          await fetch('/consent',{method:'POST'});e.target.remove();
        };
        form.onsubmit=e=>fetch('/audit?state='+encodeURIComponent(JSON.stringify({keys,
          trustedInputs,submitTrusted:e.isTrusted,eventType:e.type,value:field.value,focused:document.activeElement===field})));
      </script>`);
    });
    try {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
      const origin = `http://127.0.0.1:${server.address().port}`;
      for (const fixture of [
        {
          name: "slow-short",
          kind: "slow",
          observationMs: 100,
          status: 200,
          results: 1,
          overrun: true,
        },
        { name: "slow-common", kind: "slow", observationMs: 1000, status: 200, results: 1 },
        {
          name: "retry-short",
          kind: "retry",
          delay: 600,
          observationMs: 100,
          results: 0,
        },
        {
          name: "retry-common",
          kind: "retry",
          delay: 600,
          observationMs: 1000,
          results: 0,
        },
        {
          name: "retry-fast-boundary",
          kind: "retry",
          delay: 20,
          observationMs: 100,
          results: 0,
        },
        {
          name: "retry-fast-common",
          kind: "retry",
          delay: 20,
          observationMs: 1000,
          results: 0,
        },
        {
          name: "immediate-redirect",
          kind: "redirect",
          observationMs: 1000,
          status: 429,
          results: 0,
        },
        {
          name: "script-enter",
          kind: "slow",
          scriptEnter: true,
          observationMs: 1000,
          status: 200,
          results: 1,
        },
        {
          name: "provisional-old-serp",
          kind: "slow-challenge",
          oldResult: true,
          observationMs: 100,
          status: 429,
          results: 0,
          overrun: true,
          finalPath: "/search",
        },
        {
          name: "background-scoping",
          kind: "background",
          chromeOnly: true,
          observationMs: 100,
          status: 200,
          results: 1,
        },
      ]) {
        for (const runtime of fixture.chromeOnly ? ["chrome"] : ["chrome", "native"]) {
          active = { ...fixture, runtime };
          const own = active;
          const output = join(directory, `${fixture.name}-${runtime}`);
          const config = {
            url: origin + (fixture.oldResult ? "/existing-search" : "/"),
            interfaces: false,
            chromePath: chrome,
            chromeIncognito: true,
            disableExtensions: true,
            readySelector: "[data-input-ready]",
            dismissSelector: "#dismiss",
            waitMs: 100,
            observationMs: fixture.observationMs,
            inputCadenceMs,
            pageBudgetMs: 5000,
            requireNativeFormNavigation: !fixture.scriptEnter,
            disableFormFallback: true,
            steps: searchSteps("fixture query"),
          };
          // The collector creates its output folder; the configuration must exist first.
          mkdirSync(output);
          const trace = await collect(t, runtime, config, output);
          const label = `${fixture.name}/${runtime}`;
          assert.equal(trace.captureComplete, true, `${label}: ${JSON.stringify(trace.errors)}`);
          assert.equal(trace.actions.length, 6, label);
          assert(
            trace.actions.every((action) => action.completed === true),
            label,
          );
          const observation = trace.observation;
          assert.equal(observation.policy, "fixed-deadline-before-enter", label);
          assert.equal(observation.triggerStepIndex, 5, label);
          assert.equal(observation.requestedMs, fixture.observationMs, label);
          if (fixture.kind !== "retry") assert.equal(observation.completed, true, label);
          else assert.equal(typeof observation.completed, "boolean", label);
          assert(observation.elapsedMs >= fixture.observationMs, label);
          assert(observation.overrunMs >= 0, label);
          assert(observation.startedMs <= trace.actions[5].startedMs, label);
          assert.equal(
            observation.deadlineMs,
            observation.startedMs + fixture.observationMs,
            label,
          );
          assert.equal(
            trace.dwell.some((span) => span.stage === "search"),
            false,
            label,
          );
          if (fixture.kind !== "retry") {
            assert.notEqual(observation.navigationWaitTimedOut, true, label);
            assert.notEqual(observation.navigationHopLimit, true, label);
            assert(!observation.pendingNavigation, label);
          } else if (
            observation.pendingNavigation ||
            observation.navigationFailed ||
            observation.navigationWaitTimedOut ||
            observation.navigationHopLimit
          ) {
            assert.equal(observation.completed, false, label);
            assert.equal(observationSummary(trace).complete, false, label);
          }
          if (fixture.overrun) assert(observation.overrunMs >= 400, label);
          const final = trace.pages.at(-1);
          const seen = events.filter((event) => event.fixture === own);
          if (fixture.kind === "retry")
            assertRetryOutcome(runtime, fixture, trace, output, seen, label);
          else assert.equal(final.status, fixture.status, label);
          if (fixture.kind === "redirect") {
            assertDocumentResponse(runtime, trace, output, final, label);
            assert.equal(chromeSearchOutcome(trace).accepted, false, label);
            assert(
              seen.some((event) => event.path === "/terminal" && event.status === 429),
              label,
            );
          }
          assert.equal(final.results, fixture.results, label);
          if (fixture.kind === "background")
            assert(["interactive", "complete"].includes(final.readyState), label);
          else assert.equal(final.readyState, "complete", label);
          assert.equal(
            new URL(final.url).pathname,
            fixture.finalPath || (final.status === 429 ? "/terminal" : "/search"),
            label,
          );
          const search = seen.find((event) => event.path === "/search");
          const consent = seen.find((event) => event.path === "/consent");
          const audit = seen.find((event) => event.audit)?.audit;
          assert.equal(search.query, "fixture query", label);
          assert.equal(consent.status, 204, label);
          assert(consent.finishedMs < search.timeMs, label);
          assert.equal(audit.value, "fixture query", label);
          assert.equal(audit.submitTrusted, true, label);
          assert.equal(audit.eventType, fixture.scriptEnter ? "keydown" : "submit", label);
          assert.equal(audit.trustedInputs, 13, label);
          assert.equal(audit.focused, true, label);
          assert(
            audit.keys.every((key) => key.trusted),
            label,
          );
          const characters = audit.keys.filter((key) => key.key !== "Enter");
          assert.equal(characters.length, 26, label);
          const typing = trace.actions.find((action) => action.action === "type");
          const hostSpan = typing.finishedMs - typing.startedMs;
          assert(hostSpan >= 13 * inputCadenceMs, label);
          for (const clock of ["timeStamp", "performanceNow", "dateNow"]) {
            const guestSpan = characters.at(-1)[clock] - characters[0][clock];
            // Scheduler/protocol overhead is allowed; a second virtual 110 ms per
            // character would exceed the containing host action by over a second.
            assert(
              guestSpan <= hostSpan + 250,
              `${label}/${clock}: guest ${guestSpan}, host ${hostSpan}`,
            );
          }
          if (fixture.oldResult) {
            assert.equal(trace.pages.find((page) => page.phase === "home").results, 1, label);
            assertDocumentResponse(runtime, trace, output, final, label);
            assert.equal(observationSummary(trace).complete, true, label);
            // The request event can arrive after the pre-evaluation tracker
            // sample while snapshot evaluation waits for the new document.
            console.log(
              "Old result replacement",
              JSON.stringify({
                runtime,
                pendingSnapshotCount: trace.pages.filter(
                  (page) => page.mainNavigationBeforeSnapshot?.pendingNavigation === true,
                ).length,
                enterFinishedMs: trace.actions[5].finishedMs,
                searchRequestedMs: search.timeMs,
                searchResponseFinishedMs: search.finishedMs,
                finalSnapshotStartedMs: observation.snapshotStartedMs,
                finalStatus: final.status,
              }),
            );
          }
          if (fixture.kind === "background") {
            assert(
              seen.some((event) => event.path === "/background-xhr"),
              label,
            );
            assert(
              seen.some((event) => event.path === "/background-frame"),
              label,
            );
            assert.equal(observation.pendingNavigation, false, label);
            assert.equal(observation.navigationFailed, false, label);
          }
          if (runtime === "chrome") {
            assert.equal(trace.browserContext.createdContextVerified, true, label);
            assert.equal(trace.browserContext.nonDefaultContext, true, label);
          } else {
            assert.equal(trace.windowTimerMode, "real-time", label);
            assert.equal(
              trace.navigationSource,
              fixture.scriptEnter ? "script" : "native-form",
              label,
            );
            assert.equal(trace.formFallback, null, label);
          }
        }
      }
      complete = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      for (const timer of timers) clearTimeout(timer);
      writeFileSync(join(directory, "server-events.json"), JSON.stringify(events, null, 2));
      if (complete) rmSync(directory, { recursive: true, force: true });
      else console.error(`Navigation observation captures retained at ${directory}`);
    }
  },
);
