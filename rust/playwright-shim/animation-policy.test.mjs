import assert from "node:assert/strict";
import test from "node:test";

process.env.TURBO_SURF_ALLOWED_ORIGINS = "[]";
const { BrowserContext, chromium } = await import("./index.mjs");
const HTML = `<!doctype html><style>@keyframes spin{to{transform:rotate(90deg)}}
#target{width:40px;height:20px;transform:none;animation:spin 1s infinite}</style><div id=target></div>`;

test("animation presentation rejects unknown context modes", () => {
  assert.throws(() => new BrowserContext({ animations: "fast-forward" }), /animation/i);
});

test("context animation presentation reaches native geometry and preserves WAAPI", async () => {
  const context = new BrowserContext({ animations: "disabled" });
  try {
    const page = await context.newPage();
    await page.setContent(HTML);
    assert.deepEqual(
      await page.evaluate(() => {
        const target = document.getElementById("target");
        return [
          target.getBoundingClientRect().width,
          getComputedStyle(target).transform,
          typeof target.animate,
        ];
      }),
      [40, "none", "function"],
    );
  } finally {
    await context.close();
  }
});

test("browser newPage accepts an independent animation presentation choice", async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ animations: "disabled" });
    await page.setContent(HTML);
    assert.equal(await page.locator("#target").isVisible(), true);
    const enabled = await browser.newPage({ animations: "enabled" });
    await enabled.setContent(HTML);
    await assert.rejects(
      enabled.evaluate(() => document.getElementById("target").getBoundingClientRect().width),
      /animation resource/,
    );
    assert.equal(await page.locator("#target").isVisible(), true);
  } finally {
    await browser.close();
  }
});
