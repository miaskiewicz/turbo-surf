import assert from "node:assert/strict";
import test from "node:test";
import { LiveActorHub } from "./actor-hub.mjs";

function protocol(identity = {}) {
  const root = { name: "root" };
  const events = [];
  const states = [];
  const batches = [];
  const snapshot = (extra = {}) => ({
    actor: 7,
    root: "root",
    capability: { kind: "supported" },
    pages: [],
    events: [],
    closed: false,
    ...extra,
  });
  const native = {
    liveAuxiliaryState: async () => states.shift() ?? snapshot(),
    liveAuxiliaryAdopt: (_actor, target) => `alias:${target}`,
    liveNetworkLog: async () => batches.shift() ?? "",
    liveNetworkLogAcknowledge: (actor, revision) => events.push(["ack", actor, revision]),
    liveNetworkLogWake: (actor) => events.push(["wake", actor]),
    liveActorClose: () => snapshot({ closed: true }),
  };
  const callbacks = {
    create: (info, alias) => ({ name: info.target, alias }),
    created: (page) => events.push(["created", page.name]),
    closed: (page) => {
      events.push(["closed", page.name]);
    },
    lifecycle: (page, event) => events.push([event.kind, page.name]),
    network: (page, entry) => {
      events.push(["network", page.name, entry.event ?? "request"]);
    },
    actorClosed: (actor) => events.push(["actorClosed", actor]),
    unsupported: () => events.push(["unsupported"]),
    failure: (error) => events.push(["failure", error]),
  };
  const hub = new LiveActorHub(
    native,
    { actor: 7, root: "root", capability: { kind: "supported" }, ...identity },
    root,
    callbacks,
  );
  return { hub, root, events, states, batches, snapshot, native, callbacks };
}

test("actor capability descriptors fail closed without publishing unowned identities", async () => {
  for (const capability of [{}, { kind: "invented" }]) {
    const f = protocol({ capability: { kind: "pending" } });
    await assert.rejects(
      f.hub.applyState(f.snapshot({ capability })),
      /Invalid native auxiliary capability/,
    );
    assert.deepEqual(f.events, []);
    assert.equal(f.hub.page("root"), f.root);
  }
  const established = protocol();
  await assert.rejects(
    established.hub.applyState(established.snapshot({ capability: { kind: "pending" } })),
    /Native root capability changed/,
  );
  assert.equal(established.hub.page("root"), established.root);
});

test("unsupported opening actors wake consumers and reject auxiliary identity publication", async () => {
  const f = protocol({ root: null, capability: undefined, opening: true });
  await f.hub.applyState(
    f.snapshot({ root: null, capability: { kind: "unsupported", reason: "no auxiliary API" } }),
  );
  assert.deepEqual(f.events, [["unsupported"], ["wake", 7]]);
  await assert.rejects(
    f.hub.applyState(f.snapshot({ capability: { kind: "unsupported" } })),
    /Unsupported root published auxiliary identities/,
  );
  assert.equal(f.hub.page("root"), undefined);
  await f.hub.applyState(
    f.snapshot({ root: null, capability: { kind: "unsupported" }, closed: true }),
  );
  await assert.rejects(f.hub.ready, /closed during initialization/);
  assert.deepEqual(f.events.slice(-2), [
    ["closed", "root"],
    ["actorClosed", 7],
  ]);
});

test("closed child snapshots cannot reopen an identity or repeat its initial-document notification", async () => {
  const f = protocol();
  f.callbacks.initialDocumentComplete = (page) => f.events.push(["initial", page.name]);
  const child = { target: "child", closed: false };
  await f.hub.applyState(
    f.snapshot({
      pages: [child],
      events: [{ kind: "created", target: "child", initialDocumentComplete: true }],
    }),
  );
  await f.hub.applyState(
    f.snapshot({
      pages: [{ ...child, closed: true }],
      events: [{ kind: "closed", target: "child" }],
    }),
  );
  await f.hub.applyState(
    f.snapshot({
      pages: [child],
      events: [{ kind: "created", target: "child", initialDocumentComplete: true }],
    }),
  );
  assert.equal(f.hub.canReadTarget("child"), false);
  assert.deepEqual(f.events, [
    ["initial", "child"],
    ["created", "child"],
    ["closed", "child"],
  ]);
});

test("network overflow fans out once and orphan body completions are consumed without misrouting", async () => {
  const f = protocol();
  f.states.push(
    f.snapshot({
      pages: [{ target: "child", closed: false }],
      events: [{ kind: "created", target: "child" }],
    }),
  );
  f.batches.push({
    revision: 5,
    entries: [
      { event: "networkoverflow" },
      {
        event: "request",
        id: "pending",
        bodyPending: true,
        context: { frame: { pageId: "child" } },
      },
      { event: "responsebody", id: "pending" },
      { event: "responsebody", id: "pending" },
      { event: "responsebody" },
    ],
  });
  await f.hub.drain();
  assert.deepEqual(f.events, [
    ["created", "child"],
    ["network", "root", "networkoverflow"],
    ["network", "child", "networkoverflow"],
    ["network", "child", "request"],
    ["network", "child", "responsebody"],
    ["ack", 7, 5],
  ]);
  f.batches.push([{ id: null, context: { frame: { pageId: "foreign" } } }]);
  await assert.rejects(f.hub.drain(), /no owned Page identity/);
  assert.equal(f.hub.page("root"), f.root);
});

test("history protocol rejects missing callbacks and unknown identities but accepts retired targets", async () => {
  const f = protocol();
  await assert.rejects(
    f.hub.applyState(f.snapshot({ events: [{ kind: "historyChanged", target: "root" }] })),
    /history callback is unavailable/,
  );
  await assert.rejects(
    f.hub.applyState(f.snapshot({ events: [{ kind: "historyChanged", target: "foreign" }] })),
    /no owned Page identity/,
  );
  await assert.rejects(
    f.hub.applyState(f.snapshot({ events: [{ kind: "created", target: "foreign" }] })),
    /creation has no owned identity/,
  );
  await f.hub.applyState(
    f.snapshot({
      pages: [{ target: "retired", closed: true }],
      events: [
        { kind: "historyChanged", target: "retired" },
        { kind: "load", target: "unknown" },
      ],
    }),
  );
  f.callbacks.historyChanged = (page, event) => f.events.push(["history", page.name, event.url]);
  await f.hub.applyState(
    f.snapshot({
      events: [{ kind: "historyChanged", target: "root", url: "https://example.test/next" }],
    }),
  );
  assert.deepEqual(f.events, [["history", "root", "https://example.test/next"]]);
});

test("actor overflow errors preserve ownership and a following valid turn remains usable", async () => {
  const f = protocol();
  await assert.rejects(
    f.hub.applyState(
      f.snapshot({ events: [{ kind: "overflow", message: "queue overflow marker" }] }),
    ),
    /queue overflow marker/,
  );
  await f.hub.applyState(f.snapshot({ events: [{ kind: "load", target: "root" }] }));
  assert.equal(f.hub.page("root"), f.root);
  assert.deepEqual(f.events, [["load", "root"]]);
});

test("pending capability holds lifecycle events until support arrives and does not consume network early", async () => {
  const f = protocol({ root: null, capability: { kind: "pending" }, opening: true });
  const rootInstallations = [];
  f.callbacks.root = (target) => rootInstallations.push(target);
  const child = { target: "child", closed: false };
  f.states.push(
    f.snapshot({
      root: null,
      capability: { kind: "pending" },
      events: [{ kind: "created", target: "child" }],
    }),
  );
  f.batches.push({ entries: [{ context: { frame: { pageId: "child" } } }] });
  await f.hub.drain();
  assert.deepEqual(f.events, []);
  assert.equal(f.batches.length, 1, "network ownership cannot precede capability negotiation");
  f.states.push(f.snapshot({ opening: false, pages: [child] }));
  await f.hub.drain();
  await f.hub.ready;
  assert.deepEqual(rootInstallations, ["root"]);
  assert.deepEqual(f.events, [
    ["created", "child"],
    ["network", "child", "request"],
  ]);
});

test("retiring a target is idempotent and applies its final native lifecycle receipt", async () => {
  const f = protocol();
  f.callbacks.closed = (page) => {
    f.events.push(["closed", page.name]);
    return JSON.stringify(
      f.snapshot({
        pages: [{ target: "root", closed: true }],
        events: [{ kind: "load", target: "root" }],
      }),
    );
  };
  await f.hub.closeTarget("root");
  await f.hub.closeTarget("root");
  assert.equal(f.hub.canReadTarget("root"), false);
  assert.equal(
    f.hub.page("root"),
    f.root,
    "retired Page remains identifiable for final lifecycle receipts",
  );
  assert.deepEqual(f.events, [
    ["closed", "root"],
    ["load", "root"],
  ]);
});
