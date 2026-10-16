import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
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

test("source-less native script pump is runtime but its real page callee stays page", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.clock=0;class Document {querySelectorAll(){clock+=3;return document;}}
    globalThis.Document=Document;globalThis.document=new Document();
    globalThis.navigator={};globalThis.screen={};globalThis.performance={now:()=>clock};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  vm.runInContext(`function googlePage(){return document.querySelectorAll('page');}`, context, {
    filename: "https://example.test/app.js",
  });
  vm.runInContext(
    `globalThis.__G={};__G.execScriptEl=function(){
      const hostResult=document.querySelectorAll('script');
      return hostResult===googlePage();
    };`,
    context,
    { filename: "<anonymous>" },
  );
  assert.equal(vm.runInContext(`__G.execScriptEl()`, context), true);
  const rows = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot().accesses)`,
      context,
    ),
  ).filter((row) => row.name === "Document.querySelectorAll");
  assert.deepEqual(
    rows.map((row) => [row.origin, row.count, row.durationMs]),
    [
      ["runtime", 1, 3],
      ["page", 1, 3],
    ],
  );
  assert.match(rows[0].stack.split("\n")[0], /at (?:Object\.)?__G\.execScriptEl \(<anonymous>:/);
  assert.match(rows[1].stack.split("\n")[0], /at googlePage \(https:\/\/example\.test\/app\.js:/);
  assert.match(rows[1].stack, /(?:Object\.)?__G\.execScriptEl/);
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

test("interface durations bracket synchronous calls, getters and throws without changing results", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.clock=0;globalThis.order=[];globalThis.value={};globalThis.failure={};
    globalThis.promise=Promise.resolve(value);
    class Document {
      querySelectorAll(){order.push('call');clock+=7;return value;}
      get URL(){order.push('get');clock+=3;return 'document';}
      set cookie(value){order.push('set');clock+=5;throw failure;}
    }
    globalThis.Document=Document;globalThis.document=new Document();
    globalThis.navigator={};globalThis.screen={};
    globalThis.fetch=()=>{order.push('promise');clock+=2;return promise;};
    globalThis.performance={now:()=>{order.push('clock');return clock;}};
    globalThis.Error=class {get stack(){clock+=100;return 'Error\\nframe\\nframe\\npage';}};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(
    vm.runInContext(
      `performance.now=()=>{throw failure;};order.length=0;
      const returned=document.querySelectorAll();
      globalThis.firstOrder=order.slice();returned===value`,
      context,
    ),
    true,
  );
  assert.deepEqual(Array.from(context.firstOrder), ["clock", "call", "clock", "clock"]);
  assert.equal(vm.runInContext(`document.querySelectorAll()===value`, context), true);
  assert.equal(vm.runInContext(`document.URL`, context), "document");
  assert.equal(
    vm.runInContext(`try{document.cookie='other';false}catch(error){error===failure}`, context),
    true,
  );
  assert.equal(vm.runInContext(`fetch()===promise`, context), true);
  const rows = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot().accesses)`,
      context,
    ),
  );
  const call = rows.find((row) => row.name === "Document.querySelectorAll");
  assert.equal(call.durationMs, 7);
  assert.equal(call.totalDurationMs, 14);
  assert.equal(call.maxDurationMs, 7);
  assert.equal(call.samples[0].durationMs, 7);
  assert.equal(rows.find((row) => row.name === "Document.URL" && row.kind === "get").durationMs, 3);
  const setter = rows.find((row) => row.name === "Document.cookie" && row.kind === "set");
  assert.equal(setter.durationMs, 5);
  assert.notEqual(setter.error, null);
  assert.equal(rows.find((row) => row.name === "window.fetch").durationMs, 2);
});
