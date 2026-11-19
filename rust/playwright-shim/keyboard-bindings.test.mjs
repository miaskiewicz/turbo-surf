import assert from "node:assert/strict";
import test from "node:test";
import { newPage } from "./index.mjs";

function gate() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function bounded(work) {
  let timer;
  try {
    return await Promise.race([
      work,
      new Promise((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Keyboard waited for unrelated host callback")),
          2000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

for (const rejected of [false, true]) {
  test(
    `keyboard completes independently and preserves eventual host ${rejected ? "error" : "value"}`,
    { timeout: 15000 },
    async () => {
      const page = newPage();
      const held = gate();
      const entered = gate();
      let typing;
      let released = false;
      try {
        await page.setContent('<input id="input">');
        await page.locator("#input").focus();
        await page.exposeFunction("backgroundHostWork", async () => {
          entered.resolve();
          await held.promise;
          if (rejected) throw new RangeError("preserved-host-error");
          return "preserved-host-value";
        });
        page._readEngine(`setTimeout(() => {
        globalThis.reply = { pending: true };
        backgroundHostWork().then(value => { reply = {value}; document.querySelector('#input').dataset.reply = value; }, error => { reply = {name:error.name,message:error.message}; });
      }, 0); return null;`);
        typing = page.keyboard.type("12");
        await bounded(entered.promise);
        await bounded(typing);
        assert.equal(released, false);
        assert.equal(await page.locator("#input").inputValue(), "12");
        assert.deepEqual(page._readEngine("return globalThis.reply;"), { pending: true });
        released = true;
        held.resolve();
        const reply = await page.evaluate(async () => {
          while (reply.pending) await new Promise((resolve) => setTimeout(resolve, 0));
          return globalThis.reply;
        });
        assert.deepEqual(
          reply,
          rejected
            ? { name: "RangeError", message: "preserved-host-error" }
            : { value: "preserved-host-value" },
        );
        if (!rejected)
          assert.equal(
            await page.locator("#input").getAttribute("data-reply"),
            "preserved-host-value",
          );
      } finally {
        held.resolve();
        await typing?.catch(() => {});
        await page.close();
      }
    },
  );
}

test(
  "held callbacks do not change key cancellation, modifiers, order or frame ownership",
  { timeout: 15000 },
  async () => {
    const page = newPage();
    const held = gate();
    let typing;
    try {
      await page.setContent('<input id="parent"><iframe srcdoc="&lt;input id=child&gt;"></iframe>');
      await page._ensureLive();
      const frame = page.frames()[1];
      await frame.locator("#child").focus();
      await frame.evaluate(() => {
        window.events = [];
        for (const type of ["keydown", "keypress", "keyup"])
          document.addEventListener(type, (event) => {
            events.push([event.type, event.key, event.shiftKey]);
            if (event.type === "keydown" && event.key === "x") event.preventDefault();
          });
      });
      await page.exposeFunction("backgroundHostWork", () => held.promise);
      page._readEngine("setTimeout(() => { void backgroundHostWork(); },0); return null;");
      const { makeKeyboard } = await import("./keyboard.mjs");
      const keyboard = makeKeyboard(frame);
      typing = (async () => {
        await keyboard.press("x");
        await keyboard.press("Shift+a");
        await keyboard.insertText("12");
      })();
      await bounded(typing);
      assert.equal(await frame.locator("#child").inputValue(), "A12");
      assert.equal(await page.locator("#parent").inputValue(), "");
      held.resolve();
      assert.deepEqual(await frame.evaluate(() => events), [
        ["keydown", "x", false],
        ["keyup", "x", false],
        ["keydown", "Shift", true],
        ["keydown", "A", true],
        ["keypress", "A", true],
        ["keyup", "A", true],
        ["keyup", "Shift", false],
      ]);
    } finally {
      held.resolve();
      await typing?.catch(() => {});
      await page.close();
    }
  },
);

test("background binding collection errors remain observable", { timeout: 15000 }, async () => {
  const page = newPage();
  try {
    await page.setContent('<input id="input">');
    await page.locator("#input").focus();
    await page.exposeFunction("unusedHostWork", () => null);
    const failure = new RangeError("binding-collection-failed");
    page._takeBindingCalls = async () => {
      throw failure;
    };
    await page.keyboard.down("a");
    await assert.rejects(page._drainBindings(), (error) => error === failure);
  } finally {
    await page.close();
  }
});

test(
  "closing during unrelated host work does not forge callback completion",
  { timeout: 15000 },
  async () => {
    const page = newPage();
    const held = gate();
    const entered = gate();
    const completed = gate();
    let finished = false;
    let typing;
    try {
      await page.setContent('<input id="input">');
      await page.locator("#input").focus();
      await page.exposeFunction("backgroundHostWork", async () => {
        entered.resolve();
        await held.promise;
        finished = true;
        completed.resolve();
        return "actual-late-value";
      });
      page._readEngine("setTimeout(() => { void backgroundHostWork(); },0); return null;");
      typing = page.keyboard.type("12");
      await bounded(entered.promise);
      await bounded(typing);
      await page.close();
      assert.equal(finished, false);
      held.resolve();
      await bounded(completed.promise);
      assert.equal(finished, true);
    } finally {
      held.resolve();
      await typing?.catch(() => {});
      await page.close();
    }
  },
);
