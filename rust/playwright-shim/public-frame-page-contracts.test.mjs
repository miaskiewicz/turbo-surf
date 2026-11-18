import assert from "node:assert/strict";
import { test } from "node:test";
import { expect, newPage } from "./index.mjs";

const doc = (note) =>
  `<title>${note} document</title><p id="note">${note}</p><label for="search">Find item</label><input id="search" placeholder="Search inventory"><img alt="Inventory map"><span title="Inventory tip">Tip</span><iframe name="nested" srcdoc="<p id='nested-note'>Nested note</p>"></iframe>`;
const frame = (name, text) =>
  `<iframe class="card" name="${name}" srcdoc="${doc(text).replaceAll("&", "&amp;").replaceAll('"', "&quot;")}"></iframe>`;

async function pageWith(t, html) {
  const page = newPage();
  t.after(() => page.close());
  await page.setContent(html);
  return page;
}

test("frame and frame-locator queries retain ownership, scoped accessors and nested parentage", async (t) => {
  const page = await pageWith(t, frame("first", "First note") + frame("second", "Second note"));
  const first = page.frame({ name: "first" });
  assert.ok(first);
  assert.equal(first.page(), page);
  assert.equal(page.mainFrame().parentFrame(), null);
  assert.equal(page.mainFrame().isDetached(), false);
  assert.equal(page.childFrames().length, 2);
  assert.equal(first.parentFrame(), page.mainFrame());
  assert.equal(await first.title(), "First note document");
  assert.equal(await first.innerText("#note"), "First note");
  assert.equal(await first.innerHTML("#note"), "First note");
  assert.equal(await first.textContent("#note"), "First note");
  assert.equal(first.childFrames().length, 1);
  assert.equal(first.childFrames()[0].parentFrame(), first);
  assert.equal(first.childFrames()[0].name(), "nested");
  await expect(first.getByText("First note", { exact: true })).toHaveText("First note");
  await expect(first.getByLabel("Find item", { exact: true })).toHaveAttribute("id", "search");
  await expect(first.getByPlaceholder("inventory")).toHaveAttribute("id", "search");
  await expect(first.getByAltText("Inventory map")).toHaveAttribute("alt", "Inventory map");
  await expect(first.getByTitle("Inventory tip")).toHaveText("Tip");
  await expect(first.frameLocator('iframe[name="nested"]').getByText("Nested note")).toHaveText(
    "Nested note",
  );
  const cards = page.frameLocator("iframe.card");
  await expect(cards.first().getByText("First note", { exact: true })).toHaveText("First note");
  await expect(cards.last().getByText("Second note", { exact: true })).toHaveText("Second note");
  await expect(cards.first().getByLabel("Find item")).toHaveAttribute("id", "search");
  await expect(cards.first().getByPlaceholder("inventory")).toHaveAttribute("id", "search");
  await expect(cards.first().getByAltText("Inventory map")).toHaveAttribute("alt", "Inventory map");
  await expect(cards.first().getByTitle("Inventory tip")).toHaveText("Tip");
  await expect(
    cards.first().frameLocator('iframe[name="nested"]').getByText("Nested note"),
  ).toHaveText("Nested note");
  assert.throws(() => cards.nth(9).getByText("missing"), /matched no iframe/);
  assert.throws(() => page.frameLocator("#missing").getByLabel("Find item"), /matched no iframe/);
  await assert.rejects(
    expect(first.getByText("First note", { exact: true })).toHaveText("not present", {
      timeout: 50,
    }),
    /expected text/,
  );
  await page.setContent(frame("replacement", "Fresh note"));
  await assert.rejects(first.content(), /frame is detached/);
  await expect(cards.first().getByText("Fresh note", { exact: true })).toHaveText("Fresh note");
});

test("locator descriptions, ARIA, collection text and class assertions distinguish real mismatches", async (t) => {
  const page = await pageWith(
    t,
    `<h1>Inventory</h1><a href="https://inventory.test/manual">Manual</a><button id="save" aria-describedby="hint">Save</button><p id="hint">Changes are final</p><ul><li class="row active"> First&nbsp; item </li><li class="row">Second item</li><li class="row active">Third item</li></ul>`,
  );
  assert.deepEqual(await page.links(), ["https://inventory.test/manual"]);
  const save = page.locator("#save");
  assert.equal(await save.accessibleDescription(), "Changes are final");
  assert.equal(await page.locator("#missing").accessibleDescription(), "");
  await expect(save).toHaveAccessibleDescription(/Changes are final/);
  await expect(save).not.toHaveAccessibleDescription("Unrelated");
  await assert.rejects(
    expect(save).toHaveAccessibleDescription("Unrelated"),
    /expected accessible description/,
  );
  assert.match(await save.ariaSnapshot(), /button.*Save/);
  assert.equal(await page.locator("#missing").ariaSnapshot(), "");
  assert.match(await page.ariaSnapshot(), /Inventory/);
  await save.evaluate((button) => {
    button.inventoryState = { saved: false, ids: [1, 2] };
    button.optionalState = undefined;
  });
  await expect(save).toHaveJSProperty("inventoryState", { saved: false, ids: [1, 2] });
  await expect(save).toHaveJSProperty("optionalState", undefined);
  await expect(save).not.toHaveJSProperty("inventoryState", "wrong type", { timeout: 0 });
  await assert.rejects(
    expect(page.locator("#missing")).toHaveJSProperty("optionalState", undefined, { timeout: 0 }),
    /expected property optionalState/,
  );
  const rows = page.locator("li");
  await expect(rows).toHaveText([/First\s+item/g, "Second item", "Third item"]);
  await expect(rows).toContainText(["First item", /Third/]);
  await expect(rows).not.toContainText(["Third item", "First item"]);
  await expect(rows).toHaveClass(["active row", /row/g, "row active"]);
  await expect(rows).toContainClass(["row", "row", "active"]);
  await expect(rows).not.toHaveClass(["row", "row", "row"]);
  await expect(rows.first()).toHaveAttribute("class", { timeout: 0 });
  await assert.rejects(expect(rows.first()).toHaveId(/^$/, { timeout: 0 }), /expected id/);
  await assert.rejects(expect(save).toBeInViewport({ ratio: -1 }), /ratio must be between/);
});

test("page action shortcuts mutate native controls and deliver actual pointer and file events", async (t) => {
  const page = await pageWith(
    t,
    `<input id="name"><input id="check" type="checkbox"><input id="locked" readonly><select id="items" multiple><option value="a">Alpha</option><option value="b">Beta</option></select><input id="file" type="file"><button id="action">Act</button>`,
  );
  await page.evaluate(() => {
    globalThis.actionEvents = [];
    const button = document.querySelector("#action");
    for (const type of ["mouseover", "click", "dblclick"])
      button.addEventListener(type, () => actionEvents.push(type));
    document.querySelector("#file").addEventListener("change", () => {
      globalThis.fileChanged = true;
    });
  });
  await page.fill("#name", "Ada");
  assert.equal(await page.isEditable("#name"), true);
  assert.equal(await page.isEditable("#locked"), false);
  assert.equal(await page.isChecked("#check"), false);
  await page.check("#check");
  assert.equal(await page.isChecked("#check"), true);
  await page.uncheck("#check");
  await page.setChecked("#check", true);
  assert.equal(await page.isChecked("#check"), true);
  assert.deepEqual(await page.selectOption("#items", [{ label: "Beta" }, { index: 0 }]), [
    "a",
    "b",
  ]);
  await expect(page.locator("#items")).toHaveValues(["a", "b"]);
  await assert.rejects(
    page.selectOption("#items", "missing", { timeout: 0 }),
    /Options did not match/,
  );
  await page.focus("#name");
  await expect(page.locator("#name")).toBeFocused();
  await page.hover("#action");
  await page.dblclick("#action");
  await page.tap("#action");
  assert.deepEqual(
    await page.evaluate(() => ({
      hover: actionEvents.includes("mouseover"),
      clicks: actionEvents.filter((type) => type === "click").length,
      doubles: actionEvents.filter((type) => type === "dblclick").length,
    })),
    { hover: true, clicks: 3, doubles: 1 },
  );
  await page.setInputFiles("#file", {
    name: "report.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("report"),
  });
  assert.deepEqual(
    await page
      .locator("#file")
      .evaluate((input) =>
        Array.from(input.files, (file) => ({ name: file.name, type: file.type, size: file.size })),
      ),
    [{ name: "report.txt", type: "text/plain", size: 6 }],
  );
  assert.equal(await page.evaluate(() => fileChanged), true);
  await page.setInputFiles("#file", []);
  assert.equal(await page.locator("#file").evaluate((input) => input.files.length), 0);
});
