import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function settle() {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

for (const [method, event] of [
  ["waitForRequest", "request"],
  ["waitForResponse", "response"],
]) {
  test(`${method} keeps the standard 30s default while another action runs past 15s`, async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const page = shim.newPage();
    let outcome;
    const payload = { url: () => "http://localhost/fixture" };
    const pending = page[method]("**/fixture").then(
      (value) => {
        outcome = { value };
      },
      (error) => {
        outcome = { error };
      },
    );
    try {
      t.mock.timers.tick(16000);
      await settle();
      assert.equal(
        outcome,
        undefined,
        "the waiter must remain pending after a 16s concurrent action",
      );
      page._emit(event, payload);
      await pending;
      assert.equal(outcome.value, payload);
    } finally {
      page._emit(event, payload);
      await pending;
      await page.close();
    }
  });

  test(`${method} timeout precedence and zero preserve explicit/page/context settings`, async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const cases = [
      { name: "standard", expected: 30000 },
      { name: "context", context: 37, expected: 37 },
      { name: "page", context: 37, page: 23, expected: 23 },
      { name: "explicit", context: 37, page: 23, explicit: 11, expected: 11 },
      { name: "zero explicit", context: 37, page: 23, explicit: 0, expected: 0 },
      { name: "zero page", context: 37, page: 0, expected: 0 },
      { name: "zero context", context: 0, expected: 0 },
      { name: "explicit overrides zero", page: 0, explicit: 11, expected: 11 },
      { name: "navigation unrelated", navigation: 5, expected: 30000 },
      { name: "configured action", action: 31, expected: 31 },
      { name: "configured zero action", action: 0, expected: 0 },
    ];
    for (const scenario of cases) {
      const context = await shim.chromium
        .launch()
        .then((browser) =>
          browser.newContext(
            scenario.action === undefined ? {} : { actionTimeout: scenario.action },
          ),
        );
      const page = await context.newPage();
      // Context changes made after page construction must still be inherited.
      if (scenario.context !== undefined) context.setDefaultTimeout(scenario.context);
      if (scenario.page !== undefined) page.setDefaultTimeout(scenario.page);
      if (scenario.navigation !== undefined) {
        context.setDefaultNavigationTimeout(scenario.navigation);
        page.setDefaultNavigationTimeout(scenario.navigation);
      }
      let outcome;
      const payload = { url: () => "http://localhost/fixture" };
      const pending = page[method](
        "**/fixture",
        scenario.explicit === undefined ? {} : { timeout: scenario.explicit },
      ).then(
        (value) => {
          outcome = { value };
        },
        (error) => {
          outcome = { error };
        },
      );
      try {
        if (scenario.expected === 0) {
          t.mock.timers.tick(60000);
          await settle();
          assert.equal(outcome, undefined, scenario.name);
          page._emit(event, payload);
          await pending;
          assert.equal(outcome.value, payload, scenario.name);
        } else {
          t.mock.timers.tick(scenario.expected - 1);
          await settle();
          assert.equal(outcome, undefined, `${scenario.name}: no early timeout`);
          t.mock.timers.tick(1);
          await settle();
          assert.equal(outcome?.error?.name, "TimeoutError", scenario.name);
        }
        assert.equal(
          page._listeners.get(event)?.length ?? 0,
          0,
          `${scenario.name}: listener removed`,
        );
      } finally {
        page._emit(event, payload);
        await pending;
        await context.close();
      }
    }
  });
}
