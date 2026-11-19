import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";
import { newPage } from "../../rust/playwright-shim/index.mjs";

const native = createRequire(import.meta.url)("../../rust/crates/turbo-surf-napi/index.js");
const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];

export async function measureEvaluationSync(page, collector, iterations = 10) {
  const counts = { refresh: 0, bindings: 0, serialize: 0, cookies: 0, engine: 0, expression: 0 };
  const restores = [];
  const wrap = (object, name, counter) => {
    const original = object[name];
    if (!original) return;
    object[name] = function (...args) {
      counts[counter]++;
      return original.apply(this, args);
    };
    restores.push(() => {
      object[name] = original;
    });
  };
  wrap(page, "_refreshFromSession", "refresh");
  wrap(page, "_drainBindings", "bindings");
  for (const [name, counter] of [
    ["liveSerialize", "serialize"],
    ["liveCookies", "cookies"],
    ["liveEvalEngine", "engine"],
    ["liveEvalExpression", "expression"],
    ["liveEvalExpressionTracked", "expression"],
  ])
    wrap(native, name, counter);
  try {
    const timings = [];
    let value;
    for (let index = 0; index < iterations; index++) {
      const start = performance.now();
      value = await page.evaluate(collector);
      timings.push(performance.now() - start);
    }
    return { iterations, counts, medianMs: median(timings), value };
  } finally {
    for (const restore of restores.reverse()) restore();
  }
}

export async function runEvaluationSyncBenchmark(iterations = 25) {
  const results = [];
  for (const rows of [20, 5000]) {
    const page = newPage();
    try {
      await page.setContent(`<title>collector</title><h1>heading</h1>${"<p>row</p>".repeat(rows)}`);
      const promotionStart = performance.now();
      await page._ensureLive();
      const promotionMs = performance.now() - promotionStart;
      const result = await measureEvaluationSync(
        page,
        () => ({ title: document.title, heading: document.querySelector("h1").textContent }),
        iterations,
      );
      assert.deepEqual(result.value, { title: "collector", heading: "heading" });
      results.push({ rows, phase: "retained-live-collector", promotionMs, ...result });
    } finally {
      await page.close();
    }
  }
  return results;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  console.log(
    JSON.stringify(await runEvaluationSyncBenchmark(Number(process.argv[2]) || 25), null, 2),
  );
