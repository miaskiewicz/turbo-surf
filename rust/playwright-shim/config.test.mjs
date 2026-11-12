// Offline unit tests for the Playwright-config interpreter and the
// `turbo-surf-test` CLI (`bin/turbo-surf-test.mjs`). Uses only the shim's own
// pieces + a localhost `node:http` server for the webServer path.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { cpus, tmpdir } from "node:os";
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
  orderProjects,
  startWebServers,
  stopWebServers,
  testRunnerFlags,
} from "./config.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CLI = resolve(HERE, "bin/turbo-surf-test.mjs");

// FIXME(turbo-surf#305): on GitHub Actions this file hangs after all tests
// pass (open handles from spawned subprocesses whose stdio pipes survive
// SIGKILL on the runner). The 300s file-level timeout fires, marking the file
// as cancelled and failing the run. Skip on CI; local coverage retained.
const CI = !!process.env.CI;

function tmp() {
  return mkdtempSync(join(tmpdir(), "surf-cli-"));
}

test("normalizeConfig: defaults for an empty config", { skip: CI }, () => {
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

test("normalizeConfig: projects inherit top-level use + are overridable", { skip: CI }, () => {
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

test("globToRegex: *, **, ?, @(a|b), {a,b}", { skip: CI }, () => {
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

test("discoverTests: walks testDir, honors match/ignore", { skip: CI }, () => {
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

test("loadConfig: imports an .mjs and normalizes it", { skip: CI }, async () => {
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

test("webServer: spawn, wait until ready, teardown", { skip: CI, timeout: 30_000 }, async () => {
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

test(
  "CLI end-to-end: loads config, runs test files per project, reports exit",
  { skip: CI, timeout: 30_000 },
  async () => {
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
  },
);

test(
  "CLI: --list-only prints the discovered files and exits 0",
  { skip: CI, timeout: 30_000 },
  async () => {
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
  },
);

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

function runCli(args, cwd, timeoutMs = 30_000) {
  return new Promise((res) => {
    const child = spawn(process.execPath, [CLI, ...args], {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let done = false;
    const finish = (code, extra) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      child.stdout.destroy();
      child.stderr.destroy();
      res({ code, stdout, stderr: stderr + (extra || "") });
    };
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      finish(null, "\n[runCli killed after " + timeoutMs + "ms]");
    }, timeoutMs);
    child.stdout.on("data", (b) => (stdout += b.toString("utf8")));
    child.stderr.on("data", (b) => (stderr += b.toString("utf8")));
    child.on("exit", (code) => finish(code));
  });
}

test("webServer readiness accepts 200..403 and root index fallback only", async () => {
  let status = 200;
  let indexStatus = 404;
  const server = createServer((req, res) => {
    res.writeHead(req.url === "/index.html" ? indexStatus : status);
    res.end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    for (status of [200, 403, 404, 500]) {
      const options = {
        command: `"${process.execPath}" -e "process.exit(1)"`,
        url: `${origin}/health`,
        reuseExistingServer: true,
        timeout: 1000,
      };
      if (status <= 403) {
        const handles = await startWebServers([options]);
        assert.equal(handles[0].child, null);
      } else {
        await assert.rejects(startWebServers([options]), /exited early|timed out/);
      }
    }
    status = 404;
    indexStatus = 200;
    const handles = await startWebServers([
      {
        url: origin,
        reuseExistingServer: true,
        timeout: 1000,
        command: `"${process.execPath}" -e "process.exit(1)"`,
      },
    ]);
    assert.equal(handles[0].child, null);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test(
  "webServer shutdown terminates shell descendants",
  { skip: process.platform === "win32", timeout: 10000 },
  async () => {
    const port = await freePort();
    const root = tmp();
    const script = join(root, "descendant.mjs");
    const pidFile = join(root, "pid");
    writeFileSync(
      script,
      `import {createServer} from 'node:http';
    import {writeFileSync} from 'node:fs';
    writeFileSync(${JSON.stringify(pidFile)}, String(process.pid));
    createServer((req,res)=>res.end('ok')).listen(${port}, '127.0.0.1');`,
    );
    const handles = await startWebServers([
      {
        command: `"${process.execPath}" "${script}" & wait`,
        port,
        timeout: 3000,
        gracefulShutdown: { signal: "SIGTERM", timeout: 200 },
      },
    ]);
    const pid = Number(readFileSync(pidFile, "utf8"));
    try {
      await stopWebServers(handles);
      await assert.rejects(fetch(`http://127.0.0.1:${port}`));
    } finally {
      try {
        process.kill(pid, "SIGKILL");
      } catch {}
      await stopWebServers(handles);
    }
  },
);

test("webServer cwd resolves against the configuration directory", async () => {
  const root = tmp();
  const port = await freePort();
  writeFileSync(
    join(root, "server.mjs"),
    `import {createServer} from 'node:http';
    createServer((req,res)=>res.end('ok')).listen(${port}, '127.0.0.1');`,
  );
  const config = normalizeConfig(
    {
      webServer: {
        command: `"${process.execPath}" server.mjs`,
        port,
        timeout: 2000,
        gracefulShutdown: { signal: "SIGTERM", timeout: 200 },
      },
    },
    root,
  );
  assert.equal(config.webServer[0].cwd, root);
  const relative = normalizeConfig({ webServer: { command: "unused", cwd: "nested" } }, root);
  assert.equal(relative.webServer[0].cwd, join(root, "nested"));
  const absolute = normalizeConfig({ webServer: { command: "unused", cwd: root } }, "/elsewhere");
  assert.equal(absolute.webServer[0].cwd, root);
  const handles = await startWebServers(config.webServer);
  await stopWebServers(handles);
});

test("discoverTests honors RegExp selectors against absolute paths without state leakage", () => {
  const root = tmp();
  writeFileSync(join(root, "one.spec.mjs"), "// fixture");
  writeFileSync(join(root, "two.spec.mjs"), "// fixture");
  const project = {
    testDir: root,
    testMatch: [new RegExp(`${root.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/.*\\.spec\\.mjs$`, "g")],
    testIgnore: [/two\.spec\.mjs$/g],
  };
  assert.deepEqual(discoverTests(project), [join(root, "one.spec.mjs")]);
  assert.deepEqual(discoverTests(project), [join(root, "one.spec.mjs")]);
});

test(
  "CLI runs selected project dependencies first and blocks dependents on failure",
  { timeout: 20000 },
  async () => {
    const root = tmp();
    const marker = join(root, "setup.marker");
    const appMarker = join(root, "app.marker");
    writeFileSync(
      join(root, "setup.spec.mjs"),
      `import test from 'node:test';import {writeFileSync} from 'node:fs';
    test('setup dependency',()=>writeFileSync(${JSON.stringify(marker)},'ready'));`,
    );
    writeFileSync(
      join(root, "app.spec.mjs"),
      `import test from 'node:test';import assert from 'node:assert/strict';import {existsSync,writeFileSync} from 'node:fs';
    test('dependent app',()=>{assert.ok(existsSync(${JSON.stringify(marker)}));writeFileSync(${JSON.stringify(appMarker)},'ran');});`,
    );
    const config = join(root, "projects.config.mjs");
    writeFileSync(
      config,
      `export default {projects:[
    {name:'app',testMatch:'app.spec.mjs',dependencies:['setup']},
    {name:'setup',testMatch:'setup.spec.mjs'}]};`,
    );
    const result = await runCli(["--config", config, "--project", "app"], root);
    assert.equal(result.code, 0, result.stdout + result.stderr);
    assert.equal(readFileSync(appMarker, "utf8"), "ran");
    writeFileSync(
      join(root, "setup.spec.mjs"),
      `import test from 'node:test';test('failed setup',()=>{throw Error('setup failed');});`,
    );
    writeFileSync(appMarker, "not run");
    const failed = await runCli(["--config", config, "--project", "app"], root);
    assert.equal(failed.code, 1);
    assert.equal(readFileSync(appMarker, "utf8"), "not run");
  },
);

test("project dependencies reject missing projects and cycles", () => {
  assert.throws(
    () => orderProjects([{ name: "app", dependencies: ["missing"] }]),
    /Unknown dependency/,
  );
  assert.throws(
    () =>
      orderProjects([
        { name: "app", dependencies: ["setup"] },
        { name: "setup", dependencies: ["app"] },
      ]),
    /dependency cycle/,
  );
  const setup = { name: "setup" };
  const app = { name: "app", dependencies: ["setup"] };
  const other = { name: "other", dependencies: ["setup"] };
  assert.deepEqual(orderProjects([app, other, setup], ["app", "other"]), [setup, app, other]);
});

test("CLI rejects no matching tests unless pass-with-no-tests is explicit", async () => {
  const root = tmp();
  const config = join(root, "empty.config.mjs");
  writeFileSync(config, "export default {testMatch:'missing.spec.mjs'};");
  const empty = await runCli(["--config", config], root);
  assert.equal(empty.code, 1, empty.stdout + empty.stderr);
  assert.match(empty.stdout + empty.stderr, /No tests found/);
  const allowed = await runCli(["--config", config, "--pass-with-no-tests"], root);
  assert.equal(allowed.code, 0, allowed.stdout + allowed.stderr);
});

test("CLI explicitly rejects unsupported reporters including secondary entries", async () => {
  const root = tmp();
  writeFileSync(
    join(root, "success.spec.mjs"),
    "import test from 'node:test';test('body must not run',()=>console.log('BODY_RAN'));",
  );
  const config = join(root, "json.config.mjs");
  for (const reporter of [
    "[['json',{outputFile:'result.json'}]]",
    "[['list'],['json',{outputFile:'result.json'}]]",
  ]) {
    writeFileSync(config, `export default {reporter:${reporter}};`);
    const result = await runCli(["--config", config], root);
    assert.equal(result.code, 1, result.stdout + result.stderr);
    assert.match(result.stderr, /unsupported reporter.*json/i);
    assert.doesNotMatch(result.stdout, /BODY_RAN/);
  }
});

test("CLI honors inherited and project grep/grepInvert across suite titles", async () => {
  const root = tmp();
  writeFileSync(
    join(root, "grep.spec.mjs"),
    `import {test,describe} from 'node:test';
    describe('Selected suite',()=>{
      test('desired',()=>console.log('SELECTED'));
      test('excluded',()=>{throw Error('excluded ran');});
    });
    test('unwanted',()=>{throw Error('unwanted ran');});`,
  );
  const config = join(root, "grep.config.mjs");
  writeFileSync(
    config,
    `export default {grep:/selected/i,grepInvert:/excluded/,projects:[{name:'inherited'}]};`,
  );
  const inherited = await runCli(["--config", config], root);
  assert.equal(inherited.code, 0, inherited.stdout + inherited.stderr);
  assert.match(inherited.stdout, /SELECTED/);
  writeFileSync(
    config,
    `export default {grep:/nothing/,projects:[{name:'override',grep:[/selected/i,/else/],grepInvert:/excluded/}]};`,
  );
  const override = await runCli(["--config", config], root);
  assert.equal(override.code, 0, override.stdout + override.stderr);
  assert.match(override.stdout, /SELECTED/);
});

test(
  "CLI HTML report completes when a finished test leaves a timer open",
  { timeout: 10000 },
  async () => {
    const root = tmp();
    writeFileSync(
      join(root, "timer.spec.mjs"),
      `import test from 'node:test';
    test('completed test',()=>{});setInterval(()=>{},1000);`,
    );
    const config = join(root, "html.config.mjs");
    writeFileSync(config, "export default {reporter:'html'};");
    const result = await runCli(["--config", config], root, 1500);
    assert.equal(result.code, 0, result.stdout + result.stderr);
    assert.match(readFileSync(join(root, "playwright-report.html"), "utf8"), /completed test/);
  },
);

test("test runner capabilities report explicit supported Node versions", () => {
  assert.throws(() => testRunnerFlags({}, "20.13.0"), /requires Node.js 20.14/);
  assert.throws(() => testRunnerFlags({}, "21.7.0"), /requires Node.js 20.14/);
  assert.deepEqual(testRunnerFlags({}, "20.14.0"), ["--test-force-exit"]);
  assert.throws(
    () => testRunnerFlags({ grepInvert: [/excluded/] }, "20.19.0"),
    /grepInvert requires Node.js 22.1/,
  );
  assert.throws(
    () => testRunnerFlags({ grepInvert: [/excluded/] }, "22.0.0"),
    /grepInvert requires Node.js 22.1/,
  );
  assert.deepEqual(testRunnerFlags({ grep: [/selected/i], grepInvert: [/excluded/] }, "22.1.0"), [
    "--test-force-exit",
    "--test-name-pattern=/selected/i",
    "--test-skip-pattern=/excluded/",
  ]);
});

test("artifact configuration rejects enabled trace and video before running tests (#465)", () => {
  for (const use of [
    { trace: "on-first-retry" },
    { trace: { mode: "retain-on-failure" } },
    { video: "retain-on-failure" },
    { recordVideo: { dir: "/tmp/surf-unused-video" } },
  ])
    assert.throws(() => testRunnerFlags({ use }), /trace|video|recordVideo/);
  assert.doesNotThrow(() => testRunnerFlags({ use: { trace: { mode: "off" }, video: "off" } }));
});

test("CLI rejects requested recording before executing the test body (#465)", async () => {
  const root = tmp();
  writeFileSync(
    join(root, "artifact.spec.mjs"),
    "import test from 'node:test'; test('body',()=>console.log('ARTIFACT_BODY_RAN'));\n",
  );
  writeFileSync(
    join(root, "playwright.config.mjs"),
    "export default { testDir:'.',testMatch:'artifact.spec.mjs',use:{video:'retain-on-failure'} };\n",
  );
  const result = await runCli(["--config", join(root, "playwright.config.mjs")], root);
  assert.notEqual(result.code, 0);
  assert.match(result.stderr, /video unavailable/);
  assert.doesNotMatch(result.stdout, /ARTIFACT_BODY_RAN/);
});

test("CLI honors configured workers:1 across files sharing an exclusive resource", async () => {
  const root = tmp();
  const lock = join(root, "suite.lock");
  for (const name of ["one", "two"]) {
    writeFileSync(
      join(root, `${name}.spec.mjs`),
      `
      import test from 'node:test';
      import {openSync, closeSync, unlinkSync} from 'node:fs';
      test('exclusive ${name}', async () => {
        const fd = openSync(${JSON.stringify(lock)}, 'wx');
        try { await new Promise(resolve => setTimeout(resolve, 250)); }
        finally { closeSync(fd); unlinkSync(${JSON.stringify(lock)}); }
      });`,
    );
  }
  const config = join(root, "playwright.config.mjs");
  writeFileSync(config, "export default {workers:1};");
  const result = await runCli(["--config", config], root);
  assert.equal(result.code, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /exclusive one/);
  assert.match(result.stdout, /exclusive two/);
});

test("workers configuration supports integers, percentages and project caps", () => {
  const config = normalizeConfig(
    {
      workers: 2,
      projects: [
        { name: "limited", workers: 1 },
        { name: "capped", workers: 4 },
        { name: "inherited" },
      ],
    },
    "/root",
  );
  assert.deepEqual(
    config.projects.map((project) => project.workers),
    [1, 2, 2],
  );
  assert.equal(
    normalizeConfig({ workers: "50%" }, "/root").workers,
    Math.max(1, Math.floor(cpus().length / 2)),
  );
  assert.equal(
    normalizeConfig({ workers: "1%" }, "/root").workers,
    Math.max(1, Math.floor(cpus().length / 100)),
  );
  assert.ok(testRunnerFlags(config.projects[0]).includes("--test-concurrency=1"));
  for (const workers of [0, -1, 1.5, NaN, Infinity, "0%", "bad%", "1", true]) {
    assert.throws(
      () => normalizeConfig({ workers, projects: [{ name: "project", workers }] }, "/root"),
      /workers must be/,
    );
  }
});
