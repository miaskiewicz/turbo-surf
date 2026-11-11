import assert from "node:assert/strict";
import test from "node:test";
import { expect, newPage } from "./index.mjs";

test("toBeEmpty uses control values and descendant text, including live dirty values", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<input id="input" value="present"><textarea id="textarea">present</textarea><div id="nested"><span></span></div><div id="spaces">  \n </div>',
    );
    await expect(page.locator("#input")).not.toBeEmpty({ timeout: 0 });
    await expect(page.locator("#textarea")).not.toBeEmpty({ timeout: 0 });
    await expect(page.locator("#nested")).toBeEmpty({ timeout: 0 });
    await expect(page.locator("#spaces")).toBeEmpty({ timeout: 0 });
    await page._ensureLive();
    await page._sessionEval(
      "document.querySelector('#input').value = ''; document.querySelector('#textarea').value = '';",
    );
    await expect(page.locator("#input")).toBeEmpty({ timeout: 0 });
    await expect(page.locator("#textarea")).toBeEmpty({ timeout: 0 });
  } finally {
    await page.close();
  }
});
