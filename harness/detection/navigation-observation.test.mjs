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
        response.end("<!doctype html><body>Terminal fixture</body>");
      } else if (url.pathname === "/search") {
        if (fixture.kind.startsWith("slow"))
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
            `<!doctype html><body>Shell<script>globalThis.__browserFixture={timerCreatedDate:Date.now(),timerCreatedPerformance:performance.now()};setTimeout(()=>{globalThis.__browserFixture.timerFiredDate=Date.now();globalThis.__browserFixture.timerFiredPerformance=performance.now();location.href='/terminal';},${fixture.delay})</script>`,
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
          status: 200,
          results: 0,
        },
        {
          name: "retry-common",
          kind: "retry",
          delay: 600,
          observationMs: 1000,
          status: 429,
          results: 0,
        },
        {
          name: "retry-fast-boundary",
          kind: "retry",
          delay: 20,
          observationMs: 100,
          boundary: true,
          results: 0,
        },
        {
          name: "retry-fast-common",
          kind: "retry",
          delay: 20,
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
          if (!fixture.boundary) assert.equal(observation.completed, true, label);
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
          if (!fixture.boundary) {
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
          if (fixture.boundary) {
            assert([200, 429].includes(final.status), label);
            assert.equal(chromeSearchOutcome(trace).accepted, false, label);
            const clock = trace.pages.find(
              (page) => page.browserFixture?.timerCreatedDate,
            )?.browserFixture;
            // The timer starts after navigation/bootstrap, which can consume the
            // short window in either browser. Keep the actual boundary outcome.
            console.log(
              "Boundary observation",
              JSON.stringify({
                runtime,
                status: final.status,
                completed: observation.completed,
                elapsedMs: observation.elapsedMs,
                overrunMs: observation.overrunMs,
                pendingNavigation: !!observation.pendingNavigation,
                timerCreatedAfterEnterMs: clock
                  ? clock.timerCreatedDate - trace.actions[5].startedMs
                  : null,
                timerFiredRecorded: typeof clock?.timerFiredDate === "number",
              }),
            );
          } else assert.equal(final.status, fixture.status, label);
          assert.equal(final.results, fixture.results, label);
          if (fixture.kind === "background")
            assert(["interactive", "complete"].includes(final.readyState), label);
          else assert.equal(final.readyState, "complete", label);
          assert.equal(
            new URL(final.url).pathname,
            fixture.finalPath || (final.status === 429 ? "/terminal" : "/search"),
            label,
          );
          const seen = events.filter((event) => event.fixture === own);
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
            if (runtime === "chrome")
              assert(
                trace.pages.some(
                  (page) => page.mainNavigationBeforeSnapshot?.pendingNavigation === true,
                ),
                label,
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
          if (fixture.name === "retry-short")
            assert.equal(
              seen.some((event) => event.path === "/terminal"),
              false,
              label,
            );
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
