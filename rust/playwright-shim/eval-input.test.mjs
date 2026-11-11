import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { newPage } from "./index.mjs";

test("Locator.evaluate uses indexed live nodes and persists page effects", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<button id="one">one</button><button id="two">two</button><p id="state">old</p><script>window.probeState=17; document.querySelector("#two").onclick=()=>document.querySelector("#state").textContent="clicked";</script>',
    );
    await page._ensureLive();
    assert.equal(
      await page
        .locator("button")
        .nth(1)
        .evaluate((node) => node.id),
      "two",
    );
    assert.equal(await page.locator("#two").evaluate(() => window.probeState), 17);
    await page.locator("#two").evaluate((node, value) => {
      node.setAttribute("data-probe", value);
      node.click();
    }, "yes");
    assert.equal(await page.locator("#two").getAttribute("data-probe"), "yes");
    assert.equal(await page.locator("#state").textContent(), "clicked");
  } finally {
    await page.close();
  }
});

test("Locator.evaluateAll receives the complete matched collection and optional argument", async () => {
  const page = newPage();
  try {
    await page.setContent("<ul><li>A</li><li>B</li></ul>");
    assert.deepEqual(
      await page
        .locator("li")
        .evaluateAll((nodes, suffix) => nodes.map((node) => node.textContent + suffix), "!"),
      ["A!", "B!"],
    );
    assert.deepEqual(
      await page
        .locator("li")
        .nth(1)
        .evaluateAll((nodes) => nodes.map((node) => node.textContent)),
      ["B"],
    );
    assert.equal(await page.locator(".absent").evaluateAll((nodes) => nodes.length), 0);
  } finally {
    await page.close();
  }
});

test("Page.evaluate awaits async callbacks after ordinary load navigation", async () => {
  const server = createServer((_request, response) => response.end("<!doctype html><p>page</p>"));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "load" });
    assert.equal(await page.evaluate(async () => 42), 42);
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("scrollIntoViewIfNeeded moves an offscreen target into the viewport", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<style>body{margin:0}#space{height:1600px}</style><div id="space"></div><button id="bottom">Bottom</button>',
    );
    await page._ensureLive();
    await page.locator("#bottom").scrollIntoViewIfNeeded();
    const result = await page.evaluate(() => ({
      y: scrollY,
      top: document.querySelector("#bottom").getBoundingClientRect().top,
      height: innerHeight,
    }));
    assert.ok(result.y > 0);
    assert.ok(result.top >= 0 && result.top < result.height);
  } finally {
    await page.close();
  }
});

test("hover enters newly hovered ancestors and preserves related targets on departure", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<div id="parent"><span id="child">child</span></div><button id="other">other</button><script>window.hoverLog=[]; for(const id of ["parent","child"]) for(const type of ["pointerenter","mouseenter","pointerleave","mouseleave"]) document.getElementById(id).addEventListener(type,e=>hoverLog.push([id,e.type,e.relatedTarget?.id||null]));</script>',
    );
    await page._ensureLive();
    await page.locator("#parent").hover();
    const first = await page.evaluate(() => hoverLog.slice());
    assert.ok(first.some(([id, type]) => id === "parent" && type === "mouseenter"));
    assert.ok(first.some(([id, type]) => id === "parent" && type === "pointerenter"));
    await page.locator("#parent").hover();
    assert.equal((await page.evaluate(() => hoverLog)).length, first.length);
    await page.locator("#other").hover();
    const last = await page.evaluate(() => hoverLog.slice(-4));
    assert.ok(last.every(([, type, related]) => type.endsWith("leave") && related === "other"));
  } finally {
    await page.close();
  }
});
