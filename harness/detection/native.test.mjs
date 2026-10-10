import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import {
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
const binary = resolve(root, "rust/target/debug/examples/detection_trace");
const run = (cmd, args) =>
  new Promise((done, fail) => {
    const child = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", (d) => {
      output += d;
    });
    child.stderr.on("data", (d) => {
      output += d;
    });
    const timer = setTimeout(() => child.kill("SIGKILL"), 45000);
    child.on("error", fail);
    child.on("close", (code) => {
      clearTimeout(timer);
      code === 0 ? done(output) : fail(new Error(`${code}: ${output}`));
    });
  });

test(
  "native trace follows a localhost JS retry and preserves payloads without exposing HttpOnly",
  { skip: !existsSync(binary) },
  async () => {
    const out = mkdtempSync(join(tmpdir(), "detection-native-test-"));
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
            '<form action="/search"><input type="hidden" name="ei" value="fixture"><textarea name="q"></textarea></form>',
          );
        } else if (url.pathname === "/search") {
          res.end(`<body><script>
          document.cookie='secret=guest; Path=/';
          navigator.sendBeacon('/collect',document.cookie);
          document.cookie='SG_SS=test; Path=/';
          location.replace('/results?sei=fixture');
        </script></body>`);
        } else if (url.pathname === "/results") {
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
          interfaceScript: join(root, "harness/detection/interface-trace.js"),
          steps: [{ focus: "textarea[name=q]" }, { type: "fixture query" }, { press: "Enter" }],
        }),
      );
      await run(frozen, [config, join(out, "capture")]);
      const trace = JSON.parse(readFileSync(join(out, "capture/trace.json")));
      assert.deepEqual(trace.errors, []);
      assert.equal(trace.pages.at(-1).results, 1);
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
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      rmSync(out, { recursive: true, force: true });
    }
  },
);
