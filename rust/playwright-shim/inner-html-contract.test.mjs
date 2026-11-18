import assert from "node:assert/strict";
import { test } from "node:test";
import { newPage } from "./index.mjs";

test("innerHTML reads child markup rather than the matched outer element in root and frame documents", async (t) => {
  const page = newPage();
  t.after(() => page.close());
  await page.setContent(
    `<section id="box"><b>Root &amp; child</b><!--root--><br></section><iframe name="child" srcdoc="<section id='box'><i>Frame child</i><!--frame--></section>"></iframe>`,
  );
  const root = page.locator("#box");
  assert.equal(await root.innerHTML(), "<b>Root &amp; child</b><!--root--><br>");
  assert.equal(
    await root.evaluate((element) => element.outerHTML),
    '<section id="box"><b>Root &amp; child</b><!--root--><br></section>',
  );
  assert.equal(await page.locator("#absent").innerHTML(), null);
  const child = page.frame({ name: "child" });
  assert.ok(child);
  assert.equal(await child.innerHTML("#box"), "<i>Frame child</i><!--frame-->");
  await root.evaluate((element) => {
    element.innerHTML = "<em>Updated</em>";
  });
  assert.equal(await root.innerHTML(), "<em>Updated</em>");
  assert.equal(await child.innerHTML("#box"), "<i>Frame child</i><!--frame-->");
  assert.equal(await page.locator("#box").textContent(), "Updated");
});
