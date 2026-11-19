import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(
  new URL("../crates/turbo-surf-render/src/runtime.rs", import.meta.url),
  "utf8",
);
const start = source.indexOf("__G.loadDocumentResources = function(");
const script = source.slice(start, source.indexOf("\n__G.pendingResourceCount", start));

test("resource scans skip CSS host getters for unchanged inline styles", async () => {
  const document = {};
  const styles = Array.from({ length: 5 }, (_, index) => ({
    source: `.c${index}{color:red}`,
    sheet: {},
  }));
  let reads = 0;
  const imports = [];
  const context = {
    __G: {},
    __document: document,
    __documentType: () => 9,
    __intrinsicApply: Reflect.apply,
    __discoverDictionaries() {},
    __list: () => ({ length: 0 }),
    __listPush: (list, value) => {
      list[list.length++] = value;
    },
    __resourceMutationEpoch: 0,
    __resourceDocumentQuery: () => styles,
    __resourceListApply: Reflect.apply,
    __resourceListLength() {
      return this.length;
    },
    __resourceListItem(index) {
      return this[index];
    },
    __resourceTagName: () => "STYLE",
    __resourceLocalName: () => "style",
    __resourceText() {
      return this.source;
    },
    __cssHost: () => ({
      sheet: (element) => {
        reads++;
        return element.sheet;
      },
    }),
    __intrinsicWeakGet: WeakMap.prototype.get,
    __intrinsicWeakSet: WeakMap.prototype.set,
    __resources: new WeakMap(),
    __resourceStyleRequests: new WeakMap(),
    __trackResource: (promise) => promise,
    __loadSheetImports: async (sheet) => {
      imports.push(sheet);
    },
    __resourceBaseURL: () => "https://example.test/",
    __framePromiseAll: (list) =>
      Promise.all(Array.from({ length: list.length }, (_, index) => list[index])),
    ops: { op_document_request_url: () => "https://example.test/" },
  };
  runInNewContext(script, context);
  const scan = () => context.__G.loadDocumentResources(document);
  await scan();
  assert.equal(reads, 5);
  await scan();
  assert.equal(
    reads,
    5,
    "an unchanged source must be checked before its expensive CSS host getter",
  );
  styles[2].source = ".changed{color:blue}";
  await scan();
  assert.equal(reads, 6);
  assert.equal(imports.length, 6);
  styles.push({ source: ".added{color:green}", sheet: {} });
  await scan();
  assert.equal(reads, 7);
  assert.equal(imports.length, 7);
  const pending = { source: ".pending{color:black}", sheet: null };
  styles.push(pending);
  await scan();
  await scan();
  assert.equal(reads, 9, "an unavailable sheet is retried rather than cached as loaded");
  pending.sheet = {};
  await scan();
  assert.equal(reads, 10);
  assert.equal(imports.length, 8);
});
