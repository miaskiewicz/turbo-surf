import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const cli = join(root, "cli.js");
const entry = join(root, "index.js");
const loader = join(root, "rust/crates/turbo-surf-napi/index.js");
const platformPackage = "@miaskiewicz/turbo-surf-darwin-arm64";

// Keep the production filenames intact for V8 coverage. The subprocess sees a
// private installed-package/filesystem boundary, never the developer's binaries.
const preload = String.raw`
const fs = require('node:fs');
const cp = require('node:child_process');
const Module = require('node:module');
const cfg = JSON.parse(process.env.SURF_LAUNCHER_CONTRACT);
const paths = cfg.paths || {};
const map = p => paths[p] || p;
const originalExists = fs.existsSync;
const originalStat = fs.statSync;
const originalCopy = fs.copyFileSync;
const originalResolve = Module._resolveFilename;
Object.defineProperty(process, 'platform', {value: cfg.platform || 'darwin'});
Object.defineProperty(process, 'arch', {value: cfg.arch || 'arm64'});
if (cfg.report === 'throw') process.report.getReport = () => {throw Error('report unavailable')};
else process.report.getReport = () => ({header: cfg.report === 'glibc' ? {glibcVersionRuntime:'2.39'} : {}});
fs.existsSync = p => Object.hasOwn(paths,p) ? originalExists(map(p)) : cfg.hidden.some(prefix => String(p).startsWith(prefix)) ? false : originalExists(p);
fs.statSync = p => originalStat(map(p));
fs.copyFileSync = (a,b) => originalCopy(map(a),map(b));
Module._resolveFilename = function(request,...args) {
  if (request.startsWith('@miaskiewicz/turbo-surf-') && request.endsWith('/package.json')) {
    if (!cfg.packagePath) throw Object.assign(Error('fixture package not installed'),{code:'MODULE_NOT_FOUND'});
    return cfg.packagePath;
  }
  if (Object.hasOwn(paths,request)) return map(request);
  return originalResolve.call(this,request,...args);
};
Module._extensions['.node'] = (module, filename) => {
  module.exports = JSON.parse(fs.readFileSync(filename,'utf8'));
};
const originalSpawn = cp.spawn;
const originalSpawnSync = cp.spawnSync;
cp.spawn = (file,...args) => originalSpawn(map(file),...args);
cp.spawnSync = (file,...args) => originalSpawnSync(map(file),...args);
cp.execFileSync = (file,args) => {
  if (file !== 'codesign') throw Error('unexpected external command '+file);
  fs.appendFileSync(cfg.codesignLog,JSON.stringify(args)+'\n');
  if (cfg.codesignFails) throw Error('codesign unavailable');
};
`;

function scenario(callback) {
  const directory = mkdtempSync(join(tmpdir(), "surf-launcher-contract-"));
  const cfg = {
    paths: {},
    hidden: [join(root, "bin"), join(root, "rust/target"), dirname(loader)],
    codesignLog: join(directory, "codesign.jsonl"),
  };
  const preloadPath = join(directory, "preload.cjs");
  writeFileSync(preloadPath, preload);
  const file = (name, contents, mode) => {
    const path = join(directory, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, contents, mode ? { mode } : undefined);
    return path;
  };
  const binary = (name = "native") =>
    file(
      name,
      `#!${process.execPath}\nprocess.stdin.setEncoding('utf8');let input='';process.stdin.on('data',s=>input+=s);process.stdin.on('end',()=>{console.log(JSON.stringify({argv:process.argv.slice(2),input,cwd:process.cwd(),marker:process.env.SURF_MARKER,selected:${JSON.stringify(name)}}));console.error('native-stderr');if(process.env.SURF_SIGNAL)process.kill(process.pid,'SIGTERM');else process.exit(Number(process.env.SURF_EXIT||0));});\n`,
      0o755,
    );
  const run = (args, options = {}) =>
    spawnSync(process.execPath, ["--require", preloadPath, ...args], {
      encoding: "utf8",
      timeout: 10000,
      input: "request\n",
      ...options,
      env: {
        ...process.env,
        TURBO_SURF_NAPI_PATH: "",
        NODE_OPTIONS: "",
        SURF_EXIT: "0",
        SURF_SIGNAL: "",
        SURF_LAUNCHER_CONTRACT: JSON.stringify(cfg),
        ...options.env,
      },
    });
  try {
    callback({ cfg, file, binary, run, directory });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function installPackage(s, addon = false) {
  s.cfg.packagePath = s.file("package/package.json", JSON.stringify({ name: platformPackage }));
  const name = addon ? "turbo-surf.darwin-arm64.node" : "turbo-surf-mcp";
  const path = addon
    ? s.file(`package/${name}`, '{"selected":"package"}')
    : s.binary(`package/${name}`);
  return path;
}

function readJson(result) {
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout.trim());
}

test("native CLI prefers the installed platform package and forwards stdio, argv and exit status", () => {
  scenario((s) => {
    installPackage(s);
    s.cfg.paths[join(root, "bin/turbo-surf-mcp-darwin-arm64")] = s.binary("legacy");
    const result = s.run([cli, "--flag", "space and 😀"], {
      env: { SURF_MARKER: "inherited", SURF_EXIT: "7" },
    });
    assert.equal(result.status, 7);
    assert.equal(result.stderr, "native-stderr\n");
    assert.deepEqual(JSON.parse(result.stdout), {
      argv: ["--flag", "space and 😀"],
      input: "request\n",
      cwd: process.cwd(),
      marker: "inherited",
      selected: "package/turbo-surf-mcp",
    });
  });
});

test("programmatic spawn returns a real child and honors caller cwd, env and pipe options", () => {
  scenario((s) => {
    const selected = installPackage(s);
    const result = s.run([
      "-e",
      `const {binaryPath,spawnMcp}=require(${JSON.stringify(entry)});if(binaryPath()!==${JSON.stringify(selected)})throw Error('selection');const child=spawnMcp(['hello world'],{cwd:${JSON.stringify(s.directory)},env:{SURF_MARKER:'override'},stdio:'pipe'});child.stdout.pipe(process.stdout);child.stderr.pipe(process.stderr);child.stdin.end('rpc body');child.on('close',code=>process.exit(code));`,
    ]);
    assert.deepEqual(readJson(result), {
      argv: ["hello world"],
      input: "rpc body",
      cwd: realpathSync(s.directory),
      marker: "override",
      selected: "package/turbo-surf-mcp",
    });
    assert.equal(result.stderr, "native-stderr\n");
  });
});

for (const [platform, arch, report, name] of [
  ["darwin", "arm64", "glibc", "turbo-surf-mcp-darwin-arm64"],
  ["linux", "x64", "musl", "turbo-surf-mcp-linux-x64-musl"],
  ["linux", "arm64", "glibc", "turbo-surf-mcp-linux-arm64"],
  ["linux", "x64", "throw", "turbo-surf-mcp-linux-x64"],
  ["win32", "x64", "glibc", "turbo-surf-mcp-win32-x64.exe"],
]) {
  test(`launcher legacy resolution selects ${name}`, () => {
    scenario((s) => {
      Object.assign(s.cfg, { platform, arch, report });
      const virtual = join(root, "bin", name);
      s.cfg.paths[virtual] = s.binary();
      const value = readJson(
        s.run([
          "-e",
          `const x=require(${JSON.stringify(entry)});console.log(JSON.stringify(x.binaryPath()))`,
        ]),
      );
      assert.equal(value, virtual);
      assert.deepEqual(readJson(s.run([cli, "--version"])).argv, ["--version"]);
    });
  });
}

test("missing optional package binary and musl variant fall back to the generic then cargo binary", () => {
  scenario((s) => {
    Object.assign(s.cfg, { platform: "linux", arch: "x64", report: "musl" });
    s.cfg.packagePath = s.file("package/package.json", "{}");
    const generic = join(root, "bin/turbo-surf-mcp-linux-x64");
    const dev = join(root, "rust/target/release/turbo-surf-mcp");
    s.cfg.paths[generic] = s.binary("generic");
    s.cfg.paths[dev] = s.binary("cargo");
    const path = () =>
      readJson(
        s.run([
          "-e",
          `console.log(JSON.stringify(require(${JSON.stringify(entry)}).binaryPath()))`,
        ]),
      );
    assert.equal(path(), generic);
    delete s.cfg.paths[generic];
    assert.equal(path(), dev);
    assert.deepEqual(readJson(s.run([cli, "--help"])).argv, ["--help"]);
  });
});

test("missing binaries report actionable errors without creating a child", () => {
  scenario((s) => {
    Object.assign(s.cfg, { platform: "freebsd", arch: "riscv64" });
    const result = s.run([cli]);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /no prebuilt binary for freebsd-riscv64/);
    assert.match(result.stderr, /cargo build --release -p turbo-surf-mcp/);
    const value = readJson(
      s.run([
        "-e",
        `const x=require(${JSON.stringify(entry)});let error;try{x.spawnMcp()}catch(e){error=e.message}console.log(JSON.stringify({path:x.binaryPath(),error}));`,
      ]),
    );
    assert.deepEqual(value, { path: null, error: "turbo-surf: no binary for freebsd-riscv64" });
  });
});

test("CLI distinguishes failed execution and signal termination from a successful exit", () => {
  scenario((s) => {
    s.cfg.paths[join(root, "bin/turbo-surf-mcp-darwin-arm64")] = s.file(
      "not-executable",
      "denied",
      0o600,
    );
    const denied = s.run([cli]);
    assert.equal(denied.status, 1);
    assert.match(denied.stderr, /turbo-surf:.*EACCES/);
    s.cfg.paths[join(root, "bin/turbo-surf-mcp-darwin-arm64")] = s.binary();
    const signaled = s.run([cli], { env: { SURF_SIGNAL: "yes" } });
    assert.equal(signaled.status, 1);
    assert.match(signaled.stderr, /native-stderr/);
  });
});

const requireLoader = `console.log(JSON.stringify(require(${JSON.stringify(loader)})))`;

test("native loader explicit selection overrides installed packages and surfaces explicit errors", () => {
  scenario((s) => {
    installPackage(s, true);
    const explicit = s.file("explicit.cjs", "module.exports={selected:'explicit'};");
    assert.deepEqual(
      readJson(s.run(["-e", requireLoader], { env: { TURBO_SURF_NAPI_PATH: explicit } })),
      { selected: "explicit" },
    );
    const missing = s.run(["-e", requireLoader], {
      env: { TURBO_SURF_NAPI_PATH: join(s.directory, "missing.node") },
    });
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /MODULE_NOT_FOUND/);
  });
});

test("native loader installed package precedes legacy and invalid installed addon falls back", () => {
  scenario((s) => {
    const installed = installPackage(s, true);
    const legacy = join(dirname(loader), "turbo-surf.darwin-arm64.node");
    s.cfg.paths[legacy] = s.file("legacy.node", '{"selected":"legacy"}');
    assert.deepEqual(readJson(s.run(["-e", requireLoader])), { selected: "package" });
    writeFileSync(installed, "invalid native payload");
    assert.deepEqual(readJson(s.run(["-e", requireLoader])), { selected: "legacy" });
  });
});

for (const [platform, arch, suffix, library] of [
  ["darwin", "arm64", "darwin-arm64", "libturbo_surf_napi.dylib"],
  ["linux", "x64", "linux-x64-gnu", "libturbo_surf_napi.so"],
  ["win32", "x64", "win32-x64-msvc", "turbo_surf_napi.dll"],
]) {
  test(`native loader ${platform} development build refreshes stale copies and reuses current copies`, () => {
    scenario((s) => {
      Object.assign(s.cfg, { platform, arch, codesignFails: true });
      const source = s.file("source-library", '{"selected":"release"}');
      const node = join(dirname(loader), "turbo-surf.dev.node");
      const snapshot = join(s.directory, "copy.node");
      s.cfg.paths[join(root, "rust/target/release", library)] = source;
      s.cfg.paths[node] = snapshot;
      assert.deepEqual(readJson(s.run(["-e", requireLoader])), { selected: "release" });
      assert.equal(readFileSync(snapshot, "utf8"), readFileSync(source, "utf8"));
      const copiedAt = statSync(snapshot).mtimeMs;
      assert.deepEqual(readJson(s.run(["-e", requireLoader])), { selected: "release" });
      assert.equal(statSync(snapshot).mtimeMs, copiedAt);
      if (platform === "darwin") {
        assert.deepEqual(JSON.parse(readFileSync(s.cfg.codesignLog, "utf8").trim()), [
          "--force",
          "-s",
          "-",
          node,
        ]);
      }
      s.cfg.codesignFails = false;
      const debug = s.file("debug-library", '{"selected":"debug"}');
      const fresh = new Date(Date.now() + 10000);
      utimesSync(debug, fresh, fresh);
      delete s.cfg.paths[join(root, "rust/target/release", library)];
      s.cfg.paths[join(root, "rust/target/debug", library)] = debug;
      assert.deepEqual(readJson(s.run(["-e", requireLoader])), { selected: "debug" });
      const legacy = join(dirname(loader), `turbo-surf.${suffix}.node`);
      s.cfg.paths[legacy] = s.file("bundled.node", '{"selected":"bundled"}');
      assert.deepEqual(readJson(s.run(["-e", requireLoader])), { selected: "bundled" });
    });
  });
}

test("native loader missing and unsupported-platform errors explain the recovery path", () => {
  scenario((s) => {
    const missing = s.run(["-e", requireLoader]);
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /no prebuilt binary found and no local cargo build/);
    assert.match(missing.stderr, /cargo build -p turbo-surf-napi/);
    Object.assign(s.cfg, { platform: "darwin", arch: "x64" });
    const unsupported = s.run(["-e", requireLoader]);
    assert.notEqual(unsupported.status, 0);
    assert.match(unsupported.stderr, /unsupported platform darwin-x64/);
  });
});
