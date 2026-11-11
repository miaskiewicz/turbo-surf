import assert from "node:assert/strict";
import test from "node:test";
import { newPage } from "./index.mjs";

test("Locator.evaluate uses indexed live nodes and persists page effects", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<button id="one">one</button><button id="two">two</button><p id="state">old</p><script>window.probeState=17; document.querySelector("#two").onclick=()=>document.querySelector("#state").textContent="clicked";</script>',
    );
    await page._ensureLive();
    assert.equal(
      await page
        .locator("button")
        .nth(1)
        .evaluate((node) => node.id),
      "two",
    );
    assert.equal(await page.locator("#two").evaluate(() => window.probeState), 17);
    await page.locator("#two").evaluate((node, value) => {
      node.setAttribute("data-probe", value);
      node.click();
    }, "yes");
    assert.equal(await page.locator("#two").getAttribute("data-probe"), "yes");
    assert.equal(await page.locator("#state").textContent(), "clicked");
  } finally {
    await page.close();
  }
});
