import assert from "node:assert/strict";
import test from "node:test";
import { LiveActorHub } from "./actor-hub.mjs";

function fixture() {
  const root = { id: "100", alias: 7, closed: false };
  const pages = new Map([["100", root]]);
  const emitted = [];
  const adopted = [];
  const consumed = [];
  let snapshots = [];
  let batches = [];
  const native = {
    liveAuxiliaryState: async () =>
      JSON.stringify(
        snapshots.shift() ?? {
          actor: 7,
          capability: { kind: "supported" },
          root: "100",
          pages: [],
          events: [],
          closed: false,
        },
      ),
    liveAuxiliaryAdopt: (actor, target) => {
      adopted.push([actor, target]);
      return 20 + adopted.length;
    },
    liveNetworkLog: async () => JSON.stringify(batches.shift() ?? { entries: [], revision: 1 }),
    liveNetworkLogAcknowledge: () => {},
    liveActorClose: (actor) => {
      consumed.push(["actor-close", actor]);
    },
  };
  const callbacks = {
    create: (info, alias) => {
      const page = { id: info.target, alias, closed: info.closed };
      pages.set(info.target, page);
      return page;
    },
    attach: () => {},
    created: (page, event) => emitted.push(["created", page.id, event.sourcePage]),
    lifecycle: (page, event) => emitted.push([event.kind, page.id]),
    navigated: (page, event) => emitted.push(["navigated", page.id, event.url, event.isolated]),
    closed: (page) => {
      page.closed = true;
      emitted.push(["closed", page.id]);
    },
    network: (page, entry) => consumed.push([page.id, entry]),
    discard: () => {},
    failure: (error) => {
      throw error;
    },
  };
  const hub = new LiveActorHub(
    native,
    { actor: 7, capability: { kind: "supported" }, root: "100" },
    root,
    callbacks,
  );
  return { root, pages, emitted, adopted, consumed, hub, snapshots, batches, native, callbacks };
}
const child = (extra = {}) => ({
  target: "200",
  source: "100",
  sourcePage: "100",
  url: "about:blank",
  closed: false,
  ...extra,
});

test("adopts trusted child identities once and does not repeat the root Page event", async () => {
  const f = fixture();
  f.snapshots.push({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [
      { kind: "created", ...child() },
      { kind: "created", ...child() },
    ],
    closed: false,
  });
  await f.hub.drain();
  assert.deepEqual(f.adopted, [[7, "200"]]);
  assert.deepEqual(f.emitted, [["created", "200", "100"]]);
});

test("adopts lifecycle before routing HTTP and routes body completions by request ID", async () => {
  const f = fixture();
  f.snapshots.push({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [{ kind: "created", ...child() }],
    closed: false,
  });
  const response = {
    id: "request-1",
    status: 200,
    bodyPending: true,
    context: { frame: { pageId: "200", path: [] } },
  };
  const body = { event: "responsebody", id: "request-1", body: "child payload" };
  f.batches.push({ revision: 2, entries: [response, body] });
  await f.hub.drain();
  assert.deepEqual(f.consumed, [
    ["200", response],
    ["200", body],
  ]);
});

test("a genuine created-and-retired target is published closed without allocating another native Page", async () => {
  const f = fixture();
  f.snapshots.push({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child({ closed: true })],
    events: [
      { kind: "created", ...child() },
      { kind: "closed", target: "200" },
    ],
    closed: false,
  });
  await f.hub.drain();
  assert.equal(f.pages.get("200").closed, true);
  assert.equal(f.pages.get("200").alias, null);
  assert.deepEqual(f.adopted, []);
  assert.deepEqual(f.emitted, [
    ["created", "200", "100"],
    ["closed", "200"],
  ]);
});

test("COOP navigation preserves the Page identity instead of reporting Page closure", async () => {
  const f = fixture();
  f.snapshots.push({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [
      { kind: "created", ...child() },
      { kind: "navigated", target: "200", url: "https://peer.test/", isolated: true },
    ],
    closed: false,
  });
  await f.hub.drain();
  assert.equal(f.pages.get("200").closed, false);
  assert.deepEqual(f.emitted, [
    ["created", "200", "100"],
    ["navigated", "200", "https://peer.test/", true],
  ]);
});

test("serializes competing consumers of the one actor lifecycle and network logs", async () => {
  const f = fixture();
  let calls = 0;
  let release;
  f.native.liveAuxiliaryState = () => {
    calls++;
    return new Promise((resolve) => {
      release = () =>
        resolve(
          JSON.stringify({
            actor: 7,
            capability: { kind: "supported" },
            root: "100",
            pages: [],
            events: [],
            closed: false,
          }),
        );
    });
  };
  const first = f.hub.drain();
  const second = f.hub.drain();
  assert.equal(calls, 1);
  release();
  await Promise.all([first, second]);
  assert.equal(calls, 1);
});

test("hard close uses the immutable actor token after its root Page was closed", async () => {
  const f = fixture();
  f.snapshots.push({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [
      { kind: "created", ...child() },
      { kind: "closed", target: "100" },
    ],
    closed: false,
  });
  await f.hub.drain();
  assert.equal(f.root.closed, true);
  await f.hub.close();
  await f.hub.close();
  assert.deepEqual(f.consumed, [["actor-close", 7]]);
  assert.equal(f.pages.get("200").closed, true);
});

test("binding callbacks are drained by the actor with their actual owning Page", async () => {
  const f = fixture();
  const calls = [];
  function bindingPage(id) {
    let queued = true;
    return {
      id,
      _session: id,
      _closed: false,
      _mergedBindings: () => new Map([["host", { name: id }]]),
      _takeBindingCalls: async () => (queued ? ((queued = false), [{ name: "host" }]) : []),
      _settleBindingCall: async (call, entry) => calls.push([id, entry.name]),
      _collectSession: async () => {},
      _refreshFromSession: async () => {},
    };
  }
  f.hub.pages.set("100", bindingPage("100"));
  f.hub.pages.set("200", bindingPage("200"));
  await f.hub.drainBindings();
  assert.deepEqual(calls, [
    ["100", "100"],
    ["200", "200"],
  ]);
});

test("a host callback can await another Page binding without independent queue consumers", async () => {
  const f = fixture();
  let complete;
  const nested = new Promise((resolve) => {
    complete = resolve;
  });
  let rootQueued = true;
  let childQueued = false;
  const root = {
    _session: 7,
    _closed: false,
    _mergedBindings: () => new Map([["outer", {}]]),
    _takeBindingCalls: async () => (rootQueued ? ((rootQueued = false), [{ name: "outer" }]) : []),
    _settleBindingCall: async () => {
      childQueued = true;
      await f.hub.drainBindings();
      await nested;
    },
    _collectSession: async () => {},
    _refreshFromSession: async () => {},
  };
  const child = {
    _session: 21,
    _closed: false,
    _mergedBindings: () => new Map([["nested", {}]]),
    _takeBindingCalls: async () =>
      childQueued ? ((childQueued = false), [{ name: "nested" }]) : [],
    _settleBindingCall: async () => complete(),
    _collectSession: async () => {},
    _refreshFromSession: async () => {},
  };
  f.hub.pages.set("100", root);
  f.hub.pages.set("200", child);
  await f.hub.drainBindings();
  assert.equal(rootQueued, false);
  assert.equal(childQueued, false);
});

test("lifecycle waits retain the observed native revision across root installation", async () => {
  const f = fixture();
  await f.hub.applyState({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    revision: "8",
    pages: [],
    events: [],
    closed: false,
  });
  let observed;
  f.native.liveAuxiliaryWait = async (actor, revision) => {
    observed = [actor, revision];
    return JSON.stringify({
      closed: true,
      finalState: {
        actor: 7,
        capability: { kind: "supported" },
        root: "100",
        revision: "9",
        pages: [],
        events: [],
        closed: true,
      },
    });
  };
  await f.hub._watchLifecycle();
  assert.deepEqual(observed, [7, "8"]);
  assert.equal(f.root.closed, true);
});

test("a snapshot for another actor cannot assign the opening root or its revision", async () => {
  const f = fixture();
  const hub = new LiveActorHub(
    f.native,
    { actor: 7, capability: { kind: "pending" }, root: null },
    f.root,
    f.callbacks,
  );
  await assert.rejects(
    hub.applyState({ actor: 8, root: "900", revision: "99", pages: [], events: [], closed: false }),
    /actor identity/,
  );
  assert.equal(hub.root, null);
  assert.equal(hub.pages.size, 0);
  assert.equal(hub.revision, undefined);
});

test("actor readiness survives a root navigation failure while a genuine child remains live", async () => {
  const f = fixture();
  const hub = new LiveActorHub(
    f.native,
    {
      actor: 7,
      capability: { kind: "pending" },
      root: null,
      opening: true,
    },
    f.root,
    f.callbacks,
  );
  let ready = false;
  hub.ready.then(() => {
    ready = true;
  });
  await Promise.resolve();
  assert.equal(ready, false);
  await hub.applyState({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    opening: false,
    pages: [child()],
    events: [
      { kind: "created", ...child() },
      { kind: "closed", target: "100" },
    ],
    closed: false,
  });
  await hub.ready;
  assert.equal(ready, true);
  assert.equal(f.root.closed, true);
  assert.equal(f.pages.get("200").closed, false);
  assert.equal(hub.active, true);
});

test("closing from a creation listener preserves queued genuine creations without adopting retired targets", async () => {
  const f = fixture();
  const second = child({ target: "300" });
  let closing;
  f.callbacks.created = async (page, event) => {
    f.emitted.push(["created", page.id, event.sourcePage]);
    if (page.id === "200") {
      closing = f.hub.close();
      await closing;
    }
  };
  f.native.liveActorClose = () =>
    JSON.stringify({
      actor: 7,
      capability: { kind: "supported" },
      root: "100",
      pages: [child({ closed: true }), { ...second, closed: true }],
      events: [
        { kind: "closed", target: "100" },
        { kind: "closed", target: "200" },
        { kind: "closed", target: "300" },
      ],
      closed: true,
    });
  await f.hub.initialize({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child(), second],
    events: [
      { kind: "created", ...child() },
      { kind: "created", ...second },
    ],
    closed: false,
  });
  await closing;
  assert.deepEqual(f.adopted, [[7, "200"]]);
  assert.deepEqual(f.emitted, [
    ["created", "200", "100"],
    ["created", "300", "100"],
    ["closed", "100"],
    ["closed", "200"],
    ["closed", "300"],
  ]);
  assert.equal(f.pages.get("300").alias, null);
});

test("native lifecycle overflow fails honestly without allocating a replacement Page", async () => {
  const f = fixture();
  await assert.rejects(
    f.hub.applyState({
      actor: 7,
      capability: { kind: "supported" },
      root: "100",
      pages: [],
      events: [{ kind: "overflow", message: "native auxiliary event budget exceeded" }],
      closed: false,
    }),
    /native auxiliary event budget exceeded/,
  );
  assert.deepEqual(f.adopted, []);
});

test("retired Page facades are not strongly owned by a surviving actor", async () => {
  const f = fixture();
  await f.hub.applyState({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [
      { kind: "created", ...child() },
      { kind: "closed", target: "100" },
    ],
    closed: false,
  });
  assert.equal(f.hub.pages.has("100"), false);
  assert.equal(f.hub.page("100"), f.root);
  assert.equal(f.hub.pages.get("200"), f.pages.get("200"));
});

test("opening actors acquire only their later genuine root identity", async () => {
  const f = fixture();
  const hub = new LiveActorHub(
    f.native,
    { actor: 7, capability: { kind: "pending" }, root: null },
    f.root,
    f.callbacks,
  );
  assert.equal(hub.pages.size, 0);
  await hub.applyState({
    actor: 7,
    capability: { kind: "pending" },
    root: null,
    pages: [],
    events: [],
    closed: false,
  });
  await hub.applyState({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [{ kind: "created", ...child() }],
    closed: false,
  });
  assert.equal(hub.pages.get("100"), f.root);
  assert.deepEqual(f.emitted, [["created", "200", "100"]]);
});

test("navigation commit never fabricates load before real native lifecycle events", async () => {
  const f = fixture();
  await f.hub.applyState({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [
      { kind: "created", ...child() },
      { kind: "navigated", target: "200", url: "https://receipt.test/", isolated: false },
    ],
    closed: false,
  });
  assert.deepEqual(f.emitted, [
    ["created", "200", "100"],
    ["navigated", "200", "https://receipt.test/", false],
  ]);
  await f.hub.applyState({
    actor: 7,
    capability: { kind: "supported" },
    root: "100",
    pages: [child()],
    events: [
      { kind: "domContentLoaded", target: "200" },
      { kind: "load", target: "200" },
    ],
    closed: false,
  });
  assert.deepEqual(f.emitted.slice(-2), [
    ["domContentLoaded", "200"],
    ["load", "200"],
  ]);
});

test("hard-close receipts publish genuine previously unadopted closed Pages exactly once", async () => {
  const f = fixture();
  f.native.liveActorClose = () =>
    JSON.stringify({
      actor: 7,
      capability: { kind: "supported" },
      root: "100",
      pages: [child({ closed: true })],
      events: [
        { kind: "created", ...child() },
        { kind: "closed", target: "100" },
        { kind: "closed", target: "200" },
      ],
      closed: true,
    });
  await f.hub.close();
  assert.deepEqual(f.adopted, []);
  assert.deepEqual(f.emitted, [
    ["created", "200", "100"],
    ["closed", "100"],
    ["closed", "200"],
  ]);
  assert.equal(f.pages.get("200").closed, true);
});

test("pending actual-root capability holds the consuming HTTP queue until native proof", async () => {
  const f = fixture();
  const hub = new LiveActorHub(
    f.native,
    {
      actor: 7,
      root: null,
      capability: { kind: "pending" },
      opening: true,
    },
    f.root,
    f.callbacks,
  );
  f.native.liveAuxiliaryState = async () =>
    JSON.stringify({
      actor: 7,
      root: null,
      capability: { kind: "pending" },
      pages: [],
      events: [],
      closed: false,
    });
  let reads = 0;
  f.native.liveNetworkLog = () => {
    reads++;
    return JSON.stringify({ entries: [], revision: 0 });
  };
  await hub.drain();
  assert.equal(reads, 0);
  assert.equal(hub.pages.size, 0);
});

test("an unsupported actual root retains an ordinary Page without fake auxiliary identities", async () => {
  const f = fixture();
  let unsupported = 0;
  f.callbacks.unsupported = () => {
    unsupported++;
  };
  const hub = new LiveActorHub(
    f.native,
    {
      actor: 7,
      root: null,
      capability: { kind: "pending" },
      opening: true,
    },
    f.root,
    f.callbacks,
  );
  await hub.applyState({
    actor: 7,
    root: null,
    capability: { kind: "unsupported" },
    opening: false,
    pages: [],
    events: [],
    closed: false,
  });
  await hub.ready;
  assert.equal(hub.capability, "unsupported");
  assert.equal(hub.pages.size, 0);
  assert.equal(f.root.closed, false);
  assert.equal(unsupported, 1);
  assert.deepEqual(f.adopted, []);
});

test("hard-close ACK disables native reads before pending navigation callbacks replay", async () => {
  const f = fixture();
  f.callbacks.navigated = (page, event) => {
    assert.equal(f.hub.canReadTarget(page.id), false);
    f.emitted.push(["navigated", page.id, event.url]);
  };
  f.native.liveActorClose = () =>
    JSON.stringify({
      actor: 7,
      capability: { kind: "supported" },
      root: "100",
      pages: [],
      events: [{ kind: "closed", target: "100" }],
      closed: true,
    });
  f.hub.pending = Promise.resolve().then(() =>
    f.hub.applyState({
      actor: 7,
      capability: { kind: "supported" },
      root: "100",
      pages: [],
      events: [{ kind: "navigated", target: "100", url: "https://fixture.test/queued" }],
      closed: false,
    }),
  );
  await f.hub.close();
  assert.deepEqual(f.emitted, [
    ["navigated", "100", "https://fixture.test/queued"],
    ["closed", "100"],
  ]);
});

test("retired target metadata prevents native reads without closing live siblings", () => {
  const f = fixture();
  f.hub._rememberPages({ pages: [child({ closed: true })] });
  assert.equal(f.hub.canReadTarget("200"), false);
  assert.equal(f.hub.canReadTarget("100"), true);
});
