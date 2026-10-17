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

test("returned native WebGL own methods and extension methods preserve identities and receivers", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.clock=0;globalThis.failure=new Error('native failure');
    globalThis.pixelBuffer=new Uint8Array(4);globalThis.extension={
      drawBuffersWEBGL(values){if(this!==extension)throw failure;return values;}
    };
    const inherited={getSupportedExtensions(){if(this!==gl)throw failure;return ['TEST'];}};
    globalThis.gl=Object.assign(Object.create(inherited),{
      getParameter(parameter){if(this!==gl)throw failure;clock+=4;return pixelBuffer;},
      readPixels(...args){if(this!==gl)throw failure;args.at(-1).set([1,2,3,4]);},
      fail(){throw failure;},getExtension(){if(this!==gl)throw failure;return extension;}
    });
    globalThis.canvas={getContext(kind){if(this!==canvas)throw failure;return gl;}};
    globalThis.document={createElement(){return canvas;}};
    globalThis.performance={now:()=>clock};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(
    vm.runInContext(
      `const created=document.createElement('canvas');
      const returned=created.getContext('webgl');
      const first=returned.getParameter;
      const identity=returned===gl && returned.getParameter(123)===pixelBuffer;
      const repeated=created.getContext('experimental-webgl')===returned && returned.getParameter===first;
      returned.readPixels(0,0,1,1,0,0,pixelBuffer);
      const extensions=returned.getSupportedExtensions();
      const ext=returned.getExtension('TEST');
      globalThis.extensionResult=ext===extension && ext.drawBuffersWEBGL(pixelBuffer)===pixelBuffer;
      globalThis.errorResult=false;try{returned.fail();}catch(error){errorResult=error===failure;}
      globalThis.wrongReceiver=false;try{first.call({});}catch(error){wrongReceiver=error===failure;}
      identity && repeated && extensions[0]==='TEST'`,
      context,
      { filename: "https://example.test/gpu.js" },
    ),
    true,
  );
  assert.equal(context.extensionResult, true);
  assert.equal(context.errorResult, true);
  assert.equal(context.wrongReceiver, true);
  assert.deepEqual(Array.from(context.pixelBuffer), [1, 2, 3, 4]);
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  const parameter = snapshot.accesses.find(
    (row) => row.name === "WebGLRenderingContext.getParameter",
  );
  assert.equal(parameter.count, 2);
  assert.equal(parameter.durationMs, 4);
  assert.match(parameter.stack, /example\.test\/gpu\.js/);
  for (const name of [
    "HTMLCanvasElement.getContext",
    "WebGLRenderingContext.getParameter",
    "WebGLRenderingContext.getSupportedExtensions",
    "WebGLRenderingContext.readPixels",
    "WebGLRenderingContext.fail",
    "WebGLExtension.drawBuffersWEBGL",
  ]) {
    assert.equal(snapshot.installed.filter((installed) => installed === name).length, 1, name);
    assert.ok(
      snapshot.accesses.some((row) => row.name === name),
      name,
    );
  }
  assert.ok(
    snapshot.coverage.some(
      (row) =>
        row.name === "WebGLRenderingContext.getParameter" &&
        row.scope === "own" &&
        row.status === "installed",
    ),
  );
  assert.equal(
    snapshot.accesses.some((row) => row.name.startsWith("Object.")),
    false,
  );
});

test("prototype WebGL methods are wrapped once with descriptors preserved", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `class WebGLRenderingContext {getParameter(){return this;}}
    class HTMLCanvasElement {getContext(){return gl;}}
    globalThis.WebGLRenderingContext=WebGLRenderingContext;
    globalThis.HTMLCanvasElement=HTMLCanvasElement;
    globalThis.gl=new WebGLRenderingContext();globalThis.canvas=new HTMLCanvasElement();
    globalThis.document={createElement(){return canvas;}};globalThis.performance={now:()=>0};
    globalThis.before=Object.getOwnPropertyDescriptor(WebGLRenderingContext.prototype,'getParameter');`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(
    vm.runInContext(
      `const after=Object.getOwnPropertyDescriptor(WebGLRenderingContext.prototype,'getParameter');
      const wrapped=after.value;
      document.createElement('canvas').getContext('webgl').getParameter()===gl &&
      canvas.getContext('webgl').getParameter===wrapped && !Object.hasOwn(gl,'getParameter') &&
      after.enumerable===before.enumerable && after.configurable===before.configurable &&
      after.writable===before.writable && wrapped.name===before.value.name && wrapped.length===before.value.length`,
      context,
    ),
    true,
  );
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  assert.equal(
    snapshot.installed.filter((name) => name === "WebGLRenderingContext.getParameter").length,
    1,
  );
  assert.equal(
    snapshot.accesses.find((row) => row.name === "WebGLRenderingContext.getParameter").count,
    1,
  );
});

test("GPU coverage reports immutable, unavailable and unsupported observations without invoking getters", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.getterCalls=0;globalThis.gl={CONSTANT:123};
    Object.defineProperty(gl,'getParameter',{value:function(){return this;}});
    Object.defineProperty(gl,'throwing',{configurable:true,get(){getterCalls++;throw new Error('guest');}});
    class HTMLCanvasElement {getContext(kind){return kind==='webgl' ? gl : null;}}
    globalThis.HTMLCanvasElement=HTMLCanvasElement;globalThis.canvas=new HTMLCanvasElement();
    globalThis.performance={now:()=>0};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(
    vm.runInContext(`canvas.getContext('webgl')===gl && gl.getParameter()===gl`, context),
    true,
  );
  assert.equal(context.getterCalls, 0);
  assert.equal(vm.runInContext(`canvas.getContext('webgl2')`, context), null);
  assert.equal(vm.runInContext(`canvas.getContext('bitmaprenderer')`, context), null);
  assert.equal(
    vm.runInContext(`try{gl.throwing;false}catch(error){error.message==='guest'}`, context),
    true,
  );
  assert.equal(context.getterCalls, 1);
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  for (const [name, status, reason] of [
    ["OffscreenCanvas", "unavailable", "missing-interface"],
    ["WebGLRenderingContext.getParameter", "skipped", "nonconfigurable"],
    ["WebGLRenderingContext.CONSTANT", "unobserved", "data-property-or-setter"],
    ["WebGL2RenderingContext", "unavailable", "null-or-nonobject-context"],
    ["HTMLCanvasElement.getContext", "unobserved", "unrecognized-context-kind"],
  ]) {
    assert.ok(
      snapshot.coverage.some(
        (row) => row.name === name && row.status === status && row.reason === reason,
      ),
      name,
    );
  }
  assert.equal(
    snapshot.accesses.some((row) => row.name === "WebGLRenderingContext.getParameter"),
    false,
  );
  assert.equal(
    snapshot.accesses.find((row) => row.name === "WebGLRenderingContext.throwing").error,
    "Error: guest",
  );
});

test("context return inspection failures and kind objects do not change getContext behavior", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.coercions=0;globalThis.gl=new Proxy({}, {ownKeys(){throw new Error('inspect');}});
    globalThis.kind={toString(){coercions++;return 'webgl';}};
    class OffscreenCanvas {getContext(kind){String(kind);return gl;}}
    globalThis.OffscreenCanvas=OffscreenCanvas;globalThis.canvas=new OffscreenCanvas();
    globalThis.performance={now:()=>0};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(vm.runInContext(`canvas.getContext(kind)===gl`, context), true);
  assert.equal(context.coercions, 1);
  assert.equal(vm.runInContext(`canvas.getContext('webgl')===gl`, context), true);
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  assert.ok(
    snapshot.coverage.some(
      (row) => row.name === "OffscreenCanvas.getContext" && row.reason === "inspection-threw",
    ),
  );
  assert.ok(snapshot.coverage.some((row) => row.reason === "unrecognized-context-kind"));
  assert.equal(snapshot.accesses.find((row) => row.name === "OffscreenCanvas.getContext").count, 2);
});

test("GPU descriptor traversal leaves foreign Object prototypes unchanged and bounds cycles", () => {
  const foreign = vm.createContext({});
  const foreignGL = vm.runInContext(`({getParameter(){return this;}})`, foreign);
  const context = vm.createContext({
    foreignGL,
    foreignRoot: vm.runInContext("Object.prototype", foreign),
  });
  vm.runInContext(
    `globalThis.gl=foreignGL;
    class HTMLCanvasElement {getContext(){return gl;}}
    globalThis.HTMLCanvasElement=HTMLCanvasElement;globalThis.canvas=new HTMLCanvasElement();
    globalThis.performance={now:()=>0};`,
    context,
  );
  const rootMethod = vm.runInContext(`Object.prototype.toString`, foreign);
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(vm.runInContext(`canvas.getContext('webgl').getParameter()===gl`, context), true);
  assert.equal(vm.runInContext(`Object.prototype.toString`, foreign), rootMethod);
  assert.equal(
    vm.runInContext(`gl=foreignRoot;canvas.getContext('webgl')===foreignRoot`, context),
    true,
  );
  assert.equal(vm.runInContext(`Object.prototype.toString`, foreign), rootMethod);
  assert.equal(
    vm.runInContext(
      `gl=Object.assign(Object.create(null),{getParameter(){return 37;}});canvas.getContext('webgl').getParameter()`,
      context,
    ),
    37,
  );
  assert.equal(
    vm.runInContext(
      `gl=new Proxy({}, {getPrototypeOf(){return gl;}});canvas.getContext('webgl')===gl`,
      context,
    ),
    true,
  );
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  assert.ok(snapshot.coverage.some((row) => row.reason === "prototype-depth-limit"));
  assert.equal(snapshot.installed.includes("WebGLRenderingContext.toString"), false);
});

test("phantom context descriptors cannot hang a successful getContext call", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.gl=new Proxy({}, {
      ownKeys(){return ['missing'];},
      getOwnPropertyDescriptor(){return undefined;},
      getPrototypeOf(){return gl;}
    });
    class HTMLCanvasElement {getContext(){return gl;}}
    globalThis.HTMLCanvasElement=HTMLCanvasElement;globalThis.canvas=new HTMLCanvasElement();
    globalThis.performance={now:()=>0};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(
    vm.runInContext(`canvas.getContext('webgl')===gl`, context, { timeout: 1000 }),
    true,
  );
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  assert.ok(snapshot.coverage.some((row) => row.reason === "descriptor-depth-limit"));
  assert.ok(snapshot.coverage.some((row) => row.reason === "prototype-depth-limit"));
});

test("GPU coverage has an explicit bounded overflow without losing method observations", () => {
  const context = vm.createContext({});
  vm.runInContext(
    `globalThis.gl={getParameter(){return 42;}};
    for(let index=0;index<3200;index++)gl['CONSTANT_'+index]=index;
    class HTMLCanvasElement {getContext(){return gl;}}
    globalThis.HTMLCanvasElement=HTMLCanvasElement;globalThis.canvas=new HTMLCanvasElement();
    globalThis.performance={now:()=>0};`,
    context,
  );
  vm.runInContext(readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8"), context);
  assert.equal(vm.runInContext(`canvas.getContext('webgl').getParameter()`, context), 42);
  const snapshot = JSON.parse(
    vm.runInContext(
      `JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot())`,
      context,
    ),
  );
  assert.equal(snapshot.coverage.length, 3000);
  assert.ok(snapshot.droppedCoverage > 0);
  assert.equal(
    snapshot.accesses.find((row) => row.name === "WebGLRenderingContext.getParameter").count,
    1,
  );
  assert.deepEqual(snapshot.coverageScope, {
    gpu: "observed-getContext-results-and-returned-extensions",
    ownDataReads: false,
    symbolMembers: false,
    previouslyCapturedFunctions: false,
    lateReplacements: false,
    changesFunctionIdentity: true,
  });
});
