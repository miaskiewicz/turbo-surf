import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("Locator.page synchronously preserves direct and derived owner identity without resolving DOM", async () => {
  const page = shim.newPage();
  const other = shim.newPage();
  try {
    const base = page.locator("#missing");
    const derived = [
      base,
      base.first(),
      base.last(),
      base.nth(2),
      base.filter({ hasText: "date" }),
      base.locator("input"),
      base.and(base),
      base.or(base),
      page.getByRole("textbox"),
      page.getByTestId("date"),
    ];
    for (const locator of derived) {
      assert.equal(locator.page(), page);
      assert.equal(locator.page().keyboard, page.keyboard);
    }
    assert.equal(other.locator("#missing").page(), other);
    assert.equal(page._session, null, "owner lookup must not open a live DOM session");
  } finally {
    await page.close();
    await other.close();
  }
});

test("frame and nested-frame locator owners are their top-level Page", async () => {
  const page = shim.newPage();
  try {
    const child =
      '<input id=frame-date><iframe name=inner srcdoc="<input id=inner-date>"></iframe>';
    await page.setContent(
      `<iframe id=outer name=outer srcdoc="${child.replaceAll('"', "&quot;")}"></iframe>`,
    );
    const frame = page.frame({ name: "outer" });
    const inner = page.frame({ name: "inner" });
    assert.ok(frame);
    assert.ok(inner);
    const locators = [
      frame.locator("input"),
      frame.getByRole("textbox").first(),
      frame.locator("input").filter({ visible: true }),
      page.frameLocator("#outer").locator("input"),
      page.frameLocator("#outer").frameLocator("iframe").locator("#inner-date"),
      inner.locator("#inner-date").nth(0),
    ];
    for (const locator of locators) {
      assert.equal(locator.page(), page);
      assert.equal(locator.page().keyboard, page.keyboard);
      assert.notEqual(locator.page(), frame);
      assert.notEqual(locator.page(), inner);
    }
    await page.close();
    assert.equal(locators[0].page(), page);
  } finally {
    await page.close();
  }
});

test("the payroll date-helper owning-page keyboard primitive edits the selected field", async () => {
  const page = shim.newPage();
  try {
    await page.setContent('<input data-testid="date" value="old"><input id="other" value="keep">');
    const locator = page.getByTestId("date");
    // The unchanged fillMuiDateSections helper obtains keyboard from locator.page().
    const { keyboard } = locator.page();
    await locator.click();
    await keyboard.press("ControlOrMeta+a");
    await keyboard.type("08112026");
    await locator.blur();
    assert.equal(await locator.inputValue(), "08112026");
    assert.equal(await page.locator("#other").inputValue(), "keep");
  } finally {
    await page.close();
  }
});
