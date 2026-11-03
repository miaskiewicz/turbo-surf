import assert from "node:assert/strict";
import test from "node:test";
import { chromeExtensionArgs, chromeExtraArgs, parseChromeArgs } from "./chrome-launch-policy.mjs";

test("normal Chrome launch keeps extensions enabled by default", () => {
  for (const disableExtensions of [undefined, false, "true", "false"])
    assert.deepEqual(
      chromeExtensionArgs({ url: "https://www.google.com/", disableExtensions }),
      [],
    );
});

test("authored loopback controls can explicitly disable extensions", () => {
  for (const url of ["http://localhost:1234/", "http://127.0.0.1:1234/", "http://[::1]:1234/"])
    assert.deepEqual(chromeExtensionArgs({ url, disableExtensions: true }), [
      "--disable-extensions",
      "--disable-component-extensions-with-background-pages",
    ]);
});

test("extension isolation refuses remote, lookalike and file controls", () => {
  for (const url of [
    "https://www.google.com/",
    "http://localhost.example/",
    "file:///tmp/fixture.html",
  ])
    assert.throws(
      () => chromeExtensionArgs({ url, disableExtensions: true }),
      /authored localhost/,
    );
});

test("frame-timing controls opt into foreground scheduling only on loopback", () => {
  assert.deepEqual(chromeExtensionArgs({ url: "http://127.0.0.1:1/", foregroundFrames: true }), [
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
    "--disable-background-timer-throttling",
  ]);
  assert.deepEqual(
    chromeExtensionArgs({ url: "https://www.google.com/", foregroundFrames: "true" }),
    [],
  );
  assert.throws(
    () => chromeExtensionArgs({ url: "https://www.google.com/", foregroundFrames: true }),
    /foregroundFrames is restricted to authored localhost/,
  );
});

test("keyboard-driven loopback controls can run headless, away from the OS keyboard focus", () => {
  for (const url of ["http://localhost:1/", "http://127.0.0.1:1/", "http://[::1]:1/"])
    assert.deepEqual(chromeExtensionArgs({ url, headless: true }), ["--headless=new"]);
  assert.deepEqual(
    chromeExtensionArgs({ url: "http://127.0.0.1:1/", disableExtensions: true, headless: true }),
    [
      "--disable-extensions",
      "--disable-component-extensions-with-background-pages",
      "--headless=new",
    ],
  );
  for (const headless of [undefined, false, "true", 1])
    assert.deepEqual(chromeExtensionArgs({ url: "https://www.google.com/", headless }), []);
  for (const url of ["https://www.google.com/", "http://localhost.example/", "file:///tmp/x"])
    assert.throws(
      () => chromeExtensionArgs({ url, headless: true }),
      /headless is restricted to authored localhost/,
    );
});

test("extra Chrome args default to none", () => {
  assert.deepEqual(chromeExtraArgs({ url: "https://www.google.com/" }, {}), []);
  assert.deepEqual(parseChromeArgs(["--live", "--chrome-incognito"], {}), []);
});

test("extra Chrome args pass through from env, both CLI spellings, in order", () => {
  assert.deepEqual(
    parseChromeArgs(
      ["--live", "--chrome-arg=--use-mock-keychain", "--chrome-arg", "--password-store=basic"],
      { TURBO_SURF_CHROME_ARGS: " --disable-sync  --foo=a " },
    ),
    ["--disable-sync", "--foo=a", "--use-mock-keychain", "--password-store=basic"],
  );
  const chromeArgs = ["--use-mock-keychain", "--password-store=basic"];
  const out = chromeExtraArgs({ url: "https://www.google.com/", chromeArgs });
  assert.deepEqual(out, chromeArgs);
  assert.notEqual(out, chromeArgs);
});

test("extra Chrome args reject non-flags and a missing value", () => {
  for (const chromeArgs of [["about:blank"], ["-x"], ["--"], [3], "--a"])
    assert.throws(() => chromeExtraArgs({ chromeArgs }), TypeError);
  assert.throws(() => parseChromeArgs(["--chrome-arg"], {}), /requires/);
  assert.throws(() => parseChromeArgs(["--chrome-arg=nope"], {}), TypeError);
});

test("direct collector configs fall back to TURBO_SURF_CHROME_ARGS; explicit config wins", () => {
  const env = { TURBO_SURF_CHROME_ARGS: "--use-mock-keychain --password-store=basic" };
  assert.deepEqual(chromeExtraArgs({}, env), ["--use-mock-keychain", "--password-store=basic"]);
  assert.deepEqual(chromeExtraArgs({ chromeArgs: [] }, env), []);
  assert.throws(() => chromeExtraArgs({}, { TURBO_SURF_CHROME_ARGS: "bad" }), TypeError);
});
