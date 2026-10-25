// Durable, reproducible backlog from a frozen paired localhost capture.
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export function inventorySnapshot(comparison) {
  const { members, differences, chrome } = comparison;
  if (!members || !Array.isArray(differences) || !chrome?.constructors)
    throw new TypeError("Expected a paired API comparison with member evidence");
  const entries = Object.entries(members);
  const unmeasured = entries.filter(([, item]) => !item.measured);
  const original = ["Document", "Element", "HTMLElement", "HTMLCanvasElement"];
  return {
    schemaVersion: 1,
    collectedAt: comparison.collectedAt,
    note: comparison.note,
    build: comparison.build,
    chromeVersion: comparison.chromeVersion,
    chromeV8Version: comparison.chromeV8Version,
    binaryHash: comparison.binaryHash,
    collectorHash: comparison.collectorHash,
    totals: {
      interfaceCount: entries.length,
      originalFourMissing: original.reduce(
        (sum, name) => sum + (members[name]?.missing?.length || 0),
        0,
      ),
      expandedMeasuredMissing: entries.reduce(
        (sum, [, item]) => sum + (item.measured ? item.missing.length : 0),
        0,
      ),
      unmeasuredInterfaces: unmeasured.map(([name]) => name),
      differencePaths: differences.length,
    },
    interfaces: members,
    unmeasuredChromeMembers: Object.fromEntries(
      unmeasured.map(([name]) => [
        name,
        Object.keys(chrome.constructors[name]?.prototypeMembers || {}),
      ]),
    ),
    // Retain global ownership, callable and key-order differences too. An own-member
    // summary alone silently loses these observed gaps between successive runs.
    differences,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [input, output] = process.argv.slice(2);
  if (!input || !output) throw new Error("Usage: api-inventory.mjs comparison.json output.json");
  const snapshot = inventorySnapshot(JSON.parse(readFileSync(input, "utf8")));
  writeFileSync(output, `${JSON.stringify(snapshot, null, 2)}\n`);
  console.log(JSON.stringify(snapshot.totals));
}
