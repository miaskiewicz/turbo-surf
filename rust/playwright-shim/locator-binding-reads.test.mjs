import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(
      '<div data-test-id="time-field"><input id="time" type="time" value="09:00"></div><div data-test-id="shift-field"><button id="shift" role="combobox">Day</button></div><div id="hidden" style="display:none">Hidden</div><iframe srcdoc="&lt;input id=child value=frame&gt;"></iframe>',
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage({ testIdAttribute: "data-test-id" });
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page._ensureLive();
    await run(page);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

function gate() {
  let resolve;
  const promise = new Promise((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

async function beforeRelease(read, held) {
  let timeout;
  try {
    return await Promise.race([
      read,
      new Promise((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error("Locator read waited for unrelated host work")),
          2000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timeout);
    held.resolve("released");
    await read.catch(() => {});
  }
}

test(
  "synchronous locator reads finish while an unrelated host callback is held",
  { timeout: 15000 },
  async () =>
    fixture(async (page) => {
      const held = gate();
      await page.exposeFunction("backgroundHostWork", () => held.promise);
      page._readEngine("void globalThis.backgroundHostWork(); return null;");
      const reading = (async () => {
        await page
          .getByTestId("time-field")
          .locator("input")
          ._waitActionable("fill", { timeout: 2000 });
        await page
          .getByTestId("shift-field")
          .locator('[role="combobox"]')
          .first()
          ._waitActionable("click", { timeout: 2000 });
        return {
          value: await page.locator("#time").inputValue(),
          visible: await page.locator("#shift").isVisible(),
          hidden: await page.locator("#hidden").isVisible(),
          display: await page.locator("#time").cssValue("display"),
        };
      })();
      const result = await beforeRelease(reading, held);
      assert.equal(result.value, "09:00");
      assert.equal(result.visible, true);
      assert.equal(result.hidden, false);
      assert.notEqual(result.display, "none");
    }),
);

test(
  "a caller awaiting its own host callback still waits for its actual result",
  { timeout: 15000 },
  async () =>
    fixture(async (page) => {
      const held = gate(),
        entered = gate();
      await page.exposeFunction("ownedHostWork", () => {
        entered.resolve();
        return held.promise;
      });
      let finished = false;
      const result = page
        .locator("#time")
        .evaluate(async () => {
          const reply = await globalThis.ownedHostWork();
          return reply;
        })
        .then((value) => {
          finished = true;
          return value;
        });
      try {
        await Promise.race([entered.promise, result]);
        assert.equal(finished, false);
        held.resolve("owned-result");
        assert.equal(await result, "owned-result");
      } finally {
        held.resolve("cleanup");
        await result.catch(() => {});
      }
    }),
);

test("synchronous locator read errors retain their JavaScript type", { timeout: 15000 }, async () =>
  fixture(async (page) => {
    await page.exposeFunction("unusedHostWork", () => null);
    page._readEngine(
      "Object.defineProperty(document.querySelector('#time'),'value',{get(){throw new RangeError('locator-read-fixture')}});return null;",
    );
    await assert.rejects(page.locator("#time").inputValue(), {
      name: "RangeError",
      message: "locator-read-fixture",
    });
  }),
);

test(
  "frame locator reads keep their realm while unrelated host work is held",
  { timeout: 15000 },
  async () =>
    fixture(async (page) => {
      const held = gate();
      await page.exposeFunction("backgroundFrameWork", () => held.promise);
      const frame = page.frames().find((entry) => entry !== page.mainFrame());
      assert.ok(frame);
      page._readEngine("void globalThis.backgroundFrameWork(); return null;");
      assert.equal(await beforeRelease(frame.locator("#child").inputValue(), held), "frame");
    }),
);

test(
  "actionability still waits for a disabled control to be enabled",
  { timeout: 15000 },
  async () =>
    fixture(async (page) => {
      const held = gate(),
        entered = gate();
      await page.exposeFunction("enableControl", () => {
        entered.resolve();
        return held.promise;
      });
      page._readEngine(
        "const button=document.querySelector('#shift');button.disabled=true;button.onclick=()=>button.textContent='clicked';void globalThis.enableControl().then(()=>{button.disabled=false});return null;",
      );
      let completed = false;
      const clicking = page
        .locator("#shift")
        .click({ timeout: 2000 })
        .then(() => {
          completed = true;
        });
      try {
        await Promise.race([entered.promise, clicking]);
        assert.equal(completed, false);
        assert.equal(page._readEngine("return document.querySelector('#shift').disabled"), true);
        held.resolve();
        await clicking;
        assert.equal(await page.locator("#shift").textContent(), "clicked");
      } finally {
        held.resolve();
        await clicking.catch(() => {});
      }
    }),
);
