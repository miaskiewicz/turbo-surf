import assert from "node:assert/strict";
import test from "node:test";
import { historyFixture, popup, assertSameBack, assign } from "./history-group-fixture.mjs";

if (process.env.TURBO_SURF_HISTORY_CHROME === "1")
  throw new Error("Native configurable history-policy controls must run against the shim");

test("history state byte failures preserve current object URL and forward entries", async () => {
  await historyFixture(
    async ({ root, base, requests }) => {
      const page = await popup(root);
      await page.evaluate(() => {
        history.pushState({ n: 1 }, "", "/first");
        history.pushState({ n: 2 }, "", "/second");
      });
      await assertSameBack(page, "/first");
      assert.deepEqual(
        await page.evaluate(() => {
          const saved = history.state;
          const failures = [];
          for (const operation of ["pushState", "replaceState"]) {
            try {
              history[operation]({ bytes: "x".repeat(5000) }, "", "/must-not-commit");
              failures.push("committed");
            } catch (error) {
              failures.push({
                name: error.name,
                local: Object.getPrototypeOf(error) === DOMException.prototype,
              });
            }
          }
          return {
            failures,
            path: location.pathname,
            state: history.state,
            same: history.state === saved,
            length: history.length,
          };
        }),
        {
          failures: [
            { name: "DataCloneError", local: true },
            { name: "DataCloneError", local: true },
          ],
          path: "/first",
          state: { n: 1 },
          same: true,
          length: 3,
        },
      );
      assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
      assert.equal(page.url(), `${base}/second`);
      assert.deepEqual(await page.evaluate(() => history.state), { n: 2 });
      assert.deepEqual(requests, ["/host", "/one"]);
    },
    { contextOptions: { auxiliaryPolicy: { maxHistoryStateBytes: 2048 } } },
  );
});

test("history entry limit rejects growth but permits replacement and existing traversal", async () => {
  await historyFixture(
    async ({ root, requests }) => {
      const page = await popup(root);
      await page.evaluate(() => {
        history.pushState({ n: 1 }, "", "/first");
        history.pushState({ n: 2 }, "", "/second");
      });
      const failed = await page.evaluate(() => {
        const saved = history.state;
        try {
          history.pushState({ n: 3 }, "", "/overflow");
          return { committed: true };
        } catch (error) {
          return {
            committed: false,
            message: error.message,
            path: location.pathname,
            same: history.state === saved,
            length: history.length,
          };
        }
      });
      assert.equal(failed.committed, false);
      assert.match(failed.message, /history.*(budget|limit)/i);
      assert.equal(failed.path, "/second");
      assert.equal(failed.same, true);
      assert.equal(failed.length, 3);
      await assertSameBack(page, "/first");
      await page.evaluate(() => history.replaceState({ n: 4 }, "", "/replacement"));
      assert.equal(await page.goForward({ waitUntil: "load", timeout: 5000 }), null);
      assert.deepEqual(await page.evaluate(() => history.state), { n: 2 });
      await assertSameBack(page, "/replacement");
      assert.deepEqual(await page.evaluate(() => history.state), { n: 4 });
      assert.deepEqual(requests, ["/host", "/one"]);
    },
    { contextOptions: { auxiliaryPolicy: { maxHistoryEntries: 3 } } },
  );
});

test("a state getter's successful sibling mutation is retained before outer actor-budget rejection", async () => {
  await historyFixture(
    async ({ root, base, requests }) => {
      const sibling = await popup(root, "/b", "budgetB");
      const page = await popup(root, "/a", "budgetA");
      await page.evaluate(() => history.replaceState({ tag: "A original" }, "", "/a-original"));
      assert.deepEqual(
        await page.evaluate(() => {
          const saved = history.state;
          let calls = 0;
          const data = Object.defineProperty({}, "bytes", {
            enumerable: true,
            get() {
              calls++;
              opener.budgetB.history.pushState({ bytes: "b".repeat(2600) }, "", "/b-committed");
              return "a".repeat(2600);
            },
          });
          try {
            history.pushState(data, "", "/a-must-not-commit");
            return { committed: true, calls };
          } catch (error) {
            return {
              committed: false,
              calls,
              name: error.name,
              local: Object.getPrototypeOf(error) === DOMException.prototype,
              path: location.pathname,
              same: history.state === saved,
              tag: history.state.tag,
              length: history.length,
            };
          }
        }),
        {
          committed: false,
          calls: 1,
          name: "DataCloneError",
          local: true,
          path: "/a-original",
          same: true,
          tag: "A original",
          length: 1,
        },
      );
      assert.equal(sibling.url(), `${base}/b-committed`);
      assert.deepEqual(
        await sibling.evaluate(() => ({
          path: location.pathname,
          bytes: history.state.bytes.length,
          length: history.length,
        })),
        { path: "/b-committed", bytes: 2600, length: 2 },
      );
      assert.equal(await page.goBack({ waitUntil: "load", timeout: 5000 }), null);
      assert.equal(page.url(), `${base}/a-original`);
      assert.deepEqual(requests, ["/host", "/b", "/a"]);
    },
    { contextOptions: { auxiliaryPolicy: { maxHistoryStateBytes: 4096 } } },
  );
});

test("getter failure preserves its original guest exception after a successful sibling commit", async () => {
  await historyFixture(async ({ root, base }) => {
    const sibling = await popup(root, "/b", "getterB");
    const page = await popup(root, "/a", "getterA");
    assert.deepEqual(
      await page.evaluate(() => {
        const sentinel = new Error("original state getter error");
        const data = Object.defineProperty({}, "value", {
          enumerable: true,
          get() {
            opener.getterB.history.pushState({ tag: "inner committed" }, "", "/inner");
            throw sentinel;
          },
        });
        try {
          history.pushState(data, "", "/outer");
          return { committed: true };
        } catch (error) {
          return {
            committed: false,
            identical: error === sentinel,
            local: Object.getPrototypeOf(error) === Error.prototype,
            path: location.pathname,
            state: history.state,
            length: history.length,
          };
        }
      }),
      { committed: false, identical: true, local: true, path: "/a", state: null, length: 1 },
    );
    assert.equal(sibling.url(), `${base}/inner`);
    assert.deepEqual(await sibling.evaluate(() => history.state), { tag: "inner committed" });
  });
});

test("getter-closing a popup rejects outer storage and never revives its retired history", async () => {
  await historyFixture(async ({ root, base }) => {
    const sibling = await popup(root, "/b", "closingB");
    const page = await popup(root, "/a", "closingA");
    await root.evaluate(() => {
      window.retiredHistory = closingA.history;
      window.retiredExceptionPrototype = closingA.DOMException.prototype;
      window.retiredErrorPrototype = closingA.Error.prototype;
    });
    const closed = page.waitForEvent("close", { timeout: 5000 });
    closed.catch(() => {});
    let result;
    try {
      result = await page.evaluate(() => {
        const data = Object.defineProperty({}, "value", {
          enumerable: true,
          get() {
            opener.closingB.history.pushState({ tag: "surviving sibling" }, "", "/surviving");
            window.close();
            return 7;
          },
        });
        try {
          history.pushState(data, "", "/must-not-revive");
          return { committed: true };
        } catch (error) {
          return {
            committed: false,
            local:
              Object.getPrototypeOf(error) === DOMException.prototype ||
              Object.getPrototypeOf(error) === Error.prototype,
          };
        }
      });
      assert.deepEqual(result, { committed: false, local: true });
    } catch (error) {
      assert.match(error.message, /closed|destroyed|retired|no such live session/i);
    }
    await closed;
    assert.equal(page.isClosed(), true);
    assert.equal(sibling.url(), `${base}/surviving`);
    assert.deepEqual(await sibling.evaluate(() => history.state), { tag: "surviving sibling" });
    assert.deepEqual(
      await root.evaluate(() => {
        try {
          retiredHistory.pushState({ secret: 1 }, "", "/revived");
          return { committed: true };
        } catch (error) {
          return {
            committed: false,
            local:
              Object.getPrototypeOf(error) === retiredExceptionPrototype ||
              Object.getPrototypeOf(error) === retiredErrorPrototype,
            closed: closingA.closed,
          };
        }
      }),
      { committed: false, local: true, closed: true },
    );
  });
});

test(
  "out-of-group Navigation traversal rejects both promises without changing the owned document",
  { timeout: 7000 },
  async () => {
    await historyFixture(async ({ root, base, requests }) => {
      const page = await popup(root);
      await assign(page, `${base}/other`);
      const result = await page.evaluate(async () => {
        const pending = navigation.back();
        const values = await Promise.allSettled([pending.committed, pending.finished]);
        return {
          values: values.map((value) => ({ status: value.status, name: value.reason?.name })),
          path: location.pathname,
        };
      });
      assert.deepEqual(result, {
        values: [
          { status: "rejected", name: "NotSupportedError" },
          { status: "rejected", name: "NotSupportedError" },
        ],
        path: "/other",
      });
      await page.goBack({ waitUntil: "load", timeout: 5000 });
      assert.equal(new URL(page.url()).pathname, "/one");
      assert.deepEqual(requests, ["/host", "/one", "/other", "/one"]);
    });
  },
);
