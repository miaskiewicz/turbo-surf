// Same-scope comparison of two API-INVENTORY snapshots. Interfaces measured in only
// one snapshot are listed separately, so a larger collector scope never reads as
// resolved or introduced members. Identity changes are reported, never reconciled.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const minus = (left, right) => {
  const exclude = new Set(right);
  return left.filter((item) => !exclude.has(item));
};
const measured = (snapshot) =>
  Object.keys(snapshot.interfaces || {}).filter((name) => snapshot.interfaces[name].measured);
const changedPaths = (item) => item.changed.map((change) => change.path);

// Constructor paths belong to their interface; other paths (globals, methods,
// contracts) belong to the collector itself and compare only when it is unchanged.
function pathScope(path, shared) {
  const match = /^constructors\.([^.]+)/.exec(path);
  return match ? shared.has(match[1]) : null;
}

function valueChanged(previous, current, comparable) {
  const now = new Map(current.differences.map((item) => [item.path, item]));
  return previous.differences.filter(
    (item) =>
      comparable(item.path) &&
      now.has(item.path) &&
      JSON.stringify(item) !== JSON.stringify(now.get(item.path)),
  );
}

function identity(snapshot) {
  return {
    chromeVersion: snapshot.chromeVersion,
    chromeV8Version: snapshot.chromeV8Version,
    v8Version: snapshot.build?.engine?.v8Version,
    stableWindowProxy: snapshot.build?.engine?.stableWindowProxy,
    features: snapshot.build?.features,
    collectorHash: snapshot.collectorHash,
    binaryHash: snapshot.binaryHash,
  };
}

export function compareInventories(previous, current) {
  const before = measured(previous);
  const after = new Set(measured(current));
  const shared = new Set(before.filter((name) => after.has(name)));
  const interfaces = {};
  const sum = { previousMissing: 0, currentMissing: 0, resolved: 0, introduced: 0 };
  for (const name of [...shared].sort()) {
    const old = previous.interfaces[name];
    const now = current.interfaces[name];
    const entry = {
      resolvedMissing: minus(old.missing, now.missing),
      introducedMissing: minus(now.missing, old.missing),
      resolvedExtra: minus(old.extra, now.extra),
      introducedExtra: minus(now.extra, old.extra),
      resolvedChanged: minus(changedPaths(old), changedPaths(now)),
      introducedChanged: minus(changedPaths(now), changedPaths(old)),
    };
    sum.previousMissing += old.missing.length;
    sum.currentMissing += now.missing.length;
    sum.resolved += entry.resolvedMissing.length;
    sum.introduced += entry.introducedMissing.length;
    if (Object.values(entry).some((list) => list.length)) interfaces[name] = entry;
  }
  const sameCollector = previous.collectorHash === current.collectorHash;
  const comparable = (path) => pathScope(path, shared) ?? sameCollector;
  const oldPaths = previous.differences.map((item) => item.path).filter(comparable);
  const newPaths = current.differences.map((item) => item.path).filter(comparable);
  const oldIdentity = identity(previous);
  const newIdentity = identity(current);
  return {
    previous: oldIdentity,
    current: newIdentity,
    identityChanges: Object.keys(oldIdentity).filter(
      (key) => JSON.stringify(oldIdentity[key]) !== JSON.stringify(newIdentity[key]),
    ),
    sameScope: {
      interfaceCount: shared.size,
      ...sum,
      previousDifferencePaths: oldPaths.length,
      currentDifferencePaths: newPaths.length,
      resolvedDifferencePaths: minus(oldPaths, newPaths).length,
      introducedDifferencePaths: minus(newPaths, oldPaths).length,
      // A shared path can still change value, e.g. a key order that moved closer.
      valueChangedDifferencePaths: valueChanged(previous, current, comparable).length,
      globalPathsCompared: sameCollector,
    },
    interfaces,
    onlyPrevious: minus(before, [...after]).sort(),
    onlyCurrent: [...after].filter((name) => !shared.has(name)).sort(),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [previous, current] = process.argv.slice(2);
  if (!previous || !current)
    throw new Error("Usage: api-inventory-diff.mjs previous.json current.json");
  const read = (path) => JSON.parse(readFileSync(path, "utf8"));
  console.log(JSON.stringify(compareInventories(read(previous), read(current)), null, 2));
}
