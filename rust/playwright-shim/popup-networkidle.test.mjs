import assert from "node:assert/strict";
import test from "node:test";
import { createPopupFixture } from "./fixtures/popup-app.mjs";
const fixture = await createPopupFixture();
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([fixture.origin]);
const { BrowserContext } = await import("./index.mjs");
test(
  "hydrated primary Page preserves networkidle on repeated navigation and reload",
  { timeout: 10000 },
  async () => {
    const context = new BrowserContext();
    try {
      const page = await context.newPage();
      await page.goto(fixture.origin, { waitUntil: "networkidle", timeout: 2000 });
      await page.evaluate(() => document.title);
      await page.goto(fixture.origin, { waitUntil: "networkidle", timeout: 2000 });
      await page.reload({ waitUntil: "networkidle", timeout: 2000 });
      assert.equal(await page.title(), "Opener");
    } finally {
      await context.close();
      await fixture.close();
    }
  },
);
