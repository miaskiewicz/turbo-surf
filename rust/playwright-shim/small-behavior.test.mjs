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

test("isChecked validates native and ARIA types and reads live checked state", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<input id="native" type="checkbox" checked><div id="aria" role="checkbox" aria-checked="true"></div><div id="plain" checked></div><input id="text" checked>',
    );
    assert.equal(await page.locator("#native").isChecked(), true);
    assert.equal(await page.locator("#aria").isChecked(), true);
    await assert.rejects(
      () => page.locator("#plain").isChecked(),
      /Not a checkbox or radio button/,
    );
    await assert.rejects(() => page.locator("#text").isChecked(), /Not a checkbox or radio button/);
    await page._ensureLive();
    await page._sessionEval(
      "document.querySelector('#native').checked = false; document.querySelector('#aria').setAttribute('aria-checked', 'mixed');",
    );
    assert.equal(await page.locator("#native").isChecked(), false);
    assert.equal(await page.locator("#aria").isChecked(), false);
  } finally {
    await page.close();
  }
});
