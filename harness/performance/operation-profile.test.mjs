import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { createOperationProfile } from "./operation-profile.mjs";

test("preserves receiver, arguments, result, errors and original descriptors", () => {
  let clock = 0;
  const profile = createOperationProfile(() => clock);
  const failure = new Error("private contents must not be recorded");
  class Probe {
    read(value) {
      clock += 2;
      return this === receiver ? value : null;
    }
    fail() {
      clock += 3;
      throw failure;
    }
  }
  const receiver = new Probe();
  const original = Object.getOwnPropertyDescriptor(Probe.prototype, "read");
  profile.wrap(Probe.prototype, ["read", "fail", "missing"], "Probe");
  assert.equal(receiver.read(failure), failure);
  assert.throws(
    () => receiver.fail(),
    (error) => error === failure,
  );
  const records = profile.snapshot();
  assert.equal(records["Probe.read"].elapsedMs, 2);
  assert.equal(records["Probe.fail"].errors, 1);
  assert.equal(JSON.stringify(records).includes("private contents"), false);
  profile.restore();
  assert.deepEqual(Object.getOwnPropertyDescriptor(Probe.prototype, "read"), original);
});

test("preserves Promise identity and accounts nested async calls inclusively", async () => {
  let clock = 0;
  const profile = createOperationProfile(() => clock);
  let returned;
  class Probe {
    child() {
      clock += 2;
      return 4;
    }
    parent() {
      returned = Promise.resolve().then(() => {
        this.child();
        clock += 3;
        return 7;
      });
      return returned;
    }
  }
  profile.wrap(Probe.prototype, ["parent", "child"], "Probe");
  const result = new Probe().parent();
  assert.equal(result, returned);
  assert.equal(await result, 7);
  const records = profile.snapshot();
  assert.equal(records["Probe.parent"].elapsedMs, 5);
  assert.equal(records["Probe.parent"].rootCalls, 1);
  assert.equal(records["Probe.child"].elapsedMs, 2);
  assert.equal(records["Probe.child"].rootCalls, 0);
  profile.restore();
});

test("rejected and in-flight promises are represented without error details", async () => {
  const profile = createOperationProfile(() => 1);
  let release;
  const failure = new Error("secret");
  let rejected;
  class Probe {
    pending() {
      return new Promise((resolve) => {
        release = resolve;
      });
    }
    fail() {
      rejected = Promise.reject(failure);
      return rejected;
    }
  }
  profile.wrap(Probe.prototype, ["pending", "fail"], "Probe");
  const receiver = new Probe();
  const pending = receiver.pending();
  assert.equal(profile.snapshot()["Probe.pending"].completed, 0);
  const result = receiver.fail();
  assert.equal(result, rejected);
  await assert.rejects(result, (error) => error === failure);
  assert.equal(profile.snapshot()["Probe.fail"].errors, 1);
  assert.equal(JSON.stringify(profile.snapshot()).includes("secret"), false);
  release();
  await pending;
  assert.equal(profile.snapshot()["Probe.pending"].completed, 1);
  profile.restore();
});

test("preload is inert without opt-in and writes bounded labels for an owned diagnostic", () => {
  const root = mkdtempSync(join(tmpdir(), "surf-operation-profile-"));
  try {
    const preload = new URL("auth-operation-profile.mjs", import.meta.url).pathname;
    const inactive = spawnSync(process.execPath, ["--import", preload, "--eval", ""], {
      env: {},
      encoding: "utf8",
    });
    assert.equal(inactive.status, 0, inactive.stderr);
    assert.deepEqual(readdirSync(root), []);
    const shim = join(root, "shim.mjs");
    writeFileSync(
      shim,
      "export class Page { goto(value) { return Promise.resolve(value); } } export class Locator {}",
    );
    const output = join(root, "reports");
    const active = spawnSync(
      process.execPath,
      [
        "--import",
        preload,
        "--input-type=module",
        "--eval",
        `const {Page} = await import(${JSON.stringify(pathToFileURL(shim).href)}); await new Page().goto('secret');`,
      ],
      {
        env: { TURBO_SURF_OPERATION_PROFILE_DIR: output, TURBO_SURF_OPERATION_PROFILE_SHIM: shim },
        encoding: "utf8",
      },
    );
    assert.equal(active.status, 0, active.stderr);
    const files = readdirSync(output);
    assert.equal(files.length, 1);
    const raw = readFileSync(join(output, files[0]), "utf8");
    const record = JSON.parse(raw);
    assert.equal(record.acceptanceEligible, false);
    assert.equal(record.operations["Page.goto"].calls, 1);
    assert.equal(record.operations["Page.goto"].completed, 1);
    assert.equal(raw.includes("secret"), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
