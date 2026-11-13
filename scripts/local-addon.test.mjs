import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { localAddon } from "./local-addon.mjs";

function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), "surf-local-addon-"));
  try {
    return run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function binary(root, directory = "rust/target/release") {
  const target = join(root, directory);
  mkdirSync(target, { recursive: true });
  const path = join(target, "libturbo_surf_napi.so");
  writeFileSync(path, "newly-built-addon");
  return path;
}

test("local build selection overrides a stale optional package without changing production preference", () =>
  fixture((root) => {
    const crate = join(root, "rust/crates/turbo-surf-napi");
    mkdirSync(crate, { recursive: true });
    const loader = join(crate, "index.js");
    cpSync(new URL("../rust/crates/turbo-surf-napi/index.js", import.meta.url), loader);
    const platform = `${process.platform}-${process.arch}`;
    const suffix = {
      "darwin-arm64": "darwin-arm64",
      "linux-x64": "linux-x64-gnu",
      "linux-arm64": "linux-arm64-gnu",
      "win32-x64": "win32-x64-msvc",
    }[platform];
    const packageName = {
      "darwin-arm64": "darwin-arm64",
      "linux-x64": "linux-x64",
      "linux-arm64": "linux-arm64",
      "win32-x64": "win32-x64",
    }[platform];
    const packaged = join(root, "node_modules/@miaskiewicz", `turbo-surf-${packageName}`);
    mkdirSync(packaged, { recursive: true });
    writeFileSync(
      join(packaged, "package.json"),
      JSON.stringify({ name: `@miaskiewicz/turbo-surf-${packageName}`, version: "0.8.2" }),
    );
    writeFileSync(join(packaged, `turbo-surf.${suffix}.node`), "published-old-addon");
    assert.throws(() => localAddon(root, {}, "linux"), /Local release addon is missing/);
    binary(root);
    const selected = localAddon(root, {}, "linux");
    const script = `const fs=require('node:fs');require.extensions['.node']=(m,p)=>{m.exports=fs.readFileSync(p,'utf8')};console.log(require(${JSON.stringify(loader)}));`;
    const env = { ...process.env };
    delete env.TURBO_SURF_NAPI_PATH;
    delete env.NODE_OPTIONS;
    delete env.NODE_TEST_CONTEXT;
    delete env.NODE_PATH;
    assert.equal(
      execFileSync(process.execPath, ["-e", script], { env, encoding: "utf8" }).trim(),
      "published-old-addon",
    );
    assert.equal(
      execFileSync(process.execPath, ["-e", script], {
        env: { ...env, TURBO_SURF_NAPI_PATH: selected.path },
        encoding: "utf8",
      }).trim(),
      "newly-built-addon",
    );
  }));

test("explicit harness addons are preserved and a missing local build never falls back to a published package", () =>
  fixture((root) => {
    assert.throws(() => localAddon(root, {}, "linux"), /Local release addon is missing/);
    binary(root);
    const path = join(root, "explicit-harness.node");
    writeFileSync(path, "explicit-harness-addon");
    assert.equal(localAddon(root, { TURBO_SURF_NAPI_PATH: path }, "linux").path, path);
    assert.throws(
      () => localAddon(root, { TURBO_SURF_NAPI_PATH: join(root, "missing.node") }, "linux"),
      /Explicit native addon does not exist/,
    );
  }));

test("Cargo target-directory overrides select the actual built library and fresh bytes", () =>
  fixture((root) => {
    binary(root, "rust/isolated-target/release");
    const env = { CARGO_TARGET_DIR: "isolated-target" };
    const first = localAddon(root, env, "linux");
    assert.equal(first.source, join(root, "rust/isolated-target/release/libturbo_surf_napi.so"));
    writeFileSync(first.source, "rebuilt-addon");
    const second = localAddon(root, env, "linux");
    assert.notEqual(first.path, second.path);
    assert.equal(readFileSync(second.path, "utf8"), "rebuilt-addon");
    assert.equal(readFileSync(first.path, "utf8"), "newly-built-addon");
  }));

test("absolute Cargo target directories do not resolve beneath the Rust workspace", () =>
  fixture((root) => {
    const target = join(root, "absolute-target");
    binary(root, "absolute-target/release");
    const selected = localAddon(root, { CARGO_TARGET_DIR: target }, "linux");
    assert.equal(selected.source, join(target, "release/libturbo_surf_napi.so"));
    assert.equal(readFileSync(selected.path, "utf8"), "newly-built-addon");
  }));

test("repeated selection reuses an existing snapshot without replacing its inode", () =>
  fixture((root) => {
    binary(root);
    const first = localAddon(root, {}, "linux");
    const inode = statSync(first.path).ino;
    const second = localAddon(root, {}, "linux");
    assert.equal(second.path, first.path);
    assert.equal(statSync(first.path).ino, inode);
    assert.equal(first.sourceSha256, first.artifactSha256);
  }));

test(
  "modified signed bytes get their own content address without overwriting an earlier snapshot",
  { skip: process.platform === "win32" },
  () =>
    fixture((root) => {
      const bin = join(root, "signer-bin");
      const release = join(root, "rust/target/release");
      mkdirSync(bin);
      mkdirSync(release, { recursive: true });
      const source = join(release, "libturbo_surf_napi.dylib");
      writeFileSync(source, "unsigned-addon");
      const counter = join(root, "signature-counter");
      const signer = join(bin, "codesign");
      writeFileSync(
        signer,
        `#!/usr/bin/env node
const assert=require('node:assert/strict'),fs=require('node:fs');
assert.equal(process.argv[process.argv.indexOf('--identifier')+1],'turbo-surf.local-test-addon');
assert.ok(process.argv.includes('--timestamp=none'));
const counter=${JSON.stringify(counter)};
const next=fs.existsSync(counter)?Number(fs.readFileSync(counter,'utf8'))+1:1;
fs.writeFileSync(counter,String(next));
fs.appendFileSync(process.argv.at(-1),'|signature-'+next);
`,
      );
      chmodSync(signer, 0o755);
      const previous = process.env.PATH;
      process.env.PATH = `${bin}:${previous}`;
      try {
        const first = localAddon(root, {}, "darwin");
        const firstBytes = readFileSync(first.path);
        const inode = statSync(first.path).ino;
        const second = localAddon(root, {}, "darwin");
        assert.notEqual(first.path, second.path);
        assert.equal(first.sourceSha256, second.sourceSha256);
        assert.notEqual(first.sourceSha256, first.artifactSha256);
        assert.equal(first.artifactSha256, createHash("sha256").update(firstBytes).digest("hex"));
        assert.ok(first.path.includes(first.artifactSha256));
        assert.equal(readFileSync(first.path, "utf8"), "unsigned-addon|signature-1");
        assert.equal(readFileSync(second.path, "utf8"), "unsigned-addon|signature-2");
        assert.equal(statSync(first.path).ino, inode);
        assert.equal(readFileSync(source, "utf8"), "unsigned-addon");
      } finally {
        if (previous === undefined) delete process.env.PATH;
        else process.env.PATH = previous;
      }
    }),
);

test("a corrupt existing content-addressed snapshot fails closed instead of being overwritten", () =>
  fixture((root) => {
    binary(root);
    const selected = localAddon(root, {}, "linux");
    writeFileSync(selected.path, "corrupt snapshot");
    assert.throws(() => localAddon(root, {}, "linux"), /snapshot has invalid content/);
    assert.equal(readFileSync(selected.path, "utf8"), "corrupt snapshot");
    assert.equal(
      readdirSync(join(root, "rust/target/release")).some((name) => name.endsWith(".tmp.node")),
      false,
    );
  }));

test("bare shim tests still skip cleanly when no addon is built or installed", () =>
  fixture((root) => {
    const shim = join(root, "rust/playwright-shim");
    cpSync(new URL("../rust/playwright-shim", import.meta.url), shim, { recursive: true });
    const crate = join(root, "rust/crates/turbo-surf-napi");
    mkdirSync(crate, { recursive: true });
    cpSync(
      new URL("../rust/crates/turbo-surf-napi/index.js", import.meta.url),
      join(crate, "index.js"),
    );
    const env = { ...process.env };
    delete env.TURBO_SURF_NAPI_PATH;
    delete env.NODE_OPTIONS;
    delete env.NODE_TEST_CONTEXT;
    delete env.NODE_PATH;
    const output = execFileSync(process.execPath, ["--test", join(shim, "navigation.test.mjs")], {
      env,
      encoding: "utf8",
    });
    assert.match(output, /native addon not built, skipping/);
  }));

test(
  "the build-and-test gate passes its fresh snapshot to both test stages",
  { skip: process.platform === "win32" },
  () =>
    fixture((root) => {
      const scripts = join(root, "scripts");
      const bin = join(root, "fixture-bin");
      const crate = join(root, "rust/crates/turbo-surf-napi");
      const release = join(root, "rust/target/release");
      for (const directory of [scripts, bin, crate, release])
        mkdirSync(directory, { recursive: true });
      for (const file of ["local-addon.mjs", "test-playwright.mjs"])
        cpSync(new URL(file, import.meta.url), join(scripts, file));
      writeFileSync(
        join(scripts, "local-addon.test.mjs"),
        "import test from 'node:test';test('fixture control',()=>{});",
      );
      const loader = join(crate, "index.js");
      cpSync(new URL("../rust/crates/turbo-surf-napi/index.js", import.meta.url), loader);
      const platform = `${process.platform}-${process.arch}`;
      const packages = {
        "darwin-arm64": ["darwin-arm64", "darwin-arm64"],
        "linux-x64": ["linux-x64", "linux-x64-gnu"],
        "linux-arm64": ["linux-arm64", "linux-arm64-gnu"],
      };
      const [packageName, suffix] = packages[platform];
      const packaged = join(root, "node_modules/@miaskiewicz", `turbo-surf-${packageName}`);
      mkdirSync(packaged, { recursive: true });
      writeFileSync(
        join(packaged, "package.json"),
        JSON.stringify({ name: `@miaskiewicz/turbo-surf-${packageName}`, version: "0.8.2" }),
      );
      writeFileSync(join(packaged, `turbo-surf.${suffix}.node`), "published-old-addon");
      writeFileSync(
        join(
          release,
          process.platform === "darwin" ? "libturbo_surf_napi.dylib" : "libturbo_surf_napi.so",
        ),
        "newly-built-addon",
      );
      const stages = join(root, "stages.txt");
      writeFileSync(
        join(bin, "timeout"),
        `#!/usr/bin/env node
const assert=require('node:assert/strict'),fs=require('node:fs');
assert.deepEqual(process.argv.slice(2,6),['--signal=KILL','600','npm','run']);
require.extensions['.node']=(m,p)=>{m.exports=fs.readFileSync(p,'utf8')};
assert.equal(require(${JSON.stringify(loader)}),'newly-built-addon');
fs.appendFileSync(${JSON.stringify(stages)},process.argv.at(-1)+'\\n');
`,
      );
      writeFileSync(join(bin, "codesign"), "#!/usr/bin/env node\nprocess.exit(0);\n");
      for (const file of ["timeout", "codesign"]) chmodSync(join(bin, file), 0o755);
      const env = { ...process.env, PATH: `${bin}:${process.env.PATH}` };
      for (const name of [
        "TURBO_SURF_NAPI_PATH",
        "NODE_OPTIONS",
        "NODE_TEST_CONTEXT",
        "NODE_PATH",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET",
      ])
        delete env[name];
      execFileSync(process.execPath, [join(scripts, "test-playwright.mjs")], {
        env,
        encoding: "utf8",
      });
      assert.equal(readFileSync(stages, "utf8"), "test:shim\ntest:e2e\n");
    }),
);
