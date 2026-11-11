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

test("boundingBox returns live positioned geometry and null for hidden elements", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<style>body{margin:0}#box{position:absolute;left:40px;top:120px;width:80px;height:30px}</style><div id="box"></div><div id="hidden" hidden></div>',
    );
    assert.deepEqual(await page.locator("#box").boundingBox(), {
      x: 40,
      y: 120,
      width: 80,
      height: 30,
    });
    assert.equal(await page.locator("#hidden").boundingBox(), null);
    await page._sessionEval("document.querySelector('#box').style.left = '60px'");
    assert.equal((await page.locator("#box").boundingBox()).x, 60);
  } finally {
    await page.close();
  }
});
