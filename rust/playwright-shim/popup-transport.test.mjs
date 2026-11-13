import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

let native;
try {
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  let markHeld, heldResponse;
  const held = new Promise((resolve) => {
    markHeld = resolve;
  });
  const server = createServer((request, response) => {
    response.setHeader("content-type", "text/html");
    if (request.url === "/hold") {
      response.setHeader("content-type", "application/javascript");
      heldResponse = response;
      markHeld();
    } else response.end("actual-popup-wire");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}/`;
  const actors = [];
  const open = async (html = "<body>root-marker</body>") => {
    const id = await native.liveOpen(html, base, "[]", "");
    actors.push(id);
    return id;
  };
  try {
    const reserve = () => {
      const id = native.liveNetworkLogCreate();
      actors.push(id);
      return id;
    };
    await run({ open, base, held, reserve, releaseHeld: () => heldResponse?.end("") });
  } finally {
    for (const id of actors) {
      if (typeof native.liveActorClose === "function") native.liveActorClose(id);
      else native.liveClose(id);
    }
    heldResponse?.end();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function popup(root) {
  assert.equal(
    await native.liveEvalEngineExpression(
      root,
      "(() => {globalThis.popup=window.open('about:blank','native-transport');popup.document.body.textContent='popup-marker';return !!popup;})()",
    ),
    "true",
  );
  const state = JSON.parse(native.liveAuxiliaryState(root));
  const created = state.events.filter((event) => event.kind === "created");
  assert.equal(created.length, 1);
  assert.equal(typeof created[0].target, "string");
  assert.equal(created[0].sourcePage, state.root);
  return {
    state,
    target: created[0].target,
    alias: native.liveAuxiliaryAdopt(root, created[0].target),
  };
}

const read = (id, source) => JSON.parse(native.liveReadEngine(id, source));

test("actual popup aliases are trusted, idempotent and share one retained isolate", () =>
  fixture(async ({ open }) => {
    const root = await open();
    const other = await open();
    const { target, alias } = await popup(root);
    assert.equal(native.liveAuxiliaryAdopt(root, target), alias);
    assert.throws(() => native.liveAuxiliaryAdopt(other, target), /native auxiliary target/);
    assert.throws(() => native.liveAuxiliaryAdopt(root, "01"), /Invalid native auxiliary target/);
    assert.throws(
      () => native.liveAuxiliaryAdopt(root, "18446744073709551615"),
      /native auxiliary target/,
    );
    assert.equal(read(root, "return document.body.textContent"), "root-marker");
    assert.equal(read(alias, "return document.body.textContent"), "popup-marker");
    assert.match(native.liveSerialize(alias), /popup-marker/);
    assert.equal(read(alias, "return opener.document.body.textContent"), "root-marker");
    assert.deepEqual(JSON.parse(native.liveAuxiliaryState(alias)).events, []);
  }));

test("closing opener retains auxiliary target and actor until final target closes", () =>
  fixture(async ({ open }) => {
    const root = await open();
    const { target, alias } = await popup(root);
    native.liveClose(root);
    assert.equal(read(alias, "return document.body.textContent"), "popup-marker");
    assert.equal(
      await native.liveEvalEngineExpression(
        alias,
        "fetch('/popup').then(response=>response.text())",
      ),
      "actual-popup-wire",
    );
    assert.throws(
      () => native.liveAuxiliaryAdopt(root, JSON.parse(native.liveAuxiliaryState(root)).root),
      /native auxiliary target/i,
    );
    assert.equal(native.liveAuxiliaryAdopt(root, target), alias);
    native.liveClose(alias);
    assert.throws(() => native.liveAuxiliaryState(root), /no such live actor/);
  }));

test("hard actor close retires unadopted targets and wakes lifecycle waiters", () =>
  fixture(async ({ open }) => {
    const root = await open();
    await native.liveEvalEngineExpression(root, "!!window.open('about:blank','unadopted')");
    const state = JSON.parse(native.liveAuxiliaryState(root));
    assert.equal(state.pages.length, 2);
    const waiting = native.liveAuxiliaryWait(root);
    const acknowledgement = JSON.parse(native.liveActorClose(root));
    const final = JSON.parse(await waiting);
    assert.equal(typeof final.closed, "boolean");
    assert.equal(acknowledgement.closed, true);
    assert.equal(
      acknowledgement.pages.every((page) => page.closed),
      true,
    );
    const events = acknowledgement.events.concat(final.finalState?.events ?? []);
    assert.equal(events.filter((event) => event.kind === "closed").length, 2);
    assert.throws(
      () => native.liveAuxiliaryAdopt(root, state.events[0].target),
      /no such live actor/,
    );
  }));

test(
  "target close interrupts its pending command without cancelling another Page",
  { timeout: 10000 },
  () =>
    fixture(async ({ open, held }) => {
      const root = await open();
      const { alias } = await popup(root);
      const work = native.liveEvalEngineExpression(
        alias,
        "fetch('/hold').then(response=>response.text())",
      );
      const rejected = assert.rejects(work, /closed|retired|cancelled/i);
      await held;
      native.liveClose(alias);
      await rejected;
      assert.equal(read(root, "return document.body.textContent"), "root-marker");
      assert.equal(
        await native.liveEvalEngineExpression(
          root,
          "fetch('/root').then(response=>response.text())",
        ),
        "actual-popup-wire",
      );
    }),
);

test("navigation retains the actual target identity and aliases its new document", () =>
  fixture(async ({ open, base }) => {
    const root = await open();
    const { target, alias } = await popup(root);
    const result = JSON.parse(await native.liveNavigateTarget(alias, base + "navigation", "push"));
    assert.equal(result.finalUrl, base + "navigation");
    assert.equal(native.liveAuxiliaryAdopt(root, target), alias);
    assert.equal(read(alias, "return document.body.textContent"), "actual-popup-wire");
    assert.equal(read(root, "return popup.document.body.textContent"), "actual-popup-wire");
    const state = JSON.parse(native.liveAuxiliaryState(root));
    assert.equal(
      state.events.filter((event) => event.kind === "navigated" && event.target === target).length,
      1,
    );
  }));

test("initial hydration retains canonical popup events before live registration", () =>
  fixture(async ({ open }) => {
    const root = await open(
      "<body>root-marker<script>globalThis.earlyPopup=window.open('about:blank','early');earlyPopup.document.body.textContent='early-marker';</script>",
    );
    const state = JSON.parse(native.liveAuxiliaryState(root));
    const created = state.events.filter((event) => event.kind === "created");
    assert.equal(created.length, 1);
    const alias = native.liveAuxiliaryAdopt(state.actor, created[0].target);
    assert.equal(read(alias, "return document.body.textContent"), "early-marker");
  }));

test(
  "opening actor metadata is published synchronously and never runs V8 queries early",
  { timeout: 10000 },
  () =>
    fixture(async ({ base, held, reserve, releaseHeld }) => {
      const id = reserve();
      const opening = native.liveOpen(
        "<body>root-marker<script src='/hold'></script>",
        base,
        "[]",
        "",
        undefined,
        JSON.stringify({ hostNetworkLog: id }),
      );
      // Keep ownership if an earlier capability assertion fails; later awaits
      // still observe the original opening promise and its actual result.
      opening.catch(() => {});
      const immediate = JSON.parse(native.liveAuxiliaryState(id));
      assert.equal(immediate.actor, id);
      assert.equal(immediate.root === null || typeof immediate.root === "string", true);
      await held;
      const state = JSON.parse(native.liveAuxiliaryState(id));
      assert.equal(state.opening, true);
      assert.equal(typeof state.root, "string");
      assert.notEqual(state.revision, "0");
      assert.deepEqual(JSON.parse(await native.liveAuxiliaryWait(id, "0")), { closed: false });
      const alias = native.liveAuxiliaryAdopt(id, state.root);
      assert.equal(alias, id);
      assert.throws(() => native.liveReadEngine(alias, "return 1"), /opening/);
      releaseHeld();
      assert.equal(await opening, id);
      assert.equal(read(alias, "return document.body.textContent"), "root-marker");
    }),
);

test(
  "hard close during initialization rolls back actual ownership before acknowledging",
  { timeout: 10000 },
  () =>
    fixture(async ({ base, held, reserve }) => {
      const id = reserve();
      const opening = native.liveOpen(
        "<body>root-marker<script src='/hold'></script>",
        base,
        "[]",
        "",
        undefined,
        JSON.stringify({ hostNetworkLog: id }),
      );
      // Keep ownership if an earlier capability assertion fails; later awaits
      // still observe the original opening promise and its actual result.
      opening.catch(() => {});
      const rejected = assert.rejects(opening, /closed|cancel/i);
      await held;
      const state = JSON.parse(native.liveAuxiliaryState(id));
      assert.equal(state.opening, true);
      assert.equal(state.pages.length, 1);
      const acknowledgement = JSON.parse(native.liveActorClose(id));
      await rejected;
      assert.equal(acknowledgement.closed, true);
      assert.equal(
        acknowledgement.pages.every((page) => page.closed),
        true,
      );
      assert.throws(() => native.liveAuxiliaryState(id), /no such live actor/);
    }),
);

test(
  "context and Page bootstrap updates stay scoped across later target navigation",
  { timeout: 10000 },
  () =>
    fixture(async ({ base, reserve }) => {
      const id = reserve();
      const contextInit = "globalThis.contextMarker='context-old';";
      const pageInit = "globalThis.pageMarker='root-page';";
      await native.liveOpen(
        "<body>root-marker</body>",
        base,
        "[]",
        "",
        undefined,
        JSON.stringify({
          hostNetworkLog: id,
          auxiliaryInitScript: contextInit,
          pageInitScript: pageInit,
        }),
        contextInit + pageInit,
      );
      await native.liveAuxiliaryConfigure(id, "globalThis.contextMarker='context-new';", "");
      await native.liveTargetConfigure(id, "globalThis.pageMarker='retained-root';", "");
      assert.deepEqual(read(id, "return [contextMarker,pageMarker]"), ["context-old", "root-page"]);
      const { alias, target } = await popup(id);
      assert.deepEqual(read(alias, "return [contextMarker,typeof pageMarker]"), [
        "context-new",
        "undefined",
      ]);
      await native.liveTargetConfigure(alias, "globalThis.pageMarker='child-only';", "");
      await native.liveNavigateTarget(alias, base + "child-navigation", "push");
      assert.deepEqual(read(alias, "return [contextMarker,pageMarker]"), [
        "context-new",
        "child-only",
      ]);
      await native.liveNavigateTarget(id, base + "root-navigation", "push");
      assert.deepEqual(read(id, "return [contextMarker,pageMarker]"), [
        "context-new",
        "retained-root",
      ]);
      await native.liveEvalEngineExpression(alias, "!!window.open('about:blank','grandchild')");
      const state = JSON.parse(native.liveAuxiliaryState(id));
      const grandchild = state.events.find(
        (event) => event.kind === "created" && event.target !== target,
      );
      assert.ok(grandchild);
      const grandchildAlias = native.liveAuxiliaryAdopt(id, grandchild.target);
      assert.deepEqual(read(grandchildAlias, "return [contextMarker,typeof pageMarker]"), [
        "context-new",
        "undefined",
      ]);
    }),
);

test("cancelling a reserved opening log removes it before any runtime starts", async () => {
  const id = native.liveNetworkLogCreate();
  try {
    assert.equal(native.liveActorClose(id), null);
    await assert.rejects(native.liveNetworkLog(id), /no such live session/);
  } finally {
    native.liveClose(id);
  }
});

test(
  "native frame properties stay owned by the replacement document realm",
  { timeout: 10000 },
  () =>
    fixture(async ({ open, base }) => {
      const root = await open();
      await native.liveNavigateTarget(root, base + "replacement", "push");
      assert.deepEqual(
        read(
          root,
          `const frame=document.createElement('iframe');
      document.body.appendChild(frame);
      return [window.frames===window, window.length===1, window[0]===frame.contentWindow];`,
        ),
        [true, true, true],
      );
      const { alias } = await popup(root);
      assert.deepEqual(
        read(
          alias,
          `const frame=document.createElement('iframe');
      document.body.appendChild(frame);
      return [window.frames===window, window.length===1, window[0]===frame.contentWindow,
        opener.length===1];`,
        ),
        [true, true, true, true],
      );
      assert.equal(read(root, "return window.length"), 1);
    }),
);
