import assert from "node:assert/strict";
import test from "node:test";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

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

test(
  "keyboard-driven loopback controls can run headless, away from the OS keyboard focus",
  { todo: "turbo-surf#51" },
  () => {
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
  },
);
