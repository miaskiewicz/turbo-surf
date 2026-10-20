import assert from "node:assert/strict";
import { test } from "node:test";
import {
  captureStableDocument,
  documentSnapshotState,
  finishObservation,
  observationMilliseconds,
  observationTriggerIndex,
  remainingObservationMs,
  startObservation,
  waitForObservation,
} from "./observation-window.mjs";

function fakeClock() {
  let elapsed = 0;
  let wallOffset = 100000;
  return {
    monotonic: () => elapsed,
    wall: () => wallOffset + elapsed,
    advance: (milliseconds) => {
      elapsed += milliseconds;
    },
    shiftWall: (milliseconds) => {
      wallOffset += milliseconds;
    },
    pause: async (milliseconds) => {
      elapsed += milliseconds;
    },
  };
}

test("observation configuration is bounded and preserves explicit zero", () => {
  assert.equal(observationMilliseconds({}), 2500);
  assert.equal(observationMilliseconds({ waitMs: 100 }), 100);
  assert.equal(observationMilliseconds({ waitMs: 100, observationMs: 1000 }), 1000);
  assert.equal(observationMilliseconds({ observationMs: 0 }), 0);
  for (const value of [null, -1, 60001, 1.2, "100", NaN, Infinity, true])
    assert.throws(() => observationMilliseconds({ observationMs: value }));
});

test("the final Enter defines the shared trigger and no Enter stays unmeasured", () => {
  assert.equal(observationTriggerIndex([]), -1);
  assert.equal(observationTriggerIndex([{ press: "Tab" }, { click: "button" }]), -1);
  assert.equal(observationTriggerIndex([{ press: "Enter" }, { wait: 50 }, { press: "Enter" }]), 2);
});

test("input time consumes the original deadline instead of renewing it", async () => {
  const clock = fakeClock();
  const window = startObservation(1000, 5, clock);
  clock.advance(600); // Form submission blocked while the response arrived.
  assert.equal(remainingObservationMs(window, clock), 400);
  await waitForObservation(window, clock, clock.pause);
  const record = finishObservation(window, clock, true);
  assert.equal(record.elapsedMs, 1000);
  assert.equal(record.overrunMs, 0);
  assert.equal(record.triggerStepIndex, 5);
  assert.equal(record.deadlineMs, 101000);
});

test("positive results do not end the window and negative results do not extend it", async () => {
  for (const status of [200, 429]) {
    const clock = fakeClock();
    const window = startObservation(1000, 5, clock);
    let pumps = 0;
    await waitForObservation(window, clock, clock.pause, async () => {
      pumps++;
    });
    const record = finishObservation(window, clock, true, { finalStatus: status });
    assert.equal(record.elapsedMs, 1000);
    assert.equal(pumps, 40);
    assert.equal(record.finalStatus, status);
  }
});

test("late input and slow snapshot are recorded as overruns without an extra wait", async () => {
  const clock = fakeClock();
  const window = startObservation(100, 5, clock);
  clock.advance(600);
  let pauses = 0;
  await waitForObservation(window, clock, async () => {
    pauses++;
  });
  clock.advance(25); // Final snapshot time is included.
  const record = finishObservation(window, clock, true);
  assert.equal(pauses, 0);
  assert.equal(record.elapsedMs, 625);
  assert.equal(record.overrunMs, 525);
});

test("host wall clock changes do not extend a monotonic observation window", async () => {
  const clock = fakeClock();
  const window = startObservation(100, 5, clock);
  clock.shiftWall(-10000);
  await waitForObservation(window, clock, clock.pause);
  const record = finishObservation(window, clock, true);
  assert.equal(record.elapsedMs, 100);
  assert.equal(record.overrunMs, 0);
  assert.equal(record.finishedMs, 90100);
});

test("failed captures preserve incomplete state and the original deadline", () => {
  const clock = fakeClock();
  const window = startObservation(100, 5, clock);
  clock.advance(50);
  const record = finishObservation(window, clock, false, { error: "input failed" });
  assert.equal(record.completed, false);
  assert.equal(record.elapsedMs, 50);
  assert.equal(record.deadlineMs, 100100);
});

test("document capture retries changed loaders without mixing their status and DOM", async () => {
  let loader = "old";
  let calls = 0;
  const captured = await captureStableDocument(
    async () => loader,
    async () => {
      calls++;
      if (calls === 1) {
        loader = "new";
        return { dom: "old", status: 200 };
      }
      return { dom: "new", status: 429 };
    },
  );
  assert.deepEqual(captured, { value: { dom: "new", status: 429 }, retries: 1 });
});

test("document capture bounds navigation retries and surfaces unrelated failures", async () => {
  let calls = 0;
  await assert.rejects(
    captureStableDocument(
      async () => "loader",
      async () => {
        calls++;
        throw new Error("Execution context was destroyed");
      },
    ),
    /all 3 snapshot attempts/,
  );
  assert.equal(calls, 3);
  await assert.rejects(
    captureStableDocument(
      async () => "loader",
      async () => {
        throw new Error("CDP transport disconnected");
      },
    ),
    /CDP transport disconnected/,
  );
});

test("rootless current documents retain no earlier markup or result markers", async () => {
  const oldDocument = {
    documentElement: { outerHTML: '<html><div id="rso"><a><h3>Old result</h3></a></div></html>' },
    doctype: { name: "html" },
    querySelectorAll: () => [{}],
  };
  assert.deepEqual(documentSnapshotState(oldDocument, "#rso a h3"), {
    documentElementPresent: true,
    html: '<!DOCTYPE html><html><div id="rso"><a><h3>Old result</h3></a></div></html>',
    results: 1,
  });
  const rootless = {
    documentElement: null,
    doctype: { name: "html" },
    querySelectorAll: () => {
      throw new Error("Rootless documents must not reuse marker queries");
    },
  };
  let loader = "old";
  let calls = 0;
  const result = await captureStableDocument(
    async () => loader,
    async () => {
      if (++calls === 1) {
        loader = "new";
        return { ...documentSnapshotState(oldDocument, "#rso a h3"), status: 200 };
      }
      return { ...documentSnapshotState(rootless, "#rso a h3"), status: 429 };
    },
  );
  assert.deepEqual(result, {
    retries: 1,
    value: { documentElementPresent: false, html: "", results: 0, status: 429 },
  });
  const stable = await captureStableDocument(
    async () => "new",
    async () => {
      calls++;
      return documentSnapshotState(rootless, "#rso a h3");
    },
  );
  assert.equal(stable.retries, 0);
  assert.equal(calls, 3); // Root absence does not create readiness retries.
});

test("snapshot markup preserves each collector's doctype policy in one root read", () => {
  let rootReads = 0;
  const doctype = { name: "html", publicId: "fixture-public", systemId: "fixture-system" };
  const document = {
    get documentElement() {
      rootReads++;
      return { outerHTML: "<html></html>" };
    },
    doctype,
    querySelectorAll: () => [],
  };
  assert.equal(documentSnapshotState(document, "h3").html, "<!DOCTYPE html><html></html>");
  assert.equal(rootReads, 1);
  const serialized = '<!DOCTYPE html PUBLIC "fixture-public" "fixture-system">';
  const calls = [];
  assert.equal(
    documentSnapshotState(document, "h3", (value) => {
      calls.push(value);
      return serialized;
    }).html,
    serialized + "<html></html>",
  );
  assert.deepEqual(calls, [doctype]);
  assert.equal(rootReads, 2);
  for (const root of [null, undefined]) {
    assert.deepEqual(
      documentSnapshotState({ documentElement: root, doctype }, "h3", () => {
        throw new Error("Rootless documents must not serialize an old doctype");
      }),
      { documentElementPresent: false, html: "", results: 0 },
    );
  }
});
