import assert from "node:assert/strict";
import test from "node:test";
import { createPopupFixture } from "./fixtures/popup-app.mjs";

const fixture = await createPopupFixture();
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([fixture.origin]);
let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  await fixture.close();
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("popup network idle: native addon not built, skipping");
  process.exit(0);
}
const { BrowserContext } = shim;

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
      globalThis.__idleTimerFired = false;
      setTimeout(() => {
        globalThis.__idleTimerFired = true;
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
      const observed = page._readEngine(`return {
        timer: globalThis.__idleTimerFired,
        done: globalThis.__idleContinuationDone,
        result: document.body.getAttribute('data-idle-result')
      };`);
      assert.deepEqual(
        {
          ...observed,
          requests: fixture.requests.filter((request) => request.path === "/presign").length,
        },
        { timer: true, done: true, result: "/document?kind=presigned", requests: 1 },
      );
    } finally {
      await context.close();
      await fixture.close();
    }
  },
);
