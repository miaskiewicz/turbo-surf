// Private analysis: function names can contain page data. This is not a publication sanitizer.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export function summarizeCpuProfile(profile, limit = 20) {
  if (!Number.isSafeInteger(limit) || limit < 1) throw new Error("limit must be positive");
  if (
    !Array.isArray(profile.nodes) ||
    !Array.isArray(profile.samples) ||
    !Array.isArray(profile.timeDeltas) ||
    profile.samples.length === 0 ||
    profile.samples.length !== profile.timeDeltas.length ||
    !Number.isFinite(profile.startTime) ||
    !Number.isFinite(profile.endTime) ||
    profile.endTime < profile.startTime
  )
    throw new Error("CPU profile requires valid bounds and aligned samples/timeDeltas");
  const nodes = new Map();
  for (const node of profile.nodes) {
    const frame = node.callFrame;
    if (
      !Number.isSafeInteger(node.id) ||
      nodes.has(node.id) ||
      !frame ||
      typeof frame.functionName !== "string" ||
      typeof frame.scriptId !== "string" ||
      !Number.isSafeInteger(frame.lineNumber) ||
      !Number.isSafeInteger(frame.columnNumber)
    )
      throw new Error("CPU profile has invalid or duplicate nodes");
    nodes.set(node.id, frame);
  }
  const groups = new Map();
  const samples = [];
  let timestamp = profile.startTime;
  let negativeDeltaCount = 0;
  for (const [index, id] of profile.samples.entries()) {
    const delta = profile.timeDeltas[index];
    if (!nodes.has(id) || !Number.isFinite(delta))
      throw new Error("CPU profile has an unknown sample or invalid time delta");
    timestamp += delta;
    if (timestamp < profile.startTime || timestamp > profile.endTime)
      throw new Error("CPU profile sample timestamp falls outside the profile bounds");
    if (delta < 0) negativeDeltaCount++;
    samples.push({ id, timestamp });
  }
  // Inspector samples can arrive out of order. Like DevTools' CPUProfileDataModel,
  // sort timestamps together with their samples rather than dropping negative deltas.
  samples.sort((a, b) => a.timestamp - b.timestamp);
  let previousTimestamp = profile.startTime;
  let sampledMicroseconds = 0;
  for (const { id, timestamp } of samples) {
    const frame = nodes.get(id);
    const delta = timestamp - previousTimestamp;
    previousTimestamp = timestamp;
    const identity = {
      functionName: frame.functionName,
      scriptId: frame.scriptId,
      lineNumber: frame.lineNumber,
      columnNumber: frame.columnNumber,
    };
    const key = JSON.stringify(identity);
    let group = groups.get(key);
    if (!group) {
      group = { ...identity, selfMicroseconds: 0, sampleCount: 0 };
      groups.set(key, group);
    }
    group.selfMicroseconds += delta;
    group.sampleCount++;
    sampledMicroseconds += delta;
  }
  return {
    scope:
      "Sampled self time grouped across call stacks; no inclusive-time sum or call-count inference",
    profileDurationMs: (profile.endTime - profile.startTime) / 1000,
    sampledDurationMs: sampledMicroseconds / 1000,
    uncoveredTailMs: (profile.endTime - previousTimestamp) / 1000,
    negativeDeltaCount,
    timestampsReordered: negativeDeltaCount > 0,
    sampleCount: profile.samples.length,
    nodeCount: nodes.size,
    hottest: [...groups.values()]
      .sort((a, b) => b.selfMicroseconds - a.selfMicroseconds)
      .slice(0, limit)
      .map(({ selfMicroseconds, ...group }) => ({
        ...group,
        selfMs: selfMicroseconds / 1000,
        sampledShare: sampledMicroseconds ? selfMicroseconds / sampledMicroseconds : 0,
      })),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3)
    throw new Error("Usage: node cpu-profile-summary.mjs profile.json");
  console.log(
    JSON.stringify(summarizeCpuProfile(JSON.parse(readFileSync(process.argv[2], "utf8"))), null, 2),
  );
}
