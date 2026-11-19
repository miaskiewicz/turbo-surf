import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(
    process.env.TURBO_SURF_SHADOW_ORACLE === "chromium" ? "playwright/test" : "./index.mjs"
  );
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

const html = `<!doctype html><div id="host"></div><div id="other"></div>
<button id="light">Light</button><script>
const root = document.querySelector('#host').attachShadow({mode:'open'});
root.innerHTML = '<button id="shadow-button" data-testid="shadow-id">Shadow button</button><label>Name<input id="name"></label><div id="nested"></div>';
root.querySelector('#nested').attachShadow({mode:'open'}).innerHTML = '<button class="deep">Deep</button>';
const closed = document.querySelector('#other').attachShadow({mode:'closed'});
closed.innerHTML = '<button id="secret">Secret</button>';
root.querySelector('button').addEventListener('click', () => document.body.dataset.clicked = 'shadow');
root.querySelector('.deep')?.addEventListener('click', () => document.body.dataset.clicked = 'wrong');
</script>`;

async function fixture(run) {
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(html);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const browser = await shim.chromium.launch(
    process.env.TURBO_SURF_SHADOW_ORACLE === "chromium" &&
      process.env.TURBO_SURF_SHADOW_ORACLE_EXECUTABLE
      ? { executablePath: process.env.TURBO_SURF_SHADOW_ORACLE_EXECUTABLE }
      : {},
  );
  try {
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
    await run(page);
  } finally {
    await browser.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("CSS and getBy locate the same actual open-shadow button", { timeout: 15000 }, async () => {
  await fixture(async (page) => {
    assert.equal(
      await page.evaluate(
        () => !!document.querySelector("#host").shadowRoot.querySelector("button"),
      ),
      true,
    );
    assert.equal(await page.locator("#shadow-button").count(), 1);
    assert.equal(await page.getByTestId("shadow-id").count(), 1);
    assert.equal(await page.getByRole("button", { name: "Shadow button" }).count(), 1);
    assert.equal(await page.getByText("Shadow button", { exact: true }).count(), 1);
  });
});

test("open-shadow actions and element reads target the live node", { timeout: 15000 }, async () => {
  await fixture(async (page) => {
    const button = page.getByRole("button", { name: "Shadow button" });
    assert.equal(await button.count(), 1);
    await button.click();
    assert.equal(await page.evaluate(() => document.body.dataset.clicked), "shadow");
    assert.equal(await button.getAttribute("data-testid"), "shadow-id");
    await button.focus();
    await shim.expect(button).toBeFocused();
    await shim.expect(button).toHaveJSProperty("id", "shadow-button");
    assert.equal(await button.evaluate((element) => element.getRootNode().host.id), "host");
    await page.getByLabel("Name").fill("Ada");
    assert.equal(await page.locator("#name").inputValue(), "Ada");
  });
});

test(
  "nested open roots are pierced while closed roots and XPath remain opaque",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      assert.equal(await page.locator("#host").locator(".deep").count(), 1);
      assert.equal(await page.getByRole("button", { name: "Deep" }).count(), 1);
      assert.equal(await page.locator("#secret").count(), 0);
      assert.equal(await page.getByRole("button", { name: "Secret" }).count(), 0);
      assert.equal(await page.locator("xpath=//*[@id='shadow-button']").count(), 0);
      assert.equal(
        await page.evaluate(() => document.querySelectorAll("#shadow-button").length),
        0,
      );
    });
  },
);

test(
  "shadow resolution follows mutations and preserves strict matching",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      const locator = page.locator("#shadow-button");
      assert.equal(await locator.count(), 1);
      await page.evaluate(() => {
        const root = document.querySelector("#host").shadowRoot;
        root.querySelector("button").remove();
        root.innerHTML +=
          '<button id="replacement" data-testid="shadow-id">Replacement</button><button data-testid="shadow-id">Duplicate</button>';
      });
      assert.equal(await locator.count(), 0);
      assert.equal(await page.getByTestId("shadow-id").count(), 2);
      await assert.rejects(page.getByTestId("shadow-id").click(), /strict mode violation/);
      assert.equal(await page.getByTestId("shadow-id").nth(1).textContent(), "Duplicate");
    });
  },
);

test(
  "CSS ancestry and functional selectors cross only open roots",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      assert.equal(await page.locator("#host .deep").count(), 1);
      assert.equal(await page.locator("#host /* > ignored */ .\\64 eep").count(), 1);
      assert.equal(await page.locator("#host > #nested > .deep").count(), 1);
      assert.equal(await page.locator("#host:has(.deep)").count(), 1);
      assert.equal(await page.locator("button:is(.deep, #shadow-button)").count(), 2);
      assert.equal(await page.getByTestId(/^shadow-/).count(), 1);
    });
  },
);

test(
  "guest query overrides cannot hide or manufacture locator shadow targets",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      await page.evaluate(() => {
        document.querySelectorAll = () => [];
        Element.prototype.querySelectorAll = () => [];
        Element.prototype.matches = () => false;
        Object.defineProperty(Element.prototype, "shadowRoot", {
          configurable: true,
          get() {
            throw new Error("guest shadow getter");
          },
        });
      });
      assert.equal(await page.locator("#shadow-button").count(), 1);
      assert.equal(await page.getByRole("button", { name: "Shadow button" }).count(), 1);
      assert.equal(await page.locator("#secret").count(), 0);
    });
  },
);

test(
  "shadow locator helpers belong to their frame and preserve closed boundaries",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      await page.evaluate(() => {
        const frame = document.createElement("iframe");
        frame.id = "child";
        frame.srcdoc = `<div id="child-host"></div><script>document.querySelector('#child-host').attachShadow({mode:'open'}).innerHTML='<button data-testid="shadow-id">Child shadow</button>';</script>`;
        document.body.appendChild(frame);
      });
      await page.waitForFunction(
        () =>
          !!document
            .querySelector("#child")
            ?.contentDocument?.querySelector("#child-host")
            ?.shadowRoot?.querySelector("button"),
      );
      const frame = page.frameLocator("#child");
      assert.equal(await frame.getByTestId("shadow-id").count(), 1);
      assert.equal(
        await frame.getByRole("button", { name: "Child shadow" }).textContent(),
        "Child shadow",
      );
      assert.equal(await page.getByRole("button", { name: "Child shadow" }).count(), 0);
      assert.equal(await frame.locator("#shadow-button").count(), 0);
    });
  },
);

test(
  "indexed shadow matches keep earlier open roots ahead of later hosts",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      await page.evaluate(() => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        host.attachShadow({ mode: "open" }).innerHTML = "<button>Later</button>";
      });
      assert.deepEqual(await page.getByRole("button").allTextContents(), [
        "Light",
        "Shadow button",
        "Deep",
        "Later",
      ]);
      assert.equal(await page.locator("button").nth(1).textContent(), "Shadow button");
      assert.equal(await page.getByRole("button").nth(3).textContent(), "Later");
    });
  },
);

test(
  "label and accessible-name references stay inside their actual shadow tree",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      await page.evaluate(() => {
        const root = document.querySelector("#host").shadowRoot;
        root.innerHTML +=
          '<label for="field">Account</label><input id="field"><span id="caption">Tree label</span><button aria-labelledby="caption">Fallback</button>';
        const outside = document.createElement("span");
        outside.id = "caption";
        outside.textContent = "Wrong light label";
        document.body.appendChild(outside);
      });
      assert.equal(await page.getByRole("button", { name: "Tree label" }).count(), 1);
      await shim
        .expect(page.getByRole("button", { name: "Tree label" }))
        .toHaveAccessibleName("Tree label");
      assert.equal(await page.getByRole("button", { name: "Wrong light label" }).count(), 0);
      await page.getByLabel("Account", { exact: true }).fill("123");
      assert.equal(await page.locator("#field").inputValue(), "123");
    });
  },
);

test(
  "link defaults use the actual shadow target's ancestor and URL",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      await page.evaluate(() => {
        const base = document.createElement("base");
        base.href = "/folder/";
        document.head.appendChild(base);
        document.querySelector("#host").shadowRoot.innerHTML +=
          '<a href="next"><span id="link-child">Next</span></a>';
      });
      assert.equal(await page.locator("#link-child").count(), 1);
      await page.locator("#link-child").click();
      assert.equal(new URL(page.url()).pathname, "/folder/next");
    });
  },
);

test(
  "locator metadata reads do not construct application custom elements",
  { timeout: 15000 },
  async () => {
    await fixture(async (page) => {
      const before = await page.evaluate(() => {
        globalThis.metadataConstructions = 0;
        customElements.define(
          "x-metadata-control",
          class extends HTMLElement {
            constructor() {
              super();
              metadataConstructions++;
            }
          },
        );
        document.querySelector("#host").shadowRoot.innerHTML =
          '<x-metadata-control></x-metadata-control><button id="metadata-button">Metadata</button>';
        return metadataConstructions;
      });
      assert.equal(before, 1);
      assert.equal(await page.locator("#metadata-button").getAttribute("id"), "metadata-button");
      assert.equal(await page.evaluate(() => metadataConstructions), before);
    });
  },
);
