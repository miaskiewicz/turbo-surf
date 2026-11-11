import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("locator press dispatches key events on its focused element", async () => {
  const page = shim.newPage();
  try {
    await page.setContent(
      "<input id=\"field\" onkeydown=\"document.querySelector('output').textContent+=event.key+':'\"><output></output>",
    );
    await page._ensureLive();
    await page.locator("#field").press("Enter");
    await page.locator("#field").press("Escape");
    assert.equal(
      await page.evaluate(() => document.querySelector("output").textContent),
      "Enter:Escape:",
    );
    assert.equal(await page.evaluate(() => document.activeElement.id), "field");
  } finally {
    await page.close();
  }
});
