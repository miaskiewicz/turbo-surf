import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import crypto from "node:crypto";

const require = createRequire(import.meta.url);
export const EXPECTED = {
  answer: 42,
  text: "Native 42",
  crypto: true,
  stream: true,
  private: true,
};
const HTML = '<!doctype html><body><p id="probe">Before</p></body>';
const SCRIPT = `(() => {
  const answer = 6 * 7;
  const node = document.getElementById('probe');
  node.textContent = 'Native ' + answer;
  const bytes = new Uint8Array(8);
  const stream = new ReadableStream({start(controller) { controller.close(); }});
  return JSON.stringify({answer, text: node.textContent,
    crypto: crypto.getRandomValues(bytes) === bytes,
    stream: stream instanceof ReadableStream && !stream.locked,
    private: typeof Deno === 'undefined'});
})()`;

export function verifyRuntime(addon, expectedVersion) {
  assert.equal(addon.version(), expectedVersion, "Native addon release version differs");
  const result = JSON.parse(addon.evaluate(HTML, SCRIPT));
  assert.deepEqual(result, EXPECTED, "Native DOM/V8/crypto/stream contract failed");
  return result;
}

export function smokeAddon(addonPath, version, sourceCommit) {
  assert.match(version, /^\d+\.\d+\.\d+$/, "Expected an exact release version");
  assert.match(sourceCommit, /^[0-9a-f]{40}$/, "Expected the exact source commit");
  const actual = fs.realpathSync(addonPath);
  assert.equal(path.extname(actual), ".node", "Expected the actual compiled addon");
  const before = crypto.createHash("sha256").update(fs.readFileSync(actual)).digest("hex");
  const result = verifyRuntime(require(actual), version);
  const after = crypto.createHash("sha256").update(fs.readFileSync(actual)).digest("hex");
  assert.equal(after, before, "Native addon changed during its runtime smoke");
  return {
    passed: true,
    sourceCommit,
    expectedVersion: version,
    addon: actual,
    addonSha256: before,
    nodeVersion: process.version,
    nodeModulesAbi: process.versions.modules,
    platform: process.platform,
    arch: process.arch,
    result,
  };
}

function main() {
  const [addon, version, sourceCommit, output] = process.argv.slice(2);
  assert.ok(
    addon && version && sourceCommit && output,
    "Expected addon, version, commit and receipt arguments",
  );
  const receipt = smokeAddon(addon, version, sourceCommit);
  fs.writeFileSync(output, `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx" });
  console.log(JSON.stringify(receipt));
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) main();
