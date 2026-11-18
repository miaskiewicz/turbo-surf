import assert from "node:assert/strict";
import { test } from "node:test";
import { newPage } from "./index.mjs";

test("page.dispatchEvent forwards the selector and MouseEvent init to the live native document", async (t) => {
  const page = newPage();
  t.after(() => page.close());
  await page.setContent('<button id="target">Target</button><button id="other">Other</button>');
  await page.evaluate(() => {
    globalThis.dispatched = [];
    globalThis.bubbled = [];
    document.body.addEventListener("click", (event) => bubbled.push(event.target.id));
    for (const button of document.querySelectorAll("button")) {
      button.addEventListener("click", (event) => {
        event.preventDefault();
        dispatched.push({
          target: event.target.id,
          button: event.button,
          x: event.clientX,
          trusted: event.isTrusted,
          prevented: event.defaultPrevented,
        });
      });
    }
  });
  await page.locator("#other").dispatchEvent("click", { button: 1, clientX: 8 });
  assert.deepEqual(await page.evaluate(() => dispatched), [
    { target: "other", button: 1, x: 8, trusted: false, prevented: true },
  ]);
  await page.dispatchEvent("#target", "click", {
    button: 2,
    clientX: 19,
    bubbles: false,
    cancelable: false,
  });
  assert.deepEqual(await page.evaluate(() => dispatched), [
    { target: "other", button: 1, x: 8, trusted: false, prevented: true },
    { target: "target", button: 2, x: 19, trusted: false, prevented: false },
  ]);
  assert.deepEqual(await page.evaluate(() => bubbled), ["other"]);
  await assert.rejects(page.locator("#missing").dispatchEvent("click"), /matched no elements/);
  await assert.rejects(page.dispatchEvent("#missing", "click"), /matched no elements/);
  assert.equal(await page.locator("#target").textContent(), "Target");
});
