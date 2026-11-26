import assert from "node:assert/strict";
import test from "node:test";
import { BrowserContext } from "./index.mjs";

test("click re-resolves a target removed after actionability without double dispatch", async () => {
  const context = new BrowserContext({ animations: "disabled" });
  try {
    const page = await context.newPage();
    await page.setContent(
      "<button id=\"target\" onclick=\"document.querySelector('output').textContent += 'x'\">Pick</button><output></output>",
    );
    const dispatch = page._sessionDispatch.bind(page);
    let attempts = 0;
    page._sessionDispatch = async (...args) => {
      if (args[2] !== "click" || ++attempts !== 1) return dispatch(...args);
      await page.evaluate(() => document.querySelector("#target").setAttribute("id", "removed"));
      try {
        return await dispatch(...args);
      } finally {
        await page.evaluate(() => document.querySelector("#removed").setAttribute("id", "target"));
      }
    };
    await page.locator("#target").click({ timeout: 2000 });
    assert.equal(attempts, 2);
    assert.equal(await page.locator("output").textContent(), "x");
  } finally {
    await context.close();
  }
});
