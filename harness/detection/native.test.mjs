import { test } from "node:test";
import { superviseTestChild } from "./child-supervision.mjs";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import {
  appendFileSync,
  mkdirSync,
  existsSync,
  readFileSync,
  writeFileSync,
  mkdtempSync,
  rmSync,
  copyFileSync,
  chmodSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = resolve(
  process.env.TURBO_SURF_DETECTION_BINARY ||
    join(root, "rust/target/debug/examples/detection_trace"),
);
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

test(
  "native trace follows a localhost JS retry and preserves payloads without exposing HttpOnly",
  { skip: !existsSync(binary) },
  async (t) => {
    const out = mkdtempSync(join(tmpdir(), "detection-native-test-"));
    let complete = false;
    const seen = [];
    const server = createServer((req, res) => {
      const url = new URL(req.url, "http://localhost");
      let body = "";
      req.on("data", (d) => {
        body += d;
      });
      req.on("end", () => {
        seen.push({
          path: url.pathname,
          query: url.searchParams,
          cookie: req.headers.cookie,
          headers: req.headers,
          body,
        });
        res.setHeader("Content-Type", "text/html");
        if (url.pathname === "/") {
          res.setHeader("Accept-CH", "sec-ch-ua-arch, sec-ch-ua-bitness");
          res.setHeader("Set-Cookie", ["secret=server; HttpOnly; Path=/", "visible=yes; Path=/"]);
          res.end(
            '<form action="/search"><input type="hidden" name="ei" value="fixture"><input name="q"></form>',
          );
        } else if (url.pathname === "/search") {
          res.end(`<body><script>
          document.cookie='secret=guest; Path=/';
          navigator.sendBeacon('/collect',document.cookie);
          document.cookie='SG_SS=test; Path=/';
          location.replace('/results?sei=fixture');
        </script></body>`);
        } else if (url.pathname === "/results") {
          res.writeHead(302, { Location: "/final" });
          res.end();
        } else if (url.pathname === "/final") {
          res.end('<div id="rso"><a href="https://example.test/"><h3>Result</h3></a></div>');
        } else {
          res.end("<p>ok</p>");
        }
      });
    });
    try {
      await new Promise((done) => server.listen(0, "127.0.0.1", done));
      const frozen = join(out, "native");
      copyFileSync(binary, frozen);
      chmodSync(frozen, 0o700);
      const config = join(out, "config.json");
      writeFileSync(
        config,
        JSON.stringify({
          url: `http://127.0.0.1:${server.address().port}/`,
          interfaces: true,
          requireNativeFormNavigation: true,
          interfaceScript: join(root, "harness/detection/interface-trace.js"),
          steps: [{ focus: "input[name=q]" }, { type: "fixture query" }, { press: "Enter" }],
        }),
      );
      await run(t, frozen, [config, join(out, "capture")]);
      const trace = JSON.parse(readFileSync(join(out, "capture/trace.json")));
      assert.deepEqual(trace.errors, []);
      assert.equal(trace.navigationSource, "native-form");
      assert.equal(trace.formFallback, null);
      assert.equal(trace.pages.at(-1).results, 1);
      assert.equal(seen.find((r) => r.path === "/search").headers["sec-fetch-user"], "?1");
      for (const path of ["/results", "/final"]) {
        const headers = seen.find((r) => r.path === path).headers;
        assert.equal(headers["sec-fetch-user"], undefined);
        assert.equal(headers["sec-fetch-site"], "same-origin");
        assert.equal(headers["sec-fetch-mode"], "navigate");
        assert.equal(headers["sec-fetch-dest"], "document");
      }
      assert.equal(seen.find((r) => r.path === "/search").query.get("ei"), "fixture");
      assert.equal(seen.find((r) => r.path === "/search").query.get("q"), "fixture query");
      const collect = seen.find((r) => r.path === "/collect");
      assert.equal(collect.body, "visible=yes");
      assert.equal(collect.headers["sec-fetch-dest"], "empty");
      assert.equal(collect.headers["sec-fetch-mode"], "no-cors");
      assert.equal(collect.headers["sec-ch-ua-arch"], '"arm"');
      assert.equal(collect.headers["sec-ch-ua-bitness"], '"64"');
      assert.equal(collect.headers["sec-fetch-user"], undefined);
      assert.equal(collect.headers["upgrade-insecure-requests"], undefined);
      assert.match(seen.find((r) => r.path === "/results").cookie, /secret=server/);
      const rows = readFileSync(join(out, "capture/network.jsonl"), "utf8")
        .trim()
        .split("\n")
        .map(JSON.parse);
      assert(rows.some((r) => r.event === "request" && r.body === "visible=yes"));
      assert(rows.some((r) => r.event === "body" && r.body.includes("<h3>Result</h3>")));
      assert(
        trace.pages.some((p) => p.interfaces?.accesses.some((a) => a.name.endsWith(".cookie"))),
      );
      complete = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      if (complete) rmSync(out, { recursive: true, force: true });
      else t.diagnostic(`Native failure evidence retained at ${out}`);
    }
  },
);

test(
  "native trace retains a same-URL POST response without a fallback refetch",
  { skip: !existsSync(binary) },
  async (t) => {
    const out = mkdtempSync(join(tmpdir(), "detection-native-post-"));
    let complete = false;
    const seen = [];
    const server = createServer((req, res) => {
      let body = "";
      req.on("data", (chunk) => {
        body += chunk;
      });
      req.on("end", () => {
        seen.push([req.method, req.url, body]);
        res.setHeader("Content-Type", "text/html; charset=utf-8");
        if (req.method === "POST") {
          res.writeHead(201, { "Set-Cookie": "consent=native; HttpOnly; Path=/" });
          res.end('<div id="rso"><a><h3>Actual POST response</h3></a></div>');
        } else {
          res.end(
            '<form method="post"><input name="token" value="real"></form><script>document.querySelector("form").submit()</script>',
          );
        }
      });
    });
    try {
      await new Promise((done) => server.listen(0, "127.0.0.1", done));
      const url = `http://127.0.0.1:${server.address().port}/same`;
      const config = join(out, "config.json");
      writeFileSync(config, JSON.stringify({ url, steps: [], requireNativeFormNavigation: true }));
      await run(t, binary, [config, join(out, "capture")]);
      const trace = JSON.parse(readFileSync(join(out, "capture/trace.json")));
      assert.deepEqual(trace.errors, []);
      assert.equal(trace.navigationSource, "native-form");
      assert.equal(trace.formFallback, null);
      const final = trace.pages.at(-1);
      assert.equal(final.url, url);
      assert.equal(final.status, 201);
      assert.equal(final.results, 1);
      assert.equal(final.nativeNavigationCount, 1);
      assert.deepEqual(seen, [
        ["GET", "/same", ""],
        ["POST", "/same", "token=real"],
      ]);
      assert.match(
        readFileSync(join(out, "capture", `${final.phase}.html`), "utf8"),
        /Actual POST response/,
      );
      complete = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      if (complete) rmSync(out, { recursive: true, force: true });
      else t.diagnostic(`Native failure evidence retained at ${out}`);
    }
  },
);

test(
  "native trace preserves trusted handler location activation before automatic retries",
  { skip: !existsSync(binary) },
  async (t) => {
    const out = mkdtempSync(join(tmpdir(), "detection-handler-navigation-"));
    let complete = false;
    const seen = [];
    const server = createServer((req, res) => {
      const parsed = new URL(req.url, "http://localhost");
      const path = parsed.pathname;
      seen.push({
        path,
        headers: req.headers,
        active: parsed.searchParams.get("active"),
        sticky: parsed.searchParams.get("sticky"),
      });
      res.setHeader("Content-Type", "text/html");
      if (path === "/") {
        res.end(`<button id=go>go</button><input id=q><script>
          document.querySelector('#go').addEventListener('pointerdown',()=>location.assign('/next'));
          document.querySelector('#q').addEventListener('keydown',()=>location.href='/next');
        </script>`);
      } else if (path === "/next" || path === "/retry") {
        res.writeHead(302, { Location: path === "/next" ? "/destination" : "/final" });
        res.end();
      } else if (path === "/destination") {
        res.end(
          "<script>location.replace('/retry?active='+navigator.userActivation.isActive+'&sticky='+navigator.userActivation.hasBeenActive)</script>",
        );
      } else {
        res.end('<div id="rso"><a href="https://example.test/"><h3>Result</h3></a></div>');
      }
    });
    try {
      await new Promise((done) => server.listen(0, "127.0.0.1", done));
      const frozen = join(out, "native");
      copyFileSync(binary, frozen);
      chmodSync(frozen, 0o700);
      for (const [name, steps] of [
        ["click", [{ click: "#go" }]],
        ["keydown", [{ focus: "#q" }, { press: "x" }]],
      ]) {
        seen.length = 0;
        const config = join(out, `${name}.json`);
        writeFileSync(
          config,
          JSON.stringify({ url: `http://127.0.0.1:${server.address().port}/`, steps }),
        );
        await run(t, frozen, [config, join(out, name)]);
        const trace = JSON.parse(readFileSync(join(out, name, "trace.json")));
        assert.deepEqual(trace.errors, []);
        assert.equal(trace.navigationSource, "script");
        assert.equal(trace.pages.at(-1).results, 1);
        const retry = seen.find((row) => row.path === "/retry");
        assert.equal(retry.active, "false");
        assert.equal(retry.sticky, "true");
        for (const path of ["/next", "/destination", "/retry", "/final"]) {
          const request = seen.find((row) => row.path === path);
          assert(request, `${name}: missing ${path}`);
          assert.equal(
            request.headers["sec-fetch-user"],
            ["/next", "/destination"].includes(path) ? "?1" : undefined,
            `${name}: ${path}`,
          );
        }
      }
      complete = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      if (complete) rmSync(out, { recursive: true, force: true });
      else t.diagnostic(`Native failure evidence retained at ${out}`);
    }
  },
);

test(
  "consent dismissal waits real elapsed time and installs delayed response cookies before search",
  { skip: !existsSync(binary) },
  async (t) => {
    const out = mkdtempSync(join(tmpdir(), "detection-consent-wait-"));
    let complete = false;
    const seen = [];
    const server = createServer((req, res) => {
      const path = new URL(req.url, "http://localhost").pathname;
      seen.push({ path, time: Date.now(), cookie: req.headers.cookie || "" });
      res.setHeader("Content-Type", "text/html");
      if (path === "/") {
        res.end(`<button id="consent" type="button">save</button>
          <form action="/search"><input name="q"></form><script>
          document.querySelector('#consent').onclick=()=>{
            const xhr=new XMLHttpRequest();xhr.open('POST','/save');xhr.send('consent');
          };</script>`);
      } else if (path === "/save") {
        setTimeout(() => {
          res.setHeader("Set-Cookie", "saved=real-response; HttpOnly; Path=/");
          res.writeHead(204);
          res.end();
        }, 800);
      } else {
        res.end("<main>done</main>");
      }
    });
    try {
      await new Promise((done) => server.listen(0, "127.0.0.1", done));
      const config = join(out, "config.json");
      writeFileSync(
        config,
        JSON.stringify({
          url: `http://127.0.0.1:${server.address().port}/`,
          dismissSelector: "#consent",
          waitMs: 1500,
          requireNativeFormNavigation: true,
          steps: [
            { focus: "input[name=q]" },
            { type: "fixture" },
            { wait: 350 },
            { press: "Enter" },
          ],
        }),
      );
      await run(t, binary, [config, join(out, "capture")]);
      const home = seen.find((request) => request.path === "/");
      const saved = seen.find((request) => request.path === "/save");
      const search = seen.find((request) => request.path === "/search");
      assert.ok(saved, "consent handler issued genuine XHR");
      assert.ok(search, "native form navigation reached search");
      assert.ok(search.time - home.time >= 1850, "configured wait is real elapsed time");
      assert.match(search.cookie, /(?:^|; )saved=real-response(?:;|$)/);
      const trace = JSON.parse(readFileSync(join(out, "capture/trace.json")));
      assert.deepEqual(trace.errors, []);
      assert.equal(trace.dismissal.clicked, true);
      complete = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      if (complete) rmSync(out, { recursive: true, force: true });
      else t.diagnostic(`Native failure evidence retained at ${out}`);
    }
  },
);
