import assert from "node:assert/strict";
import test, { after } from "node:test";
import { createPopupFixture } from "./fixtures/popup-app.mjs";

const fixture = await createPopupFixture();
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([fixture.origin]);
let BrowserContext;
try {
  ({ BrowserContext } = await import("./index.mjs"));
} catch (error) {
  await fixture.close();
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
after(() => fixture.close());
const observed = (promise) => {
  promise.catch(() => {});
  return promise;
};

test("popup bootstrap inherits only context scripts and bindings with actual callback ownership", async () => {
  const context = new BrowserContext();
  let child;
  try {
    await context.addInitScript(() => {
      globalThis.contextInitialization = true;
    });
    await context.exposeBinding("whichPage", ({ page }) => (page === child ? "child" : "opener"));
    const page = await context.newPage();
    await page.addInitScript(() => {
      globalThis.openerOnlyInitialization = true;
    });
    await page.exposeFunction("openerOnlyBinding", () => "opener only");
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await context.addInitScript(() => {
      globalThis.lateContextInitialization = true;
    });
    await context.exposeBinding("lateWhichPage", ({ page: owner }) =>
      owner === child ? "child" : "opener",
    );
    assert.equal(
      await page.evaluate(() => typeof globalThis.lateContextInitialization),
      "undefined",
    );
    const created = observed(page.waitForEvent("popup", { timeout: 2000 }));
    await page.click("#named");
    child = await created;
    await child.waitForURL(`${fixture.origin}/document?kind=named`, { timeout: 2000 });
    assert.deepEqual(
      await child.evaluate(async () => ({
        context: globalThis.contextInitialization,
        pageScript: typeof globalThis.openerOnlyInitialization,
        pageBinding: typeof globalThis.openerOnlyBinding,
        owner: await globalThis.whichPage(),
      })),
      { context: true, pageScript: "undefined", pageBinding: "undefined", owner: "child" },
    );
    assert.equal(await page.evaluate(() => globalThis.openerOnlyInitialization), true);
    await page.reload({ waitUntil: "load" });
    assert.deepEqual(
      await page.evaluate(async () => ({
        page: globalThis.openerOnlyInitialization,
        context: globalThis.lateContextInitialization,
        binding: await globalThis.openerOnlyBinding(),
      })),
      { page: true, context: true, binding: "opener only" },
    );
    assert.equal(child.isClosed(), false);
  } finally {
    await context.close();
  }
});

test("closing the opener leaves its native actor's child usable for requests and navigation", async () => {
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const created = observed(page.waitForEvent("popup", { timeout: 2000 }));
    await page.locator("#named").click();
    const child = await created;
    await child.waitForURL(`${fixture.origin}/document?kind=named`, { timeout: 2000 });
    await page.close();
    assert.equal(child.isClosed(), false);
    assert.equal(await child.opener(), null);
    assert.equal(
      await child.evaluate(() => fetch("/presign").then((response) => response.text())),
      "/document?kind=presigned",
    );
    await child.waitForLoadState("networkidle", { timeout: 2000 });
    const response = await child.goto(`${fixture.origin}/document?kind=after-opener`, {
      waitUntil: "networkidle",
      timeout: 2000,
    });
    assert.equal(response.status(), 200);
    assert.equal(response.frame().page(), child);
    assert.equal(await child.title(), "Receipt after-opener");
    assert.deepEqual(context.pages(), [child]);
  } finally {
    await context.close();
  }
});

test("a genuine popup created before root liveOpen completes receives one owned facade", async () => {
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    await page.addInitScript((url) => {
      window.open(url, "initial-receipt");
    }, `${fixture.origin}/document?kind=initial`);
    const created = observed(
      context.waitForEvent("page", { predicate: (target) => target !== page, timeout: 2000 }),
    );
    const navigation = observed(page.goto(fixture.origin, { waitUntil: "networkidle" }));
    const child = await created;
    assert.notEqual(child, page);
    assert.equal(child.context(), context);
    await navigation;
    await child.waitForURL(`${fixture.origin}/document?kind=initial`, { timeout: 2000 });
    assert.equal(await child.title(), "Receipt initial");
    assert.equal(context.pages().length, 2);
    assert.equal(await page.title(), "Opener");
  } finally {
    await context.close();
  }
});

test("configured native popup budgets deny allocation without inventing Pages", async () => {
  const context = new BrowserContext({ auxiliaryPolicy: { maxPages: 1, maxCreations: 2 } });
  try {
    const page = await context.newPage();
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    let creations = 0;
    context.on("page", () => {
      creations++;
    });
    assert.deepEqual(
      await page.evaluate(() => ({
        first: window.open("about:blank", "first") !== null,
        full: window.open("about:blank", "second") !== null,
      })),
      { first: true, full: false },
    );
    assert.equal(context.pages().length, 2);
    await context
      .pages()
      .find((target) => target !== page)
      .close();
    assert.equal(await page.evaluate(() => window.open("about:blank", "second") !== null), true);
    await context
      .pages()
      .find((target) => target !== page)
      .close();
    assert.equal(await page.evaluate(() => window.open("about:blank", "third") === null), true);
    assert.equal(context.pages().length, 1);
    assert.equal(creations, 2);
  } finally {
    await context.close();
  }
});
