import assert from "node:assert/strict";
import { test } from "node:test";
import { summarizeCpuProfile } from "./cpu-profile-summary.mjs";

function profile() {
  const frame = (functionName, lineNumber) => ({
    functionName,
    scriptId: "8",
    lineNumber,
    columnNumber: 0,
    url: "https://private.invalid/?credential=PRIVATE_PROFILE_URL",
  });
  return {
    startTime: 1000,
    endTime: 2000,
    nodes: [
      { id: 1, callFrame: frame("computeStyle", 153), children: [2] },
      { id: 2, callFrame: frame("computeStyle", 153), children: [3] },
      { id: 3, callFrame: frame("get", 10) },
    ],
    samples: [1, 2, 3],
    timeDeltas: [100, 200, 700],
  };
}

test("recursive frames aggregate sampled self time without double-counting inclusive time", () => {
  const summary = summarizeCpuProfile(profile());
  assert.equal(summary.sampleCount, 3);
  assert.equal(summary.sampledDurationMs, 1);
  assert.equal(summary.profileDurationMs, 1);
  assert.deepEqual(
    summary.hottest.map(({ functionName, selfMs, sampleCount }) => [
      functionName,
      selfMs,
      sampleCount,
    ]),
    [
      ["get", 0.7, 1],
      ["computeStyle", 0.3, 2],
    ],
  );
  assert.equal(summary.hottest[0].sampledShare, 0.7);
  assert.doesNotMatch(JSON.stringify(summary), /private\.invalid|PRIVATE_PROFILE_URL/);
  assert.equal(summarizeCpuProfile(profile(), 1).hottest.length, 1);
});

test("incomplete or corrupted sampling evidence fails instead of reporting a false zero cost", () => {
  for (const mutate of [
    (value) => value.timeDeltas.pop(),
    (value) => {
      value.samples[0] = 99;
    },
    (value) => {
      value.timeDeltas[0] = -1;
    },
    (value) => {
      value.timeDeltas[0] = NaN;
    },
    (value) => {
      value.endTime = 0;
    },
    (value) => value.nodes.push(value.nodes[0]),
    (value) => {
      value.nodes[0].callFrame = {};
    },
    (value) => {
      value.samples = [];
      value.timeDeltas = [];
    },
  ]) {
    const value = profile();
    mutate(value);
    assert.throws(() => summarizeCpuProfile(value));
  }
});

test("out-of-order Inspector timestamps stay paired with their sampled frames", () => {
  const value = profile();
  value.timeDeltas = [200, -100, 900];
  value.endTime = 2200;
  const summary = summarizeCpuProfile(value);
  assert.equal(summary.negativeDeltaCount, 1);
  assert.equal(summary.timestampsReordered, true);
  assert.equal(summary.sampledDurationMs, 1);
  assert.equal(summary.uncoveredTailMs, 0.2);
  assert.deepEqual(
    summary.hottest.map(({ functionName, selfMs }) => [functionName, selfMs]),
    [
      ["get", 0.8],
      ["computeStyle", 0.2],
    ],
  );
});
