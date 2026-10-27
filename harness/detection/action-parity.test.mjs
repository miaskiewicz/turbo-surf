import { test } from "node:test";
import { superviseTestChild } from "./child-supervision.mjs";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import {
  appendFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { inputCadenceMs, searchSelector, searchSteps } from "./search-plan.mjs";
import { interactionSummary } from "./compare.mjs";

const binary = process.env.TURBO_SURF_DETECTION_BINARY;
const chrome =
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const here = fileURLToPath(new URL(".", import.meta.url));
async function run(t, command, args) {
  const directory = args.at(-1);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  for (const name of ["stdout", "stderr"])
    writeFileSync(join(directory, `${name}.log`), "", { mode: 0o600 });
  const result = await superviseTestChild(t, command, args, {
    timeoutMs: 45000,
    onOutput: (name, bytes) => appendFileSync(join(directory, `${name}.log`), bytes),
  });
  const output = result.stdout + result.stderr;
  if (result.code !== 0 || result.error || result.timedOut || result.aborted)
    throw new Error(`${result.code}: ${output}\n${JSON.stringify(result)}`);
  return output;
}

test("the common search plan excludes hidden query fields", () => {
  const steps = searchSteps("fixture query");
  assert.deepEqual(steps, [
    { move: { selector: searchSelector } },
    { click: searchSelector },
    { focus: searchSelector },
    { type: "fixture query" },
    { wait: 350 },
    { press: "Enter" },
  ]);
  assert.equal(inputCadenceMs, 110);
  assert.match(searchSelector, /:not\(\[type="hidden" i\]\)/);
});

test(
  "Chrome and native execute the same consent, query and form stages",
  {
    skip: !binary || !existsSync(binary) || !existsSync(chrome) || typeof WebSocket === "undefined",
    timeout: 180000,
  },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "detection-action-parity-"));
    let complete = false;
    const requests = [];
    const server = createServer((request, response) => {
      const url = new URL(request.url, "http://127.0.0.1");
      requests.push({
        path: url.pathname,
        params: url.searchParams,
        cookie: request.headers.cookie,
      });
      if (url.pathname === "/consent") {
        response.writeHead(204, { "Set-Cookie": "consent=fixture; Path=/" });
        response.end();
        return;
      }
      response.setHeader("Content-Type", "text/html; charset=utf-8");
      if (url.pathname === "/search") {
        response.end(
          '<!doctype html><div id="rso"><a href="/result"><h3>Fixture result</h3></a></div>',
        );
        return;
      }
      response.end(`<!doctype html><style>
        #consent { margin: 40px; } textarea { display: block; margin: 20px; }
      </style><body>
        <div id="consent"><button type="button" id="dismiss">Reject all</button></div>
        <input type="hidden" name="q" value="unrelated-hidden-field">
        <form action="/search"><textarea name="q" disabled></textarea>
          <input name="audit" type="hidden"><button type="submit">Search</button>
        </form><script>
          const field=document.querySelector('textarea'),form=document.querySelector('form');
          const audit={keys:[],input:[],consent:null,ready:false};
          document.querySelector('#dismiss').onclick=async event=>{
            const r=event.currentTarget.getBoundingClientRect();
            audit.consent={trusted:event.isTrusted,inside:event.clientX>=r.left&&event.clientX<=r.right&&event.clientY>=r.top&&event.clientY<=r.bottom};
            await fetch('/consent',{method:'POST'});
            document.querySelector('#consent').style.display='none';
          };
          field.onkeydown=event=>{
            audit.keys.push({key:event.key,trusted:event.isTrusted,focused:document.activeElement===field});
            if(event.key==='Enter'){event.preventDefault();form.requestSubmit();}
          };
          field.oninput=event=>audit.input.push({data:event.data,trusted:event.isTrusted});
          form.onsubmit=()=>{document.querySelector('[name=audit]').value=JSON.stringify(audit);};
          setTimeout(()=>{field.disabled=false;document.body.setAttribute('data-input-ready','');audit.ready=true;},150);
        </script>`);
    });
    try {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
      const origin = `http://127.0.0.1:${server.address().port}`;
      for (const interfaces of [false, true]) {
        const config = {
          url: origin + "/",
          interfaces,
          debugger: false,
          chromePath: chrome,
          chromeIncognito: true,
          disableExtensions: true,
          // A headed Chrome takes the OS keyboard focus, and keys typed on the host
          // then corrupt the trusted query (turbo-surf#52).
          headless: true,
          interfaceScript: join(here, "interface-trace.js"),
          readySelector: "[data-input-ready]",
          dismissSelector: "#dismiss",
          waitMs: 200,
          inputCadenceMs,
          pageBudgetMs: 5000,
          requireNativeFormNavigation: true,
          disableFormFallback: true,
          steps: searchSteps("fixture query"),
        };
        const configPath = join(directory, `config-${interfaces}.json`);
        writeFileSync(configPath, JSON.stringify(config));
        for (const runtime of ["chrome", "native"]) {
          const start = requests.length;
          const output = join(directory, `${runtime}-${interfaces}`);
          await run(
            t,
            runtime === "chrome" ? process.execPath : binary,
            runtime === "chrome"
              ? [join(here, "chrome-cdp-trace.mjs"), configPath, output]
              : [configPath, output],
          );
          const trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
          assert.equal(trace.captureComplete, true, JSON.stringify(trace.errors));
          assert.deepEqual(
            trace.actions.map(({ index, action, completed }) => ({ index, action, completed })),
            config.steps.map((step, index) => ({
              index,
              action: Object.keys(step)[0],
              completed: true,
            })),
          );
          for (const index of config.steps.keys())
            assert(trace.pages.some((page) => page.phase === `step-${index}`));
          assert.equal(interactionSummary(trace).configuredActionsExecuted, true);
          for (const stage of ["home"]) {
            const span = trace.dwell.find((row) => row.stage === stage);
            assert.equal(span.completed, true);
            assert.equal(span.requestedMs, config.waitMs);
            assert(span.finishedMs - span.startedMs >= config.waitMs);
          }
          assert.equal(trace.observation.policy, "fixed-deadline-before-enter");
          assert.equal(trace.observation.completed, true);
          assert.equal(trace.observation.requestedMs, config.waitMs);
          assert(trace.observation.elapsedMs >= config.waitMs);
          assert(trace.observation.overrunMs >= 0);
          const entered = trace.actions.find((action) => action.action === "press");
          assert(trace.observation.startedMs <= entered.startedMs);
          assert.equal(trace.observation.deadlineMs, trace.observation.startedMs + config.waitMs);
          assert.equal(
            trace.dwell.some((span) => span.stage === "search"),
            false,
          );
          assert.equal(trace.dismissal.clicked, true);
          if (runtime === "chrome") {
            assert.deepEqual(trace.effectiveExtensionFlags, [
              "--disable-extensions",
              "--disable-component-extensions-with-background-pages",
              "--headless=new",
            ]);
            assert.equal(trace.browserContext.requestedIncognito, true);
            assert.equal(trace.browserContext.nonDefaultContext, true);
            assert.equal(trace.browserContext.createdContextVerified, true);
          }
          const seen = requests.slice(start);
          const consentIndex = seen.findIndex((row) => row.path === "/consent");
          const searchIndex = seen.findIndex((row) => row.path === "/search");
          assert(consentIndex >= 0 && searchIndex > consentIndex);
          const search = seen[searchIndex];
          assert.match(search.cookie, /consent=fixture/);
          assert.deepEqual(search.params.getAll("q"), ["fixture query"]);
          const audit = JSON.parse(search.params.get("audit"));
          assert.deepEqual(audit.consent, { trusted: true, inside: true });
          assert.equal(audit.ready, true);
          assert.deepEqual(
            audit.keys.map((row) => row.key),
            [..."fixture query", "Enter"],
          );
          assert(audit.keys.every((row) => row.trusted && row.focused));
          assert.deepEqual(
            audit.input.map((row) => row.data),
            [..."fixture query"],
          );
          assert(audit.input.every((row) => row.trusted));
          const typed = trace.pages.find((page) => page.phase === "step-3");
          assert.equal(typed.inputState.value, "fixture query");
          assert.equal(typed.inputState.activeTag, "TEXTAREA");
          assert.equal(typed.inputState.activeName, "q");
          const typing = trace.actions.find((action) => action.index === 3);
          assert.equal(typing.action, "type");
          assert(
            typing.finishedMs - typing.startedMs >= [..."fixture query"].length * inputCadenceMs,
          );
          const final = trace.pages.at(-1);
          assert.equal(new URL(final.url).pathname, "/search");
          assert.equal(final.status, 200);
          assert.equal(final.results, 1);
          assert.equal(final.readyState, "complete");
          assert.equal(
            trace.pages.some((page) => page.hydration?.budget_exceeded),
            false,
          );
          if (runtime === "native") {
            assert.equal(trace.readiness.complete, true);
            assert.equal(trace.readiness.selectorRequested, true);
            assert.equal(trace.navigationSource, "native-form");
            assert.equal(trace.formFallback, null);
          }
        }
      }
      for (const runtime of ["chrome", "native"]) {
        const start = requests.length;
        const configPath = join(directory, `missing-ready-${runtime}.json`);
        const output = join(directory, `missing-ready-${runtime}`);
        writeFileSync(
          configPath,
          JSON.stringify({
            url: origin + "/",
            chromePath: chrome,
            chromeIncognito: true,
            headless: true,
            interfaces: false,
            readySelector: "#never-created",
            readyTimeoutMs: 75,
            waitMs: 0,
            pageBudgetMs: 5000,
            steps: searchSteps("fixture query"),
          }),
        );
        await assert.rejects(
          run(
            t,
            runtime === "chrome" ? process.execPath : binary,
            runtime === "chrome"
              ? [join(here, "chrome-cdp-trace.mjs"), configPath, output]
              : [configPath, output],
          ),
          /^Error: 1:/,
        );
        const trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
        assert.equal(trace.captureComplete, false);
        assert.deepEqual(trace.actions, []);
        assert.equal(
          requests.slice(start).some((row) => row.path === "/search"),
          false,
        );
        if (runtime === "native") assert.equal(trace.readiness.complete, false);
      }
      complete = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      if (complete) rmSync(directory, { recursive: true, force: true });
      else console.error(`Action parity failure captures retained at ${directory}`);
    }
  },
);
