import assert from "node:assert/strict";
import test from "node:test";
import { newPage } from "./index.mjs";

test("typing preserves a live editor's character query until its timeout", async () => {
  const page = newPage();
  try {
    await page.setContent('<input id="date-section">');
    await page.evaluate(() => {
      window.query = "";
      window.expired = 0;
      let timer;
      document.querySelector("input").addEventListener("input", (event) => {
        clearTimeout(timer);
        window.query += event.data;
        timer = setTimeout(() => {
          window.query = "";
          window.expired++;
        }, 5000);
      });
    });
    await page.locator("input").focus();
    await page.keyboard.type("11", { delay: 25 });
    assert.deepEqual(await page.evaluate(() => [window.query, window.expired]), ["11", 0]);
  } finally {
    await page.close();
  }
});

test("live editor timers still run after their wall-clock deadline", async () => {
  const page = newPage();
  try {
    await page.setContent("<body>");
    await page.evaluate(() => {
      window.expired = false;
      setTimeout(() => {
        window.expired = true;
      }, 40);
    });
    assert.equal(await page.evaluate(() => window.expired), false);
    await page.waitForFunction(() => window.expired);
    assert.equal(await page.evaluate(() => window.expired), true);
  } finally {
    await page.close();
  }
});
