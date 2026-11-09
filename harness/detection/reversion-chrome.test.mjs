import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  EXIT,
  checkExit,
  cliArgs,
  printReport,
  transportReport,
  transportSide,
} from "./reversion-chrome.mjs";

const read = (path) => JSON.parse(readFileSync(new URL(path, import.meta.url)));
const chrome = read("./fixtures/chrome-identity-156.json");
const recorded = read("./TRANSPORT-156.json");

const printed = (report) => {
  const lines = [];
  const log = console.log;
  console.log = (text) => lines.push(text);
  try {
    printReport(report);
  } finally {
    console.log = log;
  }
  return lines;
};

test("transportSide keeps the recorded Chrome hello and skips a side without an h2 hello", () => {
  const side = transportSide("chrome", chrome, 156);
  assert.deepEqual(
    { fixture: side.raw.fixture, sha256: side.raw.sha256, bytes: side.raw.bytes },
    recorded.rawRecords.chrome,
  );
  assert.equal(side.raw.data.length, side.raw.bytes);
  assert.deepEqual(side.record, recorded.fingerprints.chrome);
  // Native rows only count on the no-JS tier.
  assert.equal(transportSide("native", chrome, 156), null);
  assert.equal(transportSide("chrome", { ...chrome, connections: [] }, 156), null);
});

test("transportReport pairs sides, filters transport deltas and drops raw bytes", () => {
  const native = structuredClone(chrome);
  for (const request of native.requests) request.tier = "nojs";
  native.build = { transportBaseChromeMajor: 150 };
  const parity = {
    summary: { tell: 1 },
    deltas: [
      { area: "transport", path: "p", expected: 1, actual: 2, class: "tell", extra: true },
      { area: "http", path: "q", expected: 1, actual: 2, class: "tell" },
    ],
  };
  const { report, raw } = transportReport(chrome, native, parity, { dependency: "d" });
  assert.deepEqual(Object.keys(raw), ["chrome", "native"]);
  assert.equal(report.rawRecords.native.fixture, "fixtures/transport-native156-client-hello.bin");
  assert.equal("data" in report.rawRecords.chrome, false);
  assert.deepEqual(report.tls.native, report.tls.chrome);
  assert.deepEqual(report.http2.chrome, recorded.http2.chrome);
  assert.equal(report.upstreamBaseTemplateMajor, 150);
  assert.equal(report.dependency, "d");
  assert.deepEqual(report.differences, [{ path: "p", expected: 1, actual: 2, class: "tell" }]);
  assert.deepEqual(report.identityParity, { tell: 1 });

  const alone = transportReport(chrome, null, null, recorded).report;
  assert.deepEqual(Object.keys(alone.rawRecords), ["chrome"]);
  assert.equal(alone.tls.native, undefined);
  assert.equal(alone.nativeBuild, null);
  assert.equal(alone.differences, null);
  assert.equal(alone.identityParity, null);
  assert.equal(alone.upstreamBaseTemplateMajor, recorded.upstreamBaseTemplateMajor);
});

test("printReport prints only the sections a report carries", () => {
  assert.deepEqual(
    printed({ chrome: { version: "156.0.1.2" }, status: "current", registryDefault: 156 }),
    ["Chrome 156.0.1.2 (current); registry default 156"],
  );
  const lines = printed({
    chrome: { version: "157.0.1.2" },
    status: "new-major",
    registryDefault: 156,
    oracle: { frozen: "reused", executable: "/oracle" },
    versionDiff: {
      from: { full: "156.0.1.2" },
      to: { full: "157.0.1.2" },
      deltas: [
        { action: "human", area: "http", type: "header-extra", kind: "nav", header: "x-new" },
        { action: "info", area: "js", type: "value" },
      ],
    },
    chromeDrift: [{}],
    parity: {
      summary: { tell: 1 },
      deltas: [
        { class: "tell", area: "http", type: "value", tier: "nojs", origin: "A", path: "/p" },
        { class: "known", area: "http", type: "value" },
      ],
    },
    engine: { status: "skipped" },
    engineIdentity: { matches: true },
    blockers: [{ reason: "needs a human" }],
  });
  assert.deepEqual(lines, [
    "Chrome 157.0.1.2 (new-major); registry default 156",
    "oracle: reused /oracle",
    "\n156.0.1.2 → 157.0.1.2: 2 release deltas",
    "  [human] http header-extra nav x-new",
    "  [info] js value",
    "\nChrome drift vs its fixture: 1 deltas",
    '\nturbo-surf parity: {"tell":1}',
    "  [tell] http value nojs A /p",
    '\nengine step: skipped; {"matches":true}',
    "\nSTOPPED — these deltas need a human (see CHROME-REVERSION.md, 'Handling deltas'):",
    "  - needs a human",
  ]);
});

test("checkExit ranks a newer Chrome over Chrome drift over native drift", () => {
  assert.equal(checkExit({ status: "new-build", chromeDrift: [{}] }), EXIT.newer);
  assert.equal(checkExit({ status: "new-major" }), EXIT.newer);
  assert.equal(
    checkExit({ status: "current", chromeDrift: [{}], parity: { summary: { tell: 1 } } }),
    EXIT.chromeDrift,
  );
  assert.equal(
    checkExit({ status: "current", chromeDrift: [], parity: { summary: { tell: 1 } } }),
    EXIT.nativeDrift,
  );
  assert.equal(checkExit({ status: "older", parity: { summary: { tell: 0 } } }), EXIT.ok);
});

test("cliArgs reads switches and the value after a flag", () => {
  const cli = cliArgs(["--chrome", "/c", "--check", "--native"]);
  assert.equal(cli.has("--check"), true);
  assert.equal(cli.has("--offline"), false);
  assert.equal(cli.flag("--chrome"), "/c");
  assert.equal(cli.flag("--native"), undefined);
  assert.equal(cli.flag("--engine-mode"), undefined);
});
