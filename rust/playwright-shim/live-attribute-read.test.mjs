import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("live attributes: native addon not built, skipping");
  process.exit(0);
}
async function fixture(run) {
  const html = `<!doctype html><p id="light" data-value="light"></p><div id="host"></div>
    <i data-many data-value="first"></i><i data-many data-value="second"></i>
    <script>document.querySelector('#host').attachShadow({mode:'open'}).innerHTML='<span data-testid="inside" data-value="shadow"></span>';</script>`;
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(html);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const context = new shim.BrowserContext({ testIdAttribute: "data-testid" });
  try {
    const page = await context.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    assert.equal(await page.evaluate(() => !!document.querySelector("#host").shadowRoot), true);
    await run(page);
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("live attribute reads do not serialize light or shadow metadata", async () => {
  await fixture(async (page) => {
    const read = page._readEngine;
    let metadataCopies = 0;
    page._readEngine = function (source) {
      if (source.includes("tcLocatorSnapshots") || source.includes("cloneNode")) {
        metadataCopies++;
        throw new Error("unexpected locator metadata copy");
      }
      return Reflect.apply(read, this, [source]);
    };
    assert.equal(await page.locator("#light").getAttribute("data-value"), "light");
    assert.equal(await page.getByTestId("inside").getAttribute("data-value"), "shadow");
    assert.equal(await page.locator("#host").getByTestId("inside").getAttribute("missing"), null);
    assert.equal(metadataCopies, 0);
  });
});

test("live attribute reads retain native access and strict cardinality", async () => {
  await fixture(async (page) => {
    await page.evaluate(() => {
      const light = document.querySelector("#light");
      const inside = document.querySelector("#host").shadowRoot.querySelector("span");
      inside.setAttribute("data-value", "changed");
      globalThis.attributeHooks = 0;
      const poisoned = () => {
        globalThis.attributeHooks++;
        throw new Error("authored attribute hook");
      };
      Element.prototype.getAttribute = poisoned;
      light.getAttribute = poisoned;
      inside.getAttribute = poisoned;
    });
    assert.equal(await page.locator("#light").getAttribute("DATA-VALUE"), "light");
    assert.equal(await page.getByTestId("inside").getAttribute("data-value"), "changed");
    assert.equal(await page.locator("#missing").getAttribute("data-value"), null);
    assert.equal(await page.locator("[data-many]").nth(1).getAttribute("data-value"), "second");
    await assert.rejects(
      page.locator("[data-many]").getAttribute("data-value"),
      /strict mode violation/,
    );
    assert.equal(await page.evaluate(() => globalThis.attributeHooks), 0);
  });
});
