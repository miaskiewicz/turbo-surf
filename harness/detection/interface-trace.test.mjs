import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

test("interface tracer preserves and separates immediate host and page call sites", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `class Document {querySelectorAll(){return [];}}; globalThis.Document=Document;globalThis.document=new Document();globalThis.navigator={};globalThis.screen={};globalThis.performance={now:()=>0};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context, {
    filename: "interface-trace.js",
  });
  vm.runInContext(`document.querySelectorAll('style')`, context, { filename: "<env>" });
  vm.runInContext(`document.querySelectorAll('style')`, context, {
    filename: "https://example.test/app.js",
  });
  const rows = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot().accesses)`,
      context,
    ),
  );
  const calls = rows.filter((row) => row.name === "Document.querySelectorAll");
  assert.deepEqual(
    calls.map((row) => [row.origin, row.count]),
    [
      ["runtime", 1],
      ["page", 1],
    ],
  );
  assert.match(calls[0].stack, /<env>/);
  assert.match(calls[1].stack, /example\.test/);
});

test("interface tracer records supported own declaration methods without installer noise", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `class CSSStyleRule {get style(){return this.declaration;}}; globalThis.CSSStyleRule=CSSStyleRule;
    globalThis.rule=new CSSStyleRule();rule.declaration={value:'red',getPropertyValue(name){return this.value;}};
    Object.defineProperty(rule.declaration,'cssText',{get(){return 'color: red;';}});
    globalThis.document={};globalThis.navigator={};globalThis.screen={};globalThis.performance={now:()=>0};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context, {
    filename: "interface-trace.js",
  });
  assert.equal(vm.runInContext(`rule.style.getPropertyValue('color')`, context), "red");
  assert.equal(vm.runInContext(`rule.style.getPropertyValue('color')`, context), "red");
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  const calls = snapshot.accesses.filter(
    (row) => row.name === "CSSStyleDeclaration.getPropertyValue",
  );
  assert.equal(calls.length, 1);
  assert.equal(calls[0].count, 2);
  assert.ok(snapshot.skipped.includes("CSSStyleDeclaration.cssText"));
  assert.equal(
    snapshot.accesses.some((row) => row.name === "Object.getOwnPropertyDescriptor"),
    false,
  );
});

test("interface tracing installs in worker globals without DOM interfaces", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.performance={now:()=>0};globalThis.navigator={language:'en-US'};globalThis.fetch=()=>Promise.resolve('body');`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  vm.runInContext(`fetch('https://example.test/')`, context, {
    filename: "https://example.test/worker.js",
  });
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  assert.equal(snapshot.accesses.find((row) => row.name === "window.fetch").count, 1);
});
