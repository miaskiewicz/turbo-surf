// Offline unit tests for the Playwright-config interpreter and the
// `turbo-surf-test` CLI (`bin/turbo-surf-test.mjs`). Uses only the shim's own
// pieces + a localhost `node:http` server for the webServer path.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  discoverTests,
  findConfig,
  globToRegex,
  loadConfig,
  matchAny,
  normalizeConfig,
  startWebServers,
  stopWebServers,
} from "./config.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CLI = resolve(HERE, "bin/turbo-surf-test.mjs");

function tmp() {
  return mkdtempSync(join(tmpdir(), "surf-cli-"));
}

test("normalizeConfig: defaults for an empty config", () => {
  const c = normalizeConfig({}, "/root");
  assert.equal(c.rootDir, "/root");
  assert.equal(c.testDir, "/root");
  assert.deepEqual(c.testMatch, ["**/*.@(spec|test).@(ts|mts|js|mjs|cjs)"]);
  assert.deepEqual(c.testIgnore, ["**/node_modules/**"]);
  assert.deepEqual(c.reporter, [["list"]]);
  assert.deepEqual(c.webServer, []);
  assert.equal(c.projects.length, 1);
  assert.equal(c.projects[0].name, "default");
});

test("normalizeConfig: projects inherit top-level use + are overridable", () => {
  const c = normalizeConfig(
    {
      testDir: "./e2e",
      use: { baseURL: "http://top", locale: "en" },
      projects: [
        { name: "a", use: { baseURL: "http://a" } },
        { name: "b", testDir: "./other", testMatch: "**/*.foo.mjs" },
      ],
      reporter: [["html", { outputFile: "r.html" }], "list"],
    },
    "/root",
  );
  assert.equal(c.projects.length, 2);
  assert.equal(c.projects[0].use.baseURL, "http://a");
  assert.equal(c.projects[0].use.locale, "en");
  assert.equal(c.projects[1].use.baseURL, "http://top");
  assert.equal(c.projects[1].testDir, "/root/other");
  assert.deepEqual(c.projects[1].testMatch, ["**/*.foo.mjs"]);
  assert.deepEqual(c.reporter, [["html", { outputFile: "r.html" }], ["list"]]);
});

test("globToRegex: *, **, ?, @(a|b), {a,b}", () => {
  assert.ok(globToRegex("*.mjs").test("a.mjs"));
  assert.ok(!globToRegex("*.mjs").test("a/b.mjs"));
  assert.ok(globToRegex("**/*.mjs").test("a/b/c.mjs"));
  assert.ok(globToRegex("**/*.mjs").test("a.mjs"));
  assert.ok(globToRegex("**/*.@(spec|test).mjs").test("e2e/x.spec.mjs"));
  assert.ok(globToRegex("**/*.@(spec|test).mjs").test("e2e/x.test.mjs"));
  assert.ok(!globToRegex("**/*.@(spec|test).mjs").test("e2e/x.other.mjs"));
  assert.ok(globToRegex("file.{a,b}").test("file.a"));
  assert.ok(globToRegex("file.?").test("file.x"));
  assert.ok(!globToRegex("file.?").test("file.xx"));
  // The ignore default must swallow nested node_modules.
  assert.ok(matchAny("foo/node_modules/x.mjs", ["**/node_modules/**"]));
});

test("discoverTests: walks testDir, honors match/ignore", () => {
  const root = tmp();
  for (const rel of [
    "a.spec.mjs",
    "nested/b.test.mjs",
    "nested/c.other.mjs",
    "node_modules/d.spec.mjs",
  ]) {
    const p = join(root, rel);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, "// fixture");
  }
  const files = discoverTests({
    testDir: root,
    testMatch: ["**/*.@(spec|test).mjs"],
    testIgnore: ["**/node_modules/**"],
  });
  const names = files.map((f) => f.slice(root.length + 1));
  assert.deepEqual(names.toSorted(), ["a.spec.mjs", "nested/b.test.mjs"]);
});

test("loadConfig: imports an .mjs and normalizes it", async () => {
  const root = tmp();
  writeFileSync(
    join(root, "playwright.config.mjs"),
    `export default {
      testDir: './t',
      use: { baseURL: 'http://x' },
      projects: [{ name: 'chrome', use: { locale: 'en' } }],
    };`,
  );
  const cfg = await loadConfig(join(root, "playwright.config.mjs"));
  assert.equal(cfg.testDir, join(root, "t"));
  assert.equal(cfg.projects[0].use.baseURL, "http://x");
  assert.equal(cfg.projects[0].use.locale, "en");
  assert.equal(findConfig(root), join(root, "playwright.config.mjs"));
});

test("webServer: spawn, wait until ready, teardown", async () => {
  const port = await freePort();
  const root = tmp();
  const script = join(root, "server.mjs");
  writeFileSync(
    script,
    `import { createServer } from 'node:http';
     setTimeout(() => {
       createServer((_q, r) => r.end('ok')).listen(${port});
     }, 150);
     process.on('SIGTERM', () => process.exit(0));`,
  );
  const handles = await startWebServers(
    [{ command: `"${process.execPath}" "${script}"`, port, timeout: 10_000 }],
    () => {},
  );
  const res = await fetch(`http://127.0.0.1:${port}`);
  assert.equal(res.status, 200);
  await stopWebServers(handles);
});

test("CLI end-to-end: loads config, runs test files per project, reports exit", async () => {
  const root = tmp();
  await mkdir(join(root, "t"), { recursive: true });
  writeFileSync(
    join(root, "t/sample.spec.mjs"),
    `
    import test from 'node:test';
    import assert from 'node:assert/strict';
    test('runs in the configured project', () => {
      assert.equal(process.env.TURBO_PW_PROJECT_NAME, 'alpha');
      const use = JSON.parse(process.env.TURBO_PW_USE || '{}');
      assert.equal(use.baseURL, 'http://example.test');
    });
    `,
  );
  writeFileSync(
    join(root, "playwright.config.mjs"),
    `export default {
      testDir: './t',
      projects: [{ name: 'alpha', use: { baseURL: 'http://example.test' } }],
    };`,
  );
  const { code, stdout, stderr } = await runCli(
    ["--config", join(root, "playwright.config.mjs")],
    root,
  );
  assert.equal(code, 0, `cli failed: ${stdout}\n---stderr---\n${stderr}`);
  assert.match(stdout, /runs in the configured project/, `stderr was: ${stderr}`);
});

test("CLI: --list-only prints the discovered files and exits 0", async () => {
  const root = tmp();
  await mkdir(join(root, "t"), { recursive: true });
  writeFileSync(join(root, "t/a.spec.mjs"), "import 'node:test';");
  writeFileSync(join(root, "playwright.config.mjs"), `export default { testDir: './t' };`);
  const { code, stdout } = await runCli(
    ["--config", join(root, "playwright.config.mjs"), "--list-only"],
    root,
  );
  assert.equal(code, 0);
  assert.match(stdout, /a\.spec\.mjs/);
});

test("CLI: html reporter writes a playwright-report.html", async () => {
  const root = tmp();
  await mkdir(join(root, "t"), { recursive: true });
  writeFileSync(join(root, "t/a.spec.mjs"), `import test from 'node:test'; test('one', () => {});`);
  writeFileSync(
    join(root, "playwright.config.mjs"),
    `export default { testDir: './t', reporter: 'html' };`,
  );
  const { code } = await runCli(["--config", join(root, "playwright.config.mjs")], root);
  assert.equal(code, 0);
  const html = readFileSync(join(root, "playwright-report.html"), "utf8");
  assert.match(html, /turbo-surf test report/);
  assert.match(html, />one</);
});

async function freePort() {
  return await new Promise((res) => {
    const s = createNetServer();
    s.listen(0, () => {
      const p = s.address().port;
      s.close(() => res(p));
    });
  });
}

function runCli(args, cwd) {
  return new Promise((res) => {
    const child = spawn(process.execPath, [CLI, ...args], { cwd });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (b) => (stdout += b.toString("utf8")));
    child.stderr.on("data", (b) => (stderr += b.toString("utf8")));
    child.on("exit", (code) => res({ code, stdout, stderr }));
  });
}
