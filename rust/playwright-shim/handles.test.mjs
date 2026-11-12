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

test("handle evaluations service host bindings while awaiting guest promises", async () => {
  const page = shim.newPage();
  try {
    await page.exposeFunction("echo", (value) => value + 1);
    await page.setContent("<body>ready</body>");
    const handle = await page.evaluateHandle(() => ({ value: 3 }));
    assert.equal(await handle.evaluate(async (object) => await echo(object.value)), 4);
    await handle.dispose();
  } finally {
    await page.close();
  }
});

test("ElementHandle actions keep the retained node when microtasks reorder its siblings", async () => {
  const page = shim.newPage();
  try {
    await page.setContent("<button id=target>target</button>");
    const handle = await page.locator("#target").elementHandle();
    await page.evaluate(() => {
      globalThis.clicked = [];
      document.addEventListener("click", (event) => clicked.push(event.target.id));
      const target = document.querySelector("#target");
      Object.defineProperty(target, "isConnected", {
        configurable: true,
        get() {
          queueMicrotask(() => {
            if (document.querySelector("#other")) return;
            const other = document.createElement("button");
            other.id = "other";
            other.textContent = "other";
            document.body.prepend(other);
          });
          return true;
        },
      });
    });
    await handle.click();
    assert.deepEqual(await page.evaluate(() => clicked), ["target"]);
    assert.equal(await page.locator("#other").count(), 1);
    await page.evaluate(() => {
      delete document.querySelector("#target").isConnected;
      document.querySelector("#target").remove();
    });
    await assert.rejects(handle.click({ force: true }), /attached/);
    await handle.dispose();
  } finally {
    await page.close();
  }
});

test("frame helper capabilities stay private when the guest replaces Function", async () => {
  const page = shim.newPage();
  try {
    await page.setContent('<iframe srcdoc="<button id=child>child</button>"></iframe>');
    const frame = page.frames().find((frame) => frame !== page);
    await frame.evaluate(() => {
      const NativeFunction = Function;
      globalThis.Function = function (...source) {
        const compiled = NativeFunction(...source);
        return function (...args) {
          if (args[0]?.remoteHandleGet) globalThis.stolenRootCapabilities = args[0];
          return compiled(...args);
        };
      };
    });
    await frame.locator("#child").click();
    assert.equal(
      await frame.evaluate(() => typeof globalThis.stolenRootCapabilities?.remoteHandleGet),
      "undefined",
    );
    const child = await frame.locator("#child").elementHandle();
    await child.click();
    assert.equal(
      await frame.evaluate(() => typeof globalThis.stolenRootCapabilities?.remoteHandleGet),
      "undefined",
    );
    await child.dispose();
  } finally {
    await page.close();
  }
});

test("nested handle arguments use the target frame prototypes", async () => {
  const page = shim.newPage();
  try {
    await page.setContent("<iframe></iframe>");
    const frame = page.frames().find((frame) => frame !== page);
    const object = await frame.evaluateHandle(() => ({ value: 7 }));
    const result = await frame.evaluate(
      ({ wrapped }) => ({
        argumentOwnRealm: wrapped.constructor === Object,
        handleOwnRealm: wrapped.object.constructor === Object,
        value: wrapped.object.value,
      }),
      { wrapped: { object } },
    );
    assert.deepEqual(result, { argumentOwnRealm: true, handleOwnRealm: true, value: 7 });
    await object.dispose();
  } finally {
    await page.close();
  }
});

test("frame helper state ignores replaced guest intrinsics and inherited setters", async () => {
  const page = shim.newPage();
  try {
    await page.setContent('<iframe srcdoc="<button>child</button>"></iframe>');
    await page.evaluate(() => {
      const source = Function.prototype.toString;
      Function.prototype.toString = function () {
        const original = Reflect.apply(source, this, []);
        return `function(...args){globalThis.leakedHelperState=__G;return (${original}).apply(this,args);}`;
      };
      globalThis.WeakMap = class {
        get() {
          return undefined;
        }
        set(_key, value) {
          globalThis.leakedHelperState = value;
        }
      };
      Object.defineProperty(Object.prototype, "remoteHandleGet", {
        configurable: true,
        set(value) {
          globalThis.leakedRemoteGetter = value;
        },
      });
    });
    const frame = page.frames().find((frame) => frame !== page);
    await frame.getByRole("button", { name: "child" }).click();
    assert.equal(await page.evaluate(() => typeof globalThis.leakedRemoteGetter), "undefined");
    assert.equal(await frame.evaluate(() => typeof globalThis.leakedHelperState), "undefined");
  } finally {
    await page.close();
  }
});
