import assert from "node:assert/strict";
import test from "node:test";
import { newPage } from "./index.mjs";

async function withInput(run) {
  const page = newPage();
  try {
    await page.setContent('<textarea id="input"></textarea>');
    await page.locator("#input").focus();
    await run(page);
  } finally {
    await page.close();
  }
}

test("keyboard emits Chromium legacy Enter and printable keypress fields in order", async () => {
  await withInput(async (page) => {
    await page.evaluate(() => {
      window.events = [];
      for (const type of ["keydown", "keypress", "keyup"])
        document.addEventListener(type, (event) =>
          events.push([event.type, event.key, event.charCode, event.keyCode, event.which]),
        );
    });
    await page.keyboard.press("Enter");
    await page.keyboard.press("a");
    assert.deepEqual(await page.evaluate(() => events), [
      ["keydown", "Enter", 0, 13, 13],
      ["keypress", "Enter", 13, 13, 13],
      ["keyup", "Enter", 0, 13, 13],
      ["keydown", "a", 0, 65, 65],
      ["keypress", "a", 97, 97, 97],
      ["keyup", "a", 0, 65, 65],
    ]);
  });
});

test("legacy React keypress filtering admits Enter and printable characters", async () => {
  await withInput(async (page) => {
    await page.evaluate(() => {
      window.accepted = [];
      document.addEventListener("keypress", (event) => {
        // React's legacy onKeyPress filters nonprinting charCodes except Enter.
        if (event.charCode >= 32 || event.charCode === 13) {
          accepted.push(event.key);
          if (event.key === "Enter") event.preventDefault();
        }
      });
    });
    await page.keyboard.press("a");
    await page.keyboard.press("Enter");
    assert.deepEqual(await page.evaluate(() => accepted), ["a", "Enter"]);
    assert.equal(await page.locator("#input").inputValue(), "a");
  });
});

for (const canceledType of ["keydown", "keypress"]) {
  test(`${canceledType} cancellation suppresses typing while preserving keyup`, async () => {
    await withInput(async (page) => {
      await page.evaluate((type) => {
        window.events = [];
        for (const name of ["keydown", "keypress", "keyup"])
          document.addEventListener(name, (event) => {
            events.push(event.type);
            if (event.type === type) event.preventDefault();
          });
      }, canceledType);
      await page.keyboard.press("a");
      assert.equal(await page.locator("#input").inputValue(), "");
      assert.deepEqual(
        await page.evaluate(() => events),
        canceledType === "keydown" ? ["keydown", "keyup"] : ["keydown", "keypress", "keyup"],
      );
    });
  });
}
