import assert from "node:assert/strict";
import test from "node:test";
import { createPopupFixture } from "./fixtures/popup-app.mjs";

const fixture = await createPopupFixture();
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([fixture.origin]);
const { BrowserContext } = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");

test(
  "target networkidle advances a delayed fetch and its DOM continuation",
  { timeout: 15000 },
  async () => {
    const context = new BrowserContext();
    try {
      const page = await context.newPage();
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      assert.equal(
        page._readEngine(`
      globalThis.__idleContinuationDone = false;
      setTimeout(() => {
        fetch('/presign').then(response => response.text()).then(value => {
          document.body.setAttribute('data-idle-result', value);
          globalThis.__idleContinuationDone = true;
        });
      }, 100);
      return globalThis.__idleContinuationDone;
    `),
        false,
      );
      await page.waitForLoadState("networkidle", { timeout: 3000 });
      // This read deliberately cannot advance timers or Promise jobs and mask a stalled observer.
      assert.deepEqual(
        page._readEngine(`return {
      done: globalThis.__idleContinuationDone,
      result: document.body.getAttribute('data-idle-result')
    };`),
        { done: true, result: "/document?kind=presigned" },
      );
    } finally {
      await context.close();
      await fixture.close();
    }
  },
);
