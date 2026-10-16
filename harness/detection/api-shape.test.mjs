import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { differences, localFixtureUrl, memberInventory } from "./api-shape.mjs";

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
    Object.assign(globalThis, { Element, HTMLCanvasElement, Document });
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
  assert.equal(data.report.methods["canvas.getContext"].shape.constructible, false);
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
