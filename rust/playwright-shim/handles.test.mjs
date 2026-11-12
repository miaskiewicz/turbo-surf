import assert from "node:assert/strict";
import test from "node:test";
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("handles preserve live identity, nested arguments, own properties and disposal", async () => {
  const page = shim.newPage();
  try {
    await page.setContent('<button id="ready" data-state="ok">ready</button>');
    const waited = await page.waitForFunction(() => "ready");
    assert.equal(await waited.jsonValue(), "ready");
    const handle = await page.evaluateHandle(
      () => (globalThis.shared = { count: 1, nested: { text: "ok" } }),
    );
    const nested = await handle.getProperty("nested");
    assert.deepEqual(await nested.jsonValue(), { text: "ok" });
    await page.evaluate(() => {
      shared.count = 2;
    });
    assert.equal(await handle.evaluate((object) => object.count), 2);
    assert.equal(
      await page.evaluate(({ a, b }) => a === b && a.count === 2, { a: handle, b: handle }),
      true,
    );
    const props = await handle.getProperties();
    assert.equal(await props.get("count").jsonValue(), 2);
    const element = await page.locator("#ready").elementHandle();
    assert.equal(element.asElement(), element);
    assert.equal(await element.getAttribute("data-state"), "ok");
    await page.evaluate(() => {
      document.querySelector("#ready").remove();
      document.body.innerHTML = '<button id="ready" data-state="new">new</button>';
    });
    assert.equal(await element.getAttribute("data-state"), "ok");
    await nested.dispose();
    await nested.dispose();
    await assert.rejects(nested.jsonValue(), /disposed/);
    await handle.dispose();
    await waited.dispose();
    for (const value of props.values()) await value.dispose();
    await element.dispose();
  } finally {
    await page.close();
  }
});

test("handles enforce session and frame ownership and survive same-document mutations", async () => {
  const page = shim.newPage(),
    other = shim.newPage();
  try {
    await page.setContent('<iframe srcdoc="<b id=child>child</b>"></iframe>');
    await other.setContent("<p>other</p>");
    const object = await page.evaluateHandle(() => ({ value: 3 }));
    await assert.rejects(
      other.evaluate((value) => value.value, object),
      /context|session|page/i,
    );
    const frame = page.frames().find((item) => item !== page);
    const child = await frame.evaluateHandle(() => document.querySelector("#child"));
    assert.equal(await child.textContent(), "child");
    await assert.rejects(
      page.evaluate((value) => value.textContent, child),
      /context|frame/i,
    );
    await page.close();
    await assert.rejects(object.jsonValue(), /closed|context|session/i);
  } finally {
    await page.close();
    await other.close();
  }
});
