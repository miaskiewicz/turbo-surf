import assert from "node:assert/strict";
import test from "node:test";
import {
  notifyNativeHistory,
  notifyNativeHistoryAcknowledged,
  syncNativeHistory,
  syncNativeHistoryAcknowledged,
  validateHistorySnapshot,
} from "./history-snapshot.mjs";
const row = (row, group, local, url = "/same") => ({ row, group, local, url });
const snapshot = () => ({
  revision: "9",
  document: "71",
  group: "40",
  closed: false,
  index: 2,
  entries: [
    row("1", null, 0, "/raw"),
    row("2", "30", 1),
    row("3", "40", 1),
    row("4", "40", 2),
    row("5", "50", 1),
  ],
});
function fixture() {
  const events = [];
  const page = {
    _session: 7,
    _auxiliaryActor: true,
    _targetId: "20",
    _actorHub: {
      canReadTarget: (id) => id === "20",
      page: (id) => (id === "20" ? page : undefined),
    },
    _emit: (...event) => events.push(event),
    _url: "/raw",
    _history: [],
    _fwd: [],
  };
  let state = snapshot();
  return {
    page,
    events,
    set: (value) => {
      state = value;
    },
    native: {
      liveTargetHistoryState: (id) => {
        assert.equal(id, 7);
        return JSON.stringify(state);
      },
    },
  };
}
test("native rows preserve repeated URLs, logical groups and real document identity", () => {
  const { page, native } = fixture();
  assert.equal(syncNativeHistory(page, native), true);
  assert.equal(page._history[0], "/raw");
  assert.deepEqual(page._history[1], {
    row: "2",
    id: 1,
    url: "/same",
    group: "7:30",
    document: null,
  });
  assert.deepEqual(page._historyCurrent, {
    row: "3",
    id: 1,
    url: "/same",
    group: "7:40",
    document: "7:71",
  });
  assert.deepEqual(
    page._fwd.map((entry) => entry.row),
    ["5", "4"],
  );
});
test("a new native snapshot replaces stale driver deltas instead of appending twice", () => {
  const { page, native } = fixture();
  page._history.push("/invented");
  page._fwd.push("/stale");
  syncNativeHistory(page, native);
  syncNativeHistory(page, native);
  assert.deepEqual(
    page._history.map((entry) => (typeof entry === "string" ? entry : entry.row)),
    ["/raw", "2"],
  );
  assert.deepEqual(
    page._fwd.map((entry) => entry.row),
    ["5", "4"],
  );
});
test("metadata reads never synthesize mutation notifications", () => {
  const f = fixture();
  syncNativeHistory(f.page, f.native);
  const next = snapshot();
  next.revision = "10";
  next.index = 3;
  f.set(next);
  syncNativeHistory(f.page, f.native);
  syncNativeHistory(f.page, f.native);
  assert.equal(f.events.length, 0);
  next.revision = "11";
  next.document = "72";
  f.set(next);
  syncNativeHistory(f.page, f.native);
  assert.equal(f.events.length, 0);
  assert.equal(f.page._historyCurrent.document, "7:72");
});
test("closed, missing, duplicate and unbound current identities fail before changing Page state", () => {
  for (const mutate of [
    (s) => {
      s.closed = true;
    },
    (s) => {
      s.entries[1].row = "1";
    },
    (s) => {
      s.entries[3].local = 1;
    },
    (s) => {
      s.entries[2].group = null;
      s.entries[2].local = 0;
    },
    (s) => {
      s.document = "guest-owner";
    },
  ]) {
    const f = fixture();
    const bad = snapshot();
    mutate(bad);
    f.set(bad);
    assert.throws(() => syncNativeHistory(f.page, f.native));
    assert.deepEqual(f.page._history, []);
    assert.equal(f.page._url, "/raw");
  }
});
test("unowned target never guesses the actor root and native errors remain visible", () => {
  const f = fixture();
  f.page._targetId = "unknown";
  assert.throws(
    () =>
      syncNativeHistory(f.page, {
        liveTargetHistoryState: () => {
          throw new Error("must not read");
        },
      }),
    /ownership/,
  );
  f.page._targetId = "20";
  assert.throws(
    () =>
      syncNativeHistory(f.page, {
        liveTargetHistoryState: () => {
          throw new Error("target closed");
        },
      }),
    /target closed/,
  );
});
test("a stale revision cannot overwrite an accepted current snapshot", () => {
  const f = fixture();
  syncNativeHistory(f.page, f.native);
  const bad = snapshot();
  bad.revision = "8";
  f.set(bad);
  assert.throws(() => syncNativeHistory(f.page, f.native), /backwards/);
  assert.equal(f.page._nativeHistory.revision, "9");
});
test("snapshot validation refuses guest-state payloads at the metadata boundary", () => {
  const value = snapshot();
  value.entries[2].state = { secret: "must remain native" };
  assert.throws(() => validateHistorySnapshot(value), /must not contain guest state/);
});

const notification = (revision, url, extra = {}) => ({
  target: "20",
  revision,
  url,
  document: "71",
  group: "40",
  row: "3",
  local: 1,
  ...extra,
});
test("ordered native notifications retain every historical URL without replaying stack deltas", () => {
  const f = fixture();
  const observed = [];
  f.page._emit = (kind, frame) => observed.push([kind, frame === f.page, frame._url]);
  notifyNativeHistory(f.page, f.native, notification("7", "/first"));
  notifyNativeHistory(f.page, f.native, notification("8", "/second"));
  notifyNativeHistory(f.page, f.native, notification("8", "/second"));
  assert.deepEqual(observed, [
    ["framenavigated", true, "/first"],
    ["framenavigated", true, "/second"],
  ]);
  assert.equal(f.page._url, "/same");
  assert.deepEqual(
    f.page._history.map((entry) => (typeof entry === "string" ? entry : entry.row)),
    ["/raw", "2"],
  );
});
test("an old physical document's valid queued event precedes later-document metadata", () => {
  const f = fixture();
  const actual = snapshot();
  actual.document = "99";
  actual.group = "40";
  f.set(actual);
  assert.equal(
    notifyNativeHistory(f.page, f.native, notification("8", "/old-event", { document: "70" })),
    true,
  );
  assert.equal(f.page._nativeHistory.document, "99");
  assert.equal(f.page._url, "/same");
});
test("unknown, conflicting or regressing native notifications fail closed", () => {
  const f = fixture();
  assert.throws(
    () => notifyNativeHistory(f.page, f.native, notification("8", "/no", { target: "unknown" })),
    /owned/,
  );
  notifyNativeHistory(f.page, f.native, notification("8", "/yes"));
  assert.throws(
    () => notifyNativeHistory(f.page, f.native, notification("8", "/different")),
    /out of order/,
  );
  assert.throws(
    () => notifyNativeHistory(f.page, f.native, notification("7", "/older")),
    /out of order/,
  );
});
test("retired targets skip native reads and listener errors preserve the latest URL", () => {
  const f = fixture();
  f.page._actorHub.nativeClosed = true;
  assert.equal(
    notifyNativeHistory(
      f.page,
      {
        liveTargetHistoryState: () => {
          throw new Error("must not read");
        },
      },
      notification("8", "/old"),
    ),
    false,
  );
  f.page._actorHub.nativeClosed = false;
  f.page._emit = () => {
    throw new Error("listener failure");
  };
  assert.throws(
    () => notifyNativeHistory(f.page, f.native, notification("8", "/old")),
    /listener failure/,
  );
  assert.equal(f.page._url, "/same");
});

test("acknowledged history read waits for owned publication without reading pending metadata", async () => {
  const f = fixture();
  let publish;
  let calls = 0;
  f.native.liveTargetHistoryState = () => {
    throw new Error("Native history mutation is awaiting private acknowledgement");
  };
  f.native.liveTargetHistoryStateAsync = (id, timeout) => {
    calls++;
    assert.equal(id, 7);
    assert.equal(timeout, 0);
    return new Promise((resolve) => {
      publish = resolve;
    });
  };
  const reading = syncNativeHistoryAcknowledged(f.page, f.native);
  assert.equal(f.page._url, "/raw");
  await new Promise((resolve) => setImmediate(resolve));
  publish(JSON.stringify(snapshot()));
  assert.equal(await reading, true);
  assert.equal(calls, 1);
  assert.equal(f.page._url, "/same");
});

test("acknowledged history preserves the caller deadline and native timeout error", async () => {
  const f = fixture();
  let calls = 0;
  const error = new Error("Native history acknowledgement wait timed out");
  f.native.liveTargetHistoryStateAsync = async (_id, timeout) => {
    calls++;
    assert.ok(timeout > 0 && timeout <= 1000);
    throw error;
  };
  await assert.rejects(
    syncNativeHistoryAcknowledged(f.page, f.native, Date.now() + 1000),
    (actual) => actual === error && actual.name === "TimeoutError",
  );
  await assert.rejects(syncNativeHistoryAcknowledged(f.page, f.native, Date.now() - 1), {
    name: "TimeoutError",
  });
  assert.equal(calls, 1);
  assert.equal(f.page._url, "/raw");
});

test("acknowledged history does not retry or replace genuine native errors", async () => {
  for (const message of [
    "Native history metadata lock poisoned",
    "Native history target is closed",
    "Native history mutation is awaiting private acknowledgement",
  ]) {
    const f = fixture();
    let calls = 0;
    const error = new Error(message);
    f.native.liveTargetHistoryStateAsync = async () => {
      calls++;
      throw error;
    };
    await assert.rejects(
      syncNativeHistoryAcknowledged(f.page, f.native),
      (actual) => actual === error,
    );
    assert.equal(calls, 1);
    assert.equal(f.page._url, "/raw");
  }
});

test("acknowledged history cannot apply a snapshot after its actual Page owner changes", async () => {
  const f = fixture();
  let publish;
  f.native.liveTargetHistoryStateAsync = () =>
    new Promise((resolve) => {
      publish = resolve;
    });
  const reading = syncNativeHistoryAcknowledged(f.page, f.native);
  f.page._session = 8;
  publish(JSON.stringify(snapshot()));
  await assert.rejects(reading, /closed or superseded/);
  assert.equal(f.page._url, "/raw");
});

test("history notifications wait for acknowledgement before publishing ordered events", async () => {
  const f = fixture();
  let publish;
  f.native.liveTargetHistoryStateAsync = () =>
    new Promise((resolve) => {
      publish = resolve;
    });
  const event = {
    target: "20",
    document: "71",
    group: "40",
    revision: "9",
    row: "3",
    local: 1,
    url: "/same",
  };
  const notifying = notifyNativeHistoryAcknowledged(f.page, f.native, event);
  assert.equal(f.events.length, 0);
  publish(JSON.stringify(snapshot()));
  assert.equal(await notifying, true);
  assert.equal(f.events.length, 1);
  assert.equal(f.page._nativeHistoryEvent.revision, "9");
});
