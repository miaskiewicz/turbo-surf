import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as pause } from "node:timers/promises";
import { runInNewContext } from "node:vm";
import { differences, localFixtureUrl, memberInventory, runApiCollector } from "./api-shape.mjs";

test("custom paired fixtures remain local and reject credential-bearing or remote URLs", () => {
  for (const url of [
    "http://127.0.0.1:1234/fixture",
    "http://127.12.3.4/",
    "https://localhost/",
    "http://[::1]/",
  ]) {
    assert.equal(localFixtureUrl(url), new URL(url).href);
  }
  for (const url of [
    "https://www.google.com/",
    "http://127.0.0.1.evil/",
    "http://192.168.1.1/",
    "http://user:secret@localhost/",
    "file:///tmp/fixture",
  ]) {
    assert.throws(() => localFixtureUrl(url), TypeError);
  }
});

test("API differential preserves descriptor paths and absent fields", () => {
  assert.deepEqual(
    differences(
      { method: { length: 1, descriptor: { enumerable: true } }, missing: null },
      { method: { length: 0, descriptor: { enumerable: false } } },
    ),
    [
      { path: "method.descriptor.enumerable", chrome: true, native: false },
      { path: "method.length", chrome: 1, native: 0 },
      { path: "missing", chrome: null, native: { absent: true } },
    ],
  );
  assert.deepEqual(differences({ a: 1, b: 2 }, { b: 2, a: 1 }), []);
  assert.deepEqual(differences({ method: { length: 1 } }, {}), [
    { path: "method", chrome: { length: 1 }, native: { absent: true } },
  ]);
});

test("collector tests construction without invoking the constructor or network APIs", () => {
  const source = readFileSync(new URL("api-shape.js", import.meta.url), "utf8");
  const result = runInNewContext(`
    let constructorCalls = 0, networkCalls = 0;
    class Element {
      get hostile() { throw new Error('inventory must not invoke accessors'); }
      getAttribute(name) { if (!(this instanceof Element)) throw new TypeError(); return null; }
    }
    class HTMLCanvasElement extends Element {
      getContext() { if (!(this instanceof HTMLCanvasElement)) throw new TypeError(); return null; }
      toDataURL() { if (!(this instanceof HTMLCanvasElement)) throw new TypeError(); return ''; }
    }
    class Document {
      createElement(name) { return name === 'canvas' ? new HTMLCanvasElement() : new Element(); }
      querySelector() { if (!(this instanceof Document)) throw new TypeError(); return null; }
    }
    class HTMLSelectElement extends Element {
      constructor() { super(); constructorCalls++; }
      get labels() { throw new Error('inventory invoked labels'); }
    }
    class HTMLOutputElement extends HTMLSelectElement { get labels() { throw new Error('inventory invoked labels'); } }
    class HTMLMeterElement extends HTMLSelectElement { get labels() { throw new Error('inventory invoked labels'); } }
    class HTMLProgressElement extends HTMLSelectElement { get labels() { throw new Error('inventory invoked labels'); } }
    class HTMLLabelElement extends HTMLSelectElement { get control() { throw new Error('inventory invoked label.control'); } }
    Object.defineProperty(HTMLSelectElement.prototype, 'labels', { enumerable: true });
    Object.assign(globalThis, { Element, HTMLCanvasElement, Document,
      HTMLSelectElement, HTMLOutputElement, HTMLMeterElement, HTMLProgressElement, HTMLLabelElement });
    globalThis.document = new Document();
    globalThis.navigator = { permissions: { query() {} }, sendBeacon() { networkCalls++; } };
    globalThis.performance = { now() { return 0; }, timeOrigin: 1 };
    globalThis.localStorage = { getItem() { return null; } };
    globalThis.fetch = () => { networkCalls++; };
    globalThis.URL = function URL() { constructorCalls++; throw new Error('must not execute'); };
    const report = ${source.trim().replace(/;$/, "")};
    JSON.stringify({ report: JSON.parse(report), constructorCalls, networkCalls });
  `);
  const data = JSON.parse(result);
  assert.equal(data.constructorCalls, 0);
  assert.equal(data.networkCalls, 0);
  assert.equal(data.report.constructors.URL.shape.constructible, true);
  for (const [name, member] of [
    ["HTMLSelectElement", "labels"],
    ["HTMLOutputElement", "labels"],
    ["HTMLMeterElement", "labels"],
    ["HTMLProgressElement", "labels"],
    ["HTMLLabelElement", "control"],
  ]) {
    const collected = data.report.constructors[name];
    assert.equal(collected.shape.name, name);
    assert.equal(collected.shape.constructible, true);
    assert.ok(collected.prototypeKeys.includes(member));
    assert.ok(collected.prototypeOwnKeyOrder.includes(member));
    assert.equal(collected.prototypeMembers[member].kind, "accessor");
    assert.equal(collected.prototypeMembers[member].get.name, `get ${member}`);
    assert.equal(collected.prototypeMembers[member].get.length, 0);
    assert.equal(collected.prototypeMembers[member].configurable, true);
  }
  assert.equal(data.report.constructors.HTMLSelectElement.prototypeMembers.labels.enumerable, true);
  assert.equal(
    data.report.constructors.HTMLOutputElement.prototypeMembers.labels.enumerable,
    false,
  );
  assert.equal(data.report.methods["canvas.getContext"].shape.constructible, false);
  assert.deepEqual(data.report.constructors.Element.prototypeOwnKeyOrder, [
    "constructor",
    "hostile",
    "getAttribute",
  ]);
  assert.equal(data.report.constructors.Element.prototypeMembers.hostile.kind, "accessor");
  assert.equal(data.report.constructors.Element.prototypeMembers.getAttribute.function.length, 1);
  assert.deepEqual(data.report.methods["document.querySelector"].invalidReceiver, {
    error: "TypeError",
  });
});

test("member inventory distinguishes missing interfaces, members and descriptor differences", () => {
  const chrome = {
    constructors: {
      Element: {
        prototypeMembers: {
          missing: { kind: "data" },
          same: { enumerable: true },
          changed: { enumerable: true },
        },
      },
      Document: { prototypeMembers: {} },
    },
  };
  const native = {
    constructors: {
      Element: {
        prototypeMembers: {
          extra: { kind: "data" },
          same: { enumerable: true },
          changed: { enumerable: false },
        },
      },
    },
  };
  const result = memberInventory(chrome, native);
  assert.deepEqual(result.Document, { measured: false, chrome: true, native: false });
  assert.deepEqual(result.Element.missing, ["missing"]);
  assert.deepEqual(result.Element.extra, ["extra"]);
  assert.deepEqual(result.Element.changed, [
    { path: "changed.enumerable", chrome: true, native: false },
  ]);
});

for (const reason of ["timeout", "abort"]) {
  test(
    `API collector ${reason} preserves full logs and retires its detached child`,
    {
      skip: process.platform === "win32",
    },
    async (t) => {
      const directory = mkdtempSync(join(tmpdir(), "api-collector-cleanup-"));
      const ready = join(directory, "ready.json");
      const evidence = join(directory, "trace.json");
      const controller = new AbortController();
      let descendant, observation;
      let passed = false;
      const source = String.raw`
      import {spawn} from 'node:child_process';
      import {writeFileSync} from 'node:fs';
      import {once} from 'node:events';
      import {collectorLifecycle} from ${JSON.stringify(new URL("collector-lifecycle.mjs", import.meta.url).href)};
      const lifecycle=collectorLifecycle({graceMs:20,onAbort(){
        writeFileSync(process.argv[2],JSON.stringify({captureComplete:false,interrupted:true}));
      }});
      const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});
      const retired=once(child,'close');lifecycle.own(child);
      await new Promise(done=>process.stdout.write('x'.repeat(2**20+17),done));
      process.stderr.write('private diagnostic\n');
      writeFileSync(process.argv[1],JSON.stringify({pid:child.pid}));
      try{await lifecycle.race(new Promise(()=>{}));}catch{}
      finally{await lifecycle.stopBrowser();await retired;lifecycle.dispose();}
    `;
      try {
        const pending = runApiCollector(
          process.execPath,
          ["--input-type=module", "-e", source, ready, evidence],
          directory,
          {
            signal: controller.signal,
            timeoutMs: reason === "timeout" ? 1000 : 5000,
            killGraceMs: 200,
          },
        );
        // Observe rejection immediately while waiting for the fixture's readiness.
        observation = pending.then(
          (value) => ({ value }),
          (error) => ({ error }),
        );
        const deadline = Date.now() + 3000;
        while (!existsSync(ready) && Date.now() < deadline) await pause(10);
        assert.equal(existsSync(ready), true, "the owned child reached its waiting state");
        descendant = JSON.parse(readFileSync(ready, "utf8")).pid;
        if (reason === "abort") controller.abort();
        const { error } = await observation;
        assert.ok(error instanceof Error);
        assert.equal(
          error.processResult.code,
          0,
          "cooperative cleanup may exit zero without becoming success",
        );
        assert.equal(error.processResult.timedOut, reason === "timeout");
        assert.equal(error.processResult.aborted, reason === "abort");
        assert.deepEqual(
          JSON.parse(readFileSync(join(directory, "process.json"), "utf8")),
          error.processResult,
        );
        assert.equal(readFileSync(join(directory, "stdout.log")).length, 2 ** 20 + 17);
        assert.equal(readFileSync(join(directory, "stderr.log"), "utf8"), "private diagnostic\n");
        assert.deepEqual(JSON.parse(readFileSync(evidence, "utf8")), {
          captureComplete: false,
          interrupted: true,
        });
        assert.throws(() => process.kill(descendant, 0), { code: "ESRCH" });
        passed = true;
      } finally {
        controller.abort();
        await observation;
        if (descendant) {
          try {
            process.kill(descendant, "SIGKILL");
          } catch {}
        }
        if (passed) rmSync(directory, { recursive: true, force: true });
        else t.diagnostic(`Failed API supervision fixture retained at ${directory}`);
      }
    },
  );
}
