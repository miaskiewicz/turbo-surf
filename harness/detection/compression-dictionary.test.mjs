import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(
  new URL("../../rust/crates/turbo-surf-render/src/compression_dictionary.js", import.meta.url),
  "utf8",
);

test("dictionary Chrome evidence is pinned and distinguishes CSP, CORS and background load", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/compression-dictionary-chrome155.json", import.meta.url)),
  );
  assert.equal(
    fixture.collectorSha256,
    createHash("sha256")
      .update(
        readFileSync(new URL("./authored-compression-dictionary-chrome.mjs", import.meta.url)),
      )
      .digest("hex"),
  );
  assert.equal(
    fixture.fixtureSha256,
    createHash("sha256")
      .update(readFileSync(new URL("./compression-dictionary-server.mjs", import.meta.url)))
      .digest("hex"),
  );
  assert.deepEqual(fixture.errors, []);
  const lifecycle = {
    image: ["DOMContentLoaded", "timer", "link:error", "future-timer", "load"],
    parser: ["timer", "link:error", "future-timer", "script", "DOMContentLoaded", "load"],
    defer: ["timer", "link:error", "future-timer", "script", "DOMContentLoaded", "load"],
  };
  for (let index = 0; index < fixture.cases.length; index++) {
    const kind = fixture.cases[index].lifecycleDelay;
    if (kind)
      assert.deepEqual(
        fixture.result[index].events.map((event) => event[0]),
        lifecycle[kind],
      );
  }
  for (let index = 0; index < fixture.cases.length; index++) {
    const item = fixture.cases[index],
      rows = fixture.wire.filter((row) => row.case === index && row.path === "/dictionary");
    const blocked =
      item.name === "connect-blocked" ||
      item.name === "default-blocked" ||
      item.foreign ||
      Boolean(item.lifecycleDelay);
    assert.equal(rows.length, blocked ? 0 : item.hrefChange || item.head ? 2 : 1, item.name);
    if (item.head) {
      const advertisement = fixture.wire
        .filter((row) => row.case === index && row.path === "/resource-" + index)
        .map((row) => Boolean(row.headers["available-dictionary"]));
      assert.deepEqual(advertisement, [false, false, true, true]);
    }
    for (const row of rows) {
      if (item.head) {
        assert.equal(row.headers.cookie, "dictionary_page_cookie=present");
        assert.ok(row.headers.referer.startsWith("https://localhost:"));
      } else {
        assert.equal(row.headers.cookie, undefined, item.name);
        assert.equal(row.headers.referer, undefined, item.name);
      }
      assert.equal(row.headers["sec-fetch-mode"], "cors");
      assert.equal(row.headers["sec-fetch-dest"], "empty");
      assert.equal(Boolean(row.headers.origin), Boolean(item.cross));
      for (const event of fixture.result[index].events.filter(
        (event) => event[0] === "load" || event[0] === "DOMContentLoaded",
      ))
        assert.ok(event[1] < row.finished, item.name);
    }
  }
});

test("background dictionary bootstrap coalesces in-flight requests and retires stale generations", async () => {
  let resourceTask;
  const reports = [];
  const pending = [],
    events = [],
    cancelled = [];
  const context = vm.createContext({
    __print: (message) => reports.push(message),
    ops: {
      op_set_host_capabilities: (value) => {
        resourceTask = value.runNextBackgroundResourceTask;
      },
      op_fetch_prepare: () => cancelled.length + pending.length + 1,
      op_fetch_cancel: (id) => cancelled.push(id),
      op_dictionary_prepare: (_doc, href, _base, rel, id) =>
        rel.includes("compression-dictionary") ? [href, JSON.stringify({ id })] : null,
      op_fetch: (url) => new Promise((resolve) => pending.push({ url, resolve })),
      op_dictionary_active: () => true,
      op_document_request_source: () => "https://site.test/",
      op_dictionary_headers: () => [],
    },
    onFetch: (url, resolve) => pending.push({ url, resolve }),
    __document: {},
    __fireResourceEvent: (element, kind) => events.push([element, kind]),
    setTimeout,
  });
  vm.runInContext(
    `class NodeList {get length(){return 0;} item(){return null;}}class Element {get namespaceURI(){return "http://www.w3.org/1999/xhtml";}get localName(){return "link";}}\n${source}\nglobalThis.controls={start(href,element){const generation={key:href};__dictionaryApply(__dictionarySet,__dictionaryLinks,[element,generation]);__dictionaryFetch(href,'https://site.test/',element,'compression-dictionary',generation);},pending:()=>__dictionaryPending,release(element){const generation=__dictionaryApply(__dictionaryGet,__dictionaryLinks,[element]);__dictionaryRelease(element,generation);__dictionaryApply(__dictionaryDelete,__dictionaryLinks,[element]);},header(href){__dictionaryFetch(href,'https://site.test/',null,'compression-dictionary');}};`,
    context,
  );
  vm.runInContext("ops.op_fetch=url=>new Promise(resolve=>onFetch(url,resolve));", context);
  vm.runInContext(
    "globalThis.dictionaryPromiseLeaks=0;Object.defineProperty(Promise.prototype,'constructor',{configurable:true,get(){dictionaryPromiseLeaks++;throw Error('guest constructor getter');}});",
    context,
  );
  vm.runInContext(
    "try{new Promise(()=>{}).then(()=>{})}catch{}globalThis.dictionaryPoisonDetected=dictionaryPromiseLeaks;dictionaryPromiseLeaks=0;",
    context,
  );
  assert.equal(
    context.dictionaryPoisonDetected,
    1,
    "negative control reaches the guest promise getter",
  );
  const element = {};
  context.controls.start("https://site.test/a", element);
  context.controls.start("https://site.test/b", element);
  context.controls.start("https://site.test/a", element);
  assert.equal(pending.length, 2);
  assert.equal(context.controls.pending(), 2);
  pending[0].resolve({ ok: true });
  pending[1].resolve({ ok: true });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  assert.deepEqual(events, [[element, "load"]]);
  assert.equal(context.controls.pending(), 0);
  context.controls.start("https://site.test/a", element);
  assert.equal(
    pending.length,
    3,
    "settled hints do not suppress later genuine fetch/cache admission",
  );
  context.ops.op_dictionary_active = () => false;
  pending[2].resolve({ ok: true });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  assert.equal(events.length, 1, "retired documents dispatch no late link events");
  context.ops.op_dictionary_active = () => true;
  const first = {},
    second = {};
  context.controls.start("https://site.test/shared", first);
  context.controls.start("https://site.test/shared", second);
  assert.equal(pending.length, 4, "two DOM consumers share one actual request");
  const before = cancelled.length;
  context.controls.release(first);
  assert.equal(cancelled.length, before, "removing one consumer preserves the other");
  context.controls.release(second);
  assert.equal(cancelled.length, before + 1, "last consumer removal cancels actual IO");
  pending[3].resolve({ aborted: true });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  context.controls.header("https://site.test/header-shared");
  context.controls.start("https://site.test/header-shared", first);
  const beforeHeaderRelease = cancelled.length;
  context.controls.release(first);
  assert.equal(
    cancelled.length,
    beforeHeaderRelease,
    "header admission owns its background request independently",
  );
  pending[4].resolve({ ok: true });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  assert.equal(context.controls.pending(), 0);
  assert.equal(
    context.dictionaryPromiseLeaks,
    0,
    "private async fetch promises never consult guest constructor/species hooks",
  );
  vm.runInContext("for(let index=0;index<4096;index++)__dictionaryQueueTask(()=>{});", context);
  context.controls.start("https://site.test/overflow", first);
  pending[5].resolve({ ok: true });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(
    vm.runInContext("__dictionaryActive.size", context),
    0,
    "overflow releases the completed request's strong DOM consumer record",
  );
  assert.deepEqual(reports, ["Background resource task limit exceeded\n"]);
  vm.runInContext("__dictionaryRetireTasks()", context);
  assert.equal(resourceTask(), 0);
});

test("native dictionary discovery cancels stale hrefs and preserves shared owners", async () => {
  let serial = 0,
    resourceTask;
  const requests = [],
    cancelled = [],
    events = [],
    document = {},
    first = { rel: "compression-dictionary", href: "https://site.test/a" },
    second = { rel: "compression-dictionary", href: "https://site.test/a" };
  let links = [first, second],
    headers = [];
  const context = vm.createContext({
    __print: () => {},
    __document: document,
    __resourceBaseURL: () => "https://site.test/",
    __resourceAttribute(name) {
      return this[name] ?? null;
    },
    __fireResourceEvent: (element, kind) => events.push([element, kind]),
    setTimeout,
    ops: {
      op_set_host_capabilities: (value) => {
        resourceTask = value.runNextBackgroundResourceTask;
      },
      op_document_request_source: () => "https://site.test/",
      op_dictionary_headers: () => {
        const result = headers;
        headers = [];
        return result;
      },
      op_dictionary_links: (_document, root) =>
        root === document ? links.filter((item) => item.rel === "compression-dictionary") : [root],
      op_dictionary_active: () => true,
      op_fetch_prepare: () => ++serial,
      op_fetch_cancel: (id) => cancelled.push(id),
      op_dictionary_prepare: (_document, href, _base, _rel, id) => [href, JSON.stringify({ id })],
      op_fetch: (url, init) =>
        new Promise((resolve) => requests.push({ url, id: JSON.parse(init).id, resolve })),
    },
  });
  vm.runInContext(
    `${source}\nglobalThis.controls={discover:__discoverDictionaries,remove:__dictionaryRemoved,pending:()=>__dictionaryPending};`,
    context,
  );
  const controls = context.controls;
  controls.discover();
  assert.equal(requests.length, 1, "two initial links share one actual request");
  const initial = requests[0];
  first.href = "https://site.test/b";
  controls.discover();
  assert.equal(requests.length, 2);
  assert.ok(!cancelled.includes(initial.id), "other link still owns the original URL");
  links = [first];
  controls.remove(second);
  assert.ok(cancelled.includes(initial.id), "removing final original consumer cancels IO");
  first.href = "https://site.test/a";
  controls.discover();
  assert.equal(requests.length, 3, "A to B to A starts a fresh request after cancellation");
  assert.ok(cancelled.includes(requests[1].id));
  initial.resolve({ ok: true, aborted: false });
  requests[1].resolve({ ok: true, aborted: false });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  assert.deepEqual(events, [], "superseded generations cannot dispatch stale events");
  requests[2].resolve({ ok: true, aborted: false });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  assert.deepEqual(events, [[first, "load"]]);
  first.href = "https://site.test/header";
  headers = [first.href];
  controls.discover();
  assert.equal(requests.length, 4);
  first.rel = "alternate";
  controls.discover();
  assert.ok(!cancelled.includes(requests[3].id), "header remains an independent owner");
  requests[3].resolve({ ok: true, aborted: false });
  await new Promise((resolve) => setImmediate(resolve));
  while (resourceTask()) {}
  assert.deepEqual(events, [[first, "load"]], "changed rel suppresses late DOM events");
  assert.equal(controls.pending(), 0);
});

test("dictionary networking tasks are bounded, private, separately dispatched and retired", () => {
  let run;
  const context = vm.createContext({
    __print: () => {},
    ops: {
      op_set_host_capabilities: (value) => {
        run = value.runNextBackgroundResourceTask;
      },
    },
  });
  vm.runInContext(
    `${source}\nglobalThis.controls={queue:__dictionaryQueueTask,retire:__dictionaryRetireTasks,count:()=>__dictionaryTaskCount};`,
    context,
  );
  const events = [];
  context.controls.queue(() => events.push("first"));
  context.controls.queue(() => {
    throw Error("task failure");
  });
  context.controls.queue(() => events.push("third"));
  assert.deepEqual(events, [], "queueing does not synchronously dispatch a resource event");
  assert.equal(run(), 1);
  assert.deepEqual(events, ["first"]);
  assert.throws(run, /task failure/);
  assert.equal(run(), 1, "failed task is removed before callback invocation");
  assert.deepEqual(events, ["first", "third"]);
  assert.equal(run(), 0);
  for (let index = 0; index < 4096; index++) context.controls.queue(() => {});
  assert.throws(() => context.controls.queue(() => {}), /task limit/);
  context.controls.retire();
  assert.equal(context.controls.count(), 0);
  assert.equal(run(), 0, "retirement releases queued callbacks");
});
