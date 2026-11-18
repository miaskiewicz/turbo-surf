import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(
  new URL("../../rust/crates/turbo-surf-render/src/timer_queue.js", import.meta.url),
  "utf8",
);
function queue() {
  return vm.runInNewContext(
    `${source}\n({timers:__timers,push:__timerPush,splice:__timerSplice,clear:__timerClearQueue,next:__nextTimerIndex,base:__taskBaseFor})`,
  );
}
function referenceIndex(timers) {
  let best = 0;
  for (let i = 1; i < timers.length; i++) {
    const a = timers[i],
      b = timers[best];
    if (a.due < b.due || (a.due === b.due && a.sequence < b.sequence)) best = i;
  }
  return best;
}
function referenceBase(timers, due) {
  let base = due;
  for (const timer of timers)
    if (!timer.rendering && timer.due > base && timer.due < due + 1) base = timer.due;
  return base;
}
function verify(actual, expected) {
  assert.equal(actual.next(), referenceIndex(expected));
  for (const due of [-1, 0, 1, 2.5, 4, 10])
    assert.equal(actual.base(due), referenceBase(expected, due));
  assert.deepEqual(
    Array.from(actual.timers, (timer) => timer.id),
    expected.map((timer) => timer.id),
  );
}

test("production timer queue matches the scanner through generated mutations", () => {
  const actual = queue(),
    expected = [];
  let seed = 17;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed;
  };
  for (let id = 0; id < 1200; id++) {
    const action = random() % 8;
    if (action < 4) {
      const timer = {
        id,
        due: (random() % 40) / 4,
        sequence: random() % 6,
        rendering: random() % 3 === 0,
      };
      actual.push(timer);
      expected.push(timer);
    } else if (action === 4) {
      const timer = { id, due: 2, sequence: 0, rendering: true };
      actual.splice(0, 0, timer);
      expected.splice(0, 0, timer);
    } else if (action === 5 && expected.length) {
      const index = referenceIndex(expected);
      const [timer] = expected.splice(index, 1);
      actual.splice(index, 1);
      const repeated = { ...timer, due: (random() % 20) / 4, sequence: id };
      actual.push(repeated);
      expected.push(repeated);
    } else if (action === 6 && expected.length) {
      const index = random() % expected.length;
      actual.splice(index, 1);
      expected.splice(index, 1);
    } else {
      actual.clear();
      expected.length = 0;
    }
    verify(actual, expected);
  }
});

test("production timer queue preserves equal keys, strict task-base bounds and nonfinite fallback", () => {
  for (const timers of [
    [
      { id: 1, due: 0, sequence: 1 },
      { id: 2, due: 0.2, sequence: 2 },
      { id: 3, due: 0.9, sequence: 3, rendering: true },
      { id: 4, due: 1, sequence: 4 },
    ],
    [
      { id: 1, due: 0, sequence: 1 },
      { id: 2, due: 0, sequence: 1 },
      { id: 3, due: 0.9, sequence: 2, rendering: true },
      { id: 4, due: 1, sequence: 3 },
    ],
    [
      { id: 1, due: NaN, sequence: 1 },
      { id: 2, due: 0, sequence: 2 },
    ],
    [
      { id: 1, due: 0, sequence: Infinity },
      { id: 2, due: 0, sequence: 2 },
    ],
  ]) {
    const actual = queue();
    for (const timer of timers) actual.push(timer);
    verify(actual, timers);
  }
});

test("ordered 4096-task burst has subquadratic production queue reads", (t) => {
  const actual = queue();
  let reads = 0;
  for (let id = 0; id < 4096; id++)
    actual.push({
      id,
      sequence: id,
      get due() {
        reads++;
        return id / 10000;
      },
    });
  while (actual.timers.length) {
    const index = actual.next(),
      timer = actual.timers[index];
    actual.splice(index, 1);
    actual.base(timer.due);
  }
  t.diagnostic(`actual deadline reads: ${reads}`);
  assert.ok(reads < 4096 * 100, `actual deadline reads: ${reads}`);
});
