import assert from "node:assert/strict";
import { test } from "node:test";
import { expect, newPage } from "./index.mjs";

async function pageWith(t, html) {
  const page = newPage();
  t.after(() => page.close());
  await page.setContent(html);
  return page;
}

test("indexed shadow queries follow list mutation and reject cross-frame composition without disturbing peers", async (t) => {
  const page = await pageWith(
    t,
    `<div id="host"></div><p id="healthy">Root healthy</p><iframe name="left" srcdoc="<p class='a'>Alpha</p><p class='a b'>Both</p><p class='b'>Beta</p>"></iframe><iframe name="right" srcdoc="<p>Peer healthy</p>"></iframe>`,
  );
  await page.evaluate(() => {
    document.querySelector("#host").attachShadow({ mode: "open" }).innerHTML =
      "<ul><li>One</li><li>Two</li></ul>";
  });
  const items = page.locator("li");
  assert.equal(await items.count(), 2);
  assert.equal(await items.nth(0).count(), 1);
  assert.equal(await items.nth(-1).count(), 1);
  assert.equal(await items.nth(2).count(), 0);
  assert.equal(await items.nth(-3).count(), 0);
  assert.equal(await page.locator(".absent").nth(0).count(), 0);
  await page.evaluate(() =>
    document.querySelector("#host").shadowRoot.querySelector("li").remove(),
  );
  assert.equal(await items.count(), 1);
  assert.equal(await items.nth(1).count(), 0);
  assert.equal(await items.last().textContent(), "Two");
  const left = page.frame({ name: "left" });
  const right = page.frame({ name: "right" });
  assert.ok(left);
  assert.ok(right);
  assert.deepEqual(await left.locator(".a").and(left.locator(".b")).allTextContents(), ["Both"]);
  assert.deepEqual(await left.locator(".a").or(left.locator(".b")).allTextContents(), [
    "Alpha",
    "Both",
    "Beta",
  ]);
  assert.throws(() => left.locator("p").and(right.locator("p")), /same frame/);
  assert.throws(() => left.locator("p").or(page.locator("#healthy")), /same frame/);
  assert.throws(() => left.locator("p").filter({ has: right.locator("p") }), /same frame/);
  assert.equal(await right.textContent("p"), "Peer healthy");
  assert.equal(await page.locator("#healthy").textContent(), "Root healthy");
});

test("invalid select choices preserve selection and events before valid clearing and recovery", async (t) => {
  const page = await pageWith(
    t,
    '<select id="choice"><option value="a">Alpha</option><option value="b">Beta</option></select><input id="not-select">',
  );
  await page.evaluate(() => {
    globalThis.selectionEvents = 0;
    document.querySelector("#choice").addEventListener("change", () => selectionEvents++);
  });
  const choice = page.locator("#choice");
  for (const [value, error] of [
    [{}, /requires a value, label or index/],
    [{ index: -1 }, /index must be a nonnegative integer/],
    [{ index: 0.5 }, /index must be a nonnegative integer/],
    [{ value: 42 }, /value must be a string/],
    [{ label: false }, /label must be a string/],
  ]) {
    await assert.rejects(choice.selectOption(value), error);
    assert.equal(await choice.inputValue(), "a");
    assert.equal(await page.evaluate(() => selectionEvents), 0);
  }
  await assert.rejects(page.locator("#not-select").selectOption("a"), /Element is not a select/);
  assert.equal(await choice.inputValue(), "a");
  assert.deepEqual(await choice.selectOption(null), []);
  assert.equal(await choice.inputValue(), "");
  assert.deepEqual(await choice.selectOption({ value: "b", label: "Beta", index: 1 }), ["b"]);
  assert.equal(await choice.inputValue(), "b");
  assert.equal(await page.evaluate(() => selectionEvents), 2);
});

test("missing value reads time out while other missing reads and inner-text arrays preserve their contracts", async (t) => {
  const page = await pageWith(
    t,
    '<p class="item">Visible<span hidden>Secret</span></p><input id="healthy" value="ready">',
  );
  const missing = page.locator("#absent");
  assert.equal(await missing.getAttribute("id"), null);
  assert.equal(await missing.innerText(), "");
  await assert.rejects(missing.inputValue({ timeout: 25 }), /state=attached.*timed out/);
  assert.equal(await missing.ariaRole(), null);
  assert.equal(await missing.accessibleName(), "");
  assert.equal(await missing.cssValue("color"), "");
  assert.deepEqual(await missing.selectedValues(), []);
  const items = page.locator(".item");
  await expect(items).toHaveText(["VisibleSecret"]);
  await expect(items).toHaveText(["Visible"], { useInnerText: true });
  await expect(items).toContainText(["Visible"], { useInnerText: true });
  await page.evaluate(() => document.querySelector("span").removeAttribute("hidden"));
  await expect(items).toHaveText(["VisibleSecret"], { useInnerText: true });
  assert.equal(await page.locator("#healthy").inputValue(), "ready");
});
