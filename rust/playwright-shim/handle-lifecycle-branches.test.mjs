import assert from "node:assert/strict";
import test from "node:test";
let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM ?? "./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("string handle evaluation retains actual values and closed-page disposal stays safe", async () => {
  const page = shim.newPage();
  let handle;
  try {
    await page.setContent("<p>handle lifecycle</p>");
    handle = await page.evaluateHandle("({ answer: 42 })");
    assert.deepEqual(await handle.jsonValue(), { answer: 42 });
    await page.close();
    await assert.rejects(page.evaluateHandle("({ answer: 99 })"), /closed/);
    await assert.rejects(handle.jsonValue(), /destroyed|closed/);
    await handle.dispose();
    await handle.dispose();
  } finally {
    await handle?.dispose();
    await page.close();
  }
});

test("zero-timeout handle fill preserves its value until actual readonly recovery", async () => {
  const page = shim.newPage();
  let input;
  try {
    await page.setContent('<input id="field" readonly value="before">');
    input = await page.locator("#field").elementHandle();
    await page.evaluate(() => {
      globalThis.inputChanges = 0;
      const input = document.getElementById("field");
      input.addEventListener("input", () => inputChanges++);
      setTimeout(() => {
        input.readOnly = false;
      }, 25);
    });
    assert.equal(await input.inputValue(), "before");
    await input.fill("after", { timeout: 0 });
    assert.equal(await input.inputValue(), "after");
    assert.equal(await page.evaluate(() => inputChanges), 1);
  } finally {
    await input?.dispose();
    await page.close();
  }
});

test("frame-owned handle press uses its own focused document rather than root keyboard focus", async () => {
  const page = shim.newPage();
  let input;
  try {
    await page.setContent(
      '<input id="root" value="unchanged"><iframe name="child" srcdoc="&lt;input id=child value=before&gt;"></iframe>',
    );
    const child = page.frame({ name: "child" });
    assert.ok(child);
    input = await child.locator("#child").elementHandle();
    await input.fill("child");
    await input.press("End");
    await input.press("!");
    assert.equal(await input.inputValue(), "child!");
    assert.equal(await child.evaluate(() => document.activeElement.id), "child");
    assert.equal(await page.locator("#root").inputValue(), "unchanged");
  } finally {
    await input?.dispose();
    await page.close();
  }
});
