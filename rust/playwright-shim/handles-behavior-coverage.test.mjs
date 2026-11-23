import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM ?? "./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(html, run) {
  const page = shim.newPage();
  const handles = new Set();
  const keep = (handle) => {
    assert.ok(handle, "fixture must resolve a real native handle");
    handles.add(handle);
    return handle;
  };
  try {
    await page.setContent(html);
    await run(page, keep);
  } finally {
    for (const handle of handles) await handle.dispose();
    await page.close();
  }
}

const isActionTimeout = (error) =>
  error.name === "TimeoutError" && /actionable/.test(error.message);

test("handle read projections preserve retained-node identity and value classification", async () => {
  await fixture(
    '<section id="kept" style="width:180px;height:40px"><span>Alpha</span> Beta</section><input id="field" value="initial"><p id="hidden" hidden>hidden text</p>',
    async (page, keep) => {
      const value = keep(await page.evaluateHandle(() => ({ nodeType: 1, tagName: "BUTTON" })));
      assert.equal(
        value.asElement(),
        null,
        "DOM-looking data must not acquire ElementHandle identity",
      );
      assert.equal(value.toString(), "JSHandle");
      const kept = keep(await page.locator("#kept").elementHandle());
      const field = keep(await page.locator("#field").elementHandle());
      const hidden = keep(await page.locator("#hidden").elementHandle());
      assert.equal(kept.asElement(), kept);
      assert.equal(await kept.innerHTML(), "<span>Alpha</span> Beta");
      assert.equal(await kept.innerText(), "Alpha Beta");
      assert.equal(await field.inputValue(), "initial");
      assert.equal(await kept.isHidden(), false);
      assert.equal(await hidden.isHidden(), true);
      await page.evaluate(() => {
        document.getElementById("kept").remove();
        const replacement = document.createElement("section");
        replacement.id = "kept";
        replacement.textContent = "replacement";
        document.body.prepend(replacement);
      });
      assert.equal(await kept.textContent(), "Alpha Beta");
      assert.equal(await kept.isHidden(), true);
      assert.equal(await page.locator("#kept").textContent(), "replacement");
    },
  );
});

test("retained input fill, focus and press act on the original control after reordering", async () => {
  await fixture(
    '<input id="target" value="old"><input id="decoy" value="unchanged">',
    async (page, keep) => {
      const input = keep(await page.locator("#target").elementHandle());
      await page.evaluate(() => {
        globalThis.inputEvents = [];
        const input = document.getElementById("target");
        for (const type of ["input", "change", "keydown", "keyup"])
          input.addEventListener(type, (event) =>
            inputEvents.push([type, event.key ?? null, input.value]),
          );
        document.body.prepend(document.getElementById("decoy"));
      });
      await input.fill("typed");
      assert.equal(await input.inputValue(), "typed");
      assert.equal(await page.locator("#decoy").inputValue(), "unchanged");
      assert.deepEqual(
        await page.evaluate(() =>
          inputEvents.filter(([type]) => type === "input" || type === "change"),
        ),
        [["input", null, "typed"]],
      );
      await page.locator("#decoy").focus();
      assert.deepEqual(
        await page.evaluate(() => inputEvents.filter(([type]) => type === "change")),
        [["change", null, "typed"]],
        "fill commits a change when the retained control loses focus",
      );
      await input.focus();
      assert.equal(await page.evaluate(() => document.activeElement.id), "target");
      await input.press("End");
      await input.press("!");
      assert.equal(await input.inputValue(), "typed!");
      assert.equal(await page.locator("#decoy").inputValue(), "unchanged");
      assert.equal(
        await page.evaluate(() =>
          inputEvents.some(([type, key]) => type === "keydown" && key === "!"),
        ),
        true,
      );
      assert.equal(
        await page.evaluate(() =>
          inputEvents.some(([type, key]) => type === "keyup" && key === "!"),
        ),
        true,
      );
    },
  );
});

test("actionability failures leave input state unchanged and recovery uses the same handle", async () => {
  await fixture(
    '<input id="target" value="original" readonly><button id="button" aria-disabled="true">action</button>',
    async (page, keep) => {
      const input = keep(await page.locator("#target").elementHandle());
      const button = keep(await page.locator("#button").elementHandle());
      await page.evaluate(() => {
        globalThis.actionLog = [];
        document.getElementById("target").addEventListener("input", () => actionLog.push("input"));
        document.getElementById("button").addEventListener("click", () => actionLog.push("click"));
      });
      await assert.rejects(input.fill("forbidden", { timeout: 30 }), isActionTimeout);
      assert.equal(await input.inputValue(), "original");
      assert.deepEqual(await page.evaluate(() => actionLog), []);
      await assert.rejects(button.click({ timeout: 30 }), isActionTimeout);
      assert.deepEqual(await page.evaluate(() => actionLog), []);
      await button.click({ force: true });
      assert.deepEqual(await page.evaluate(() => actionLog), ["click"]);
      await page.evaluate(() => {
        document.getElementById("target").readOnly = false;
        document.getElementById("target").hidden = true;
      });
      await assert.rejects(input.fill("hidden", { timeout: 30 }), isActionTimeout);
      assert.equal(await input.inputValue(), "original");
      await page.evaluate(() => {
        document.getElementById("target").hidden = false;
      });
      await input.fill("recovered", { force: true });
      assert.equal(await input.inputValue(), "recovered");
      assert.deepEqual(await page.evaluate(() => actionLog), ["click", "input"]);
    },
  );
});

test("checkbox handles check and uncheck idempotently with actual change events", async () => {
  await fixture('<input id="toggle" type="checkbox">', async (page, keep) => {
    const toggle = keep(await page.locator("#toggle").elementHandle());
    await page.evaluate(() => {
      globalThis.checkedChanges = [];
      const toggle = document.getElementById("toggle");
      toggle.addEventListener("change", () => checkedChanges.push(toggle.checked));
    });
    await toggle.check();
    await toggle.check();
    assert.equal(await toggle.evaluate((element) => element.checked), true);
    await toggle.uncheck();
    await toggle.uncheck();
    assert.equal(await toggle.evaluate((element) => element.checked), false);
    assert.deepEqual(await page.evaluate(() => checkedChanges), [true, false]);
  });
});

test("retained ARIA handles observe actual click state and remain idempotent", async () => {
  await fixture(
    '<button id="radio" role="radio" aria-checked="false">History</button><button id="checkbox" role="checkbox" aria-checked="false">Enable</button>',
    async (page, keep) => {
      await page.evaluate(() => {
        globalThis.clicks = {};
        document.addEventListener("click", ({ target }) => {
          clicks[target.id] = (clicks[target.id] || 0) + 1;
          queueMicrotask(() => {
            const on =
              target.getAttribute("role") === "radio" ||
              target.getAttribute("aria-checked") !== "true";
            target.setAttribute("aria-checked", String(on));
          });
        });
      });
      const radio = keep(await page.locator("#radio").elementHandle());
      const checkbox = keep(await page.locator("#checkbox").elementHandle());
      await radio.check();
      await radio.check();
      assert.equal(await radio.getAttribute("aria-checked"), "true");
      await checkbox.check();
      await checkbox.check();
      assert.equal(await checkbox.getAttribute("aria-checked"), "true");
      await checkbox.uncheck();
      await checkbox.uncheck();
      assert.equal(await checkbox.getAttribute("aria-checked"), "false");
      assert.deepEqual(await page.evaluate(() => clicks), { radio: 1, checkbox: 2 });
    },
  );
});

test("checked handles preserve cancellation and disabled actionability", async () => {
  await fixture(
    '<input id="cancelled" type="checkbox"><input id="disabled" type="checkbox" disabled>',
    async (page, keep) => {
      await page.evaluate(() => {
        globalThis.disabledClicks = 0;
        document
          .getElementById("cancelled")
          .addEventListener("click", (event) => event.preventDefault());
        document.getElementById("disabled").addEventListener("click", () => disabledClicks++);
      });
      const cancelled = keep(await page.locator("#cancelled").elementHandle());
      const disabled = keep(await page.locator("#disabled").elementHandle());
      await assert.rejects(cancelled.check(), /Click did not change checked state/);
      assert.equal(await cancelled.evaluate((element) => element.checked), false);
      await assert.rejects(disabled.check({ timeout: 25 }), isActionTimeout);
      assert.equal(await disabled.evaluate((element) => element.checked), false);
      assert.equal(await page.evaluate(() => disabledClicks), 0);
    },
  );
});

test("an already checked detached handle cannot act on a same-id replacement", async () => {
  await fixture('<input id="toggle" type="checkbox" checked>', async (page, keep) => {
    const old = keep(await page.locator("#toggle").elementHandle());
    await page.evaluate(() => {
      const current = document.getElementById("toggle");
      const replacement = document.createElement("input");
      replacement.id = "toggle";
      replacement.type = "checkbox";
      current.replaceWith(replacement);
    });
    await assert.rejects(old.check(), /not attached/);
    assert.equal(await page.locator("#toggle").isChecked(), false);
    assert.equal(await old.evaluate((element) => element.checked), true);
  });
});

test("object handles preserve cyclic arguments, own-property projections and getter failures", async () => {
  await fixture("<p>object handles</p>", async (page, keep) => {
    const object = keep(
      await page.evaluateHandle(() => {
        const value = Object.create({ inherited: 99 });
        value.count = 7;
        value.nested = { label: "live" };
        return value;
      }),
    );
    assert.equal(await object.evaluate("value.count * 2"), 14);
    const nested = keep(await object.evaluateHandle("value.nested"));
    assert.deepEqual(await nested.jsonValue(), { label: "live" });
    const properties = await object.getProperties();
    for (const handle of properties.values()) keep(handle);
    assert.deepEqual([...properties.keys()], ["count", "nested"]);
    assert.equal(await properties.get("count").jsonValue(), 7);
    const graph = {
      handle: object,
      pair: [nested, nested],
      date: new Date("2026-01-02T00:00:00Z"),
      regexp: /live/gi,
    };
    graph.self = graph;
    assert.deepEqual(
      await page.evaluate(
        (graph) => ({
          cycle: graph.self === graph,
          alias: graph.pair[0] === graph.pair[1],
          count: graph.handle.count,
          date: graph.date.toISOString(),
          regexp: graph.regexp.source,
          flags: graph.regexp.flags,
        }),
        graph,
      ),
      {
        cycle: true,
        alias: true,
        count: 7,
        date: "2026-01-02T00:00:00.000Z",
        regexp: "live",
        flags: "gi",
      },
    );
    await object.evaluate((value) => {
      Object.defineProperty(value, "broken", {
        configurable: true,
        get() {
          throw new TypeError("property getter marker");
        },
      });
    });
    await assert.rejects(
      object.getProperty("broken"),
      (error) => error instanceof TypeError && error.message === "property getter marker",
    );
    assert.equal(
      await object.evaluate((value) => value.count),
      7,
      "failed property projection must leave the live object usable",
    );
    await nested.dispose();
    await nested.dispose();
    await assert.rejects(
      nested.evaluate((value) => value.label),
      /disposed/,
    );
    assert.equal(await object.evaluate((value) => value.nested.label), "live");
  });
});

test("prevented and non-left handle clicks do not synthesize anchor navigation", async () => {
  const server = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "text/html" });
    response.end("<p id=destination>anchor destination</p>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const destination = `http://127.0.0.1:${server.address().port}/#unwanted`;
  try {
    await fixture(`<a id="link" href="${destination}">link</a>`, async (page, keep) => {
      const link = keep(await page.locator("#link").elementHandle());
      const before = page.url();
      await page.evaluate(() => {
        globalThis.linkClicks = 0;
        globalThis.preventLinkClick = (event) => {
          linkClicks++;
          event.preventDefault();
        };
        document.getElementById("link").addEventListener("click", preventLinkClick);
      });
      await link.click();
      assert.equal(page.url(), before);
      assert.equal(await page.evaluate(() => linkClicks), 1);
      await page.evaluate(() => {
        document.getElementById("link").removeEventListener("click", preventLinkClick);
      });
      await link.click({ button: "right" });
      assert.equal(page.url(), before);
      assert.equal(await link.getAttribute("href"), destination);
      await link.click();
      assert.equal(page.url(), destination);
      assert.equal(await page.locator("#destination").textContent(), "anchor destination");
    });
  } finally {
    await new Promise((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
