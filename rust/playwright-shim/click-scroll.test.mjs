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

const control = `<div id="root"><label>Schedule</label><div id="control" role="combobox" tabindex="0" aria-expanded="false">Select</div></div>`;
const styles = `<style>body{margin:8px}#space{height:800px}#root{position:relative;width:200px;height:56px}label{display:block;height:20px}#control{height:36px;background:lightgray}</style>`;
const events = `<script>globalThis.events=[];document.querySelector('#control').onmousedown=e=>{events.push({target:e.target.id,x:e.clientX,y:e.clientY});e.currentTarget.setAttribute('aria-expanded','true')};</script>`;
async function fixture(html, run, child = "") {
  const server = createServer((request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(request.url === "/child" ? child : html);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, {
      waitUntil: "load",
      timeout: 3000,
    });
    await run(page);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

function observed() {
  const root = document.querySelector("#root");
  const box = root.getBoundingClientRect();
  return {
    expanded: document.querySelector("#control").getAttribute("aria-expanded"),
    events,
    scrollY,
    scrollX,
    top: box.top,
    left: box.left,
    height: innerHeight,
    width: innerWidth,
  };
}

test("click scrolls a below-viewport labelled container before its actual descendant hit", async () => {
  await fixture(styles + '<div id="space"></div>' + control + events, async (page) => {
    assert.ok((await page.locator("#root").boundingBox()).y > 720);
    await page.locator("#root").click({ timeout: 2000 });
    const result = await page.evaluate(observed);
    assert.equal(result.expanded, "true");
    assert.ok(result.scrollY > 0);
    assert.equal(result.events.length, 1);
    assert.equal(result.events[0].target, "control");
    assert.ok(result.events[0].y >= 0 && result.events[0].y < result.height);
  });
});

test("click uses captured native scrolling despite poisoned instance and prototype methods", async () => {
  const poison = `<script>globalThis.poisonCalls=0;Element.prototype.scrollIntoViewIfNeeded=function(){poisonCalls++;throw new Error('authored prototype scroll')};document.querySelector('#root').scrollIntoViewIfNeeded=()=>{poisonCalls++;throw new Error('authored instance scroll')};</script>`;
  await fixture(styles + '<div id="space"></div>' + control + events + poison, async (page) => {
    await page.locator("#root").click({ timeout: 2000 });
    assert.equal((await page.evaluate(observed)).expanded, "true");
    assert.equal(await page.evaluate(() => poisonCalls), 0);
  });
});

test("click scrolls a nested scrollport without moving its already visible document", async () => {
  await fixture(
    styles +
      '<div id="viewport" style="height:150px;width:240px;overflow:auto"><div id="space"></div>' +
      control +
      "</div>" +
      events,
    async (page) => {
      await page.locator("#root").click({ timeout: 2000 });
      const result = await page.evaluate(observed);
      assert.equal(result.expanded, "true");
      assert.equal(result.scrollY, 0);
      assert.ok((await page.locator("#viewport").evaluate((el) => el.scrollTop)) > 0);
      assert.ok(result.top >= 8 && result.top + 56 <= 158);
    },
  );
});

test("click uses the actual frame viewport and scrolls only the owning document", async () => {
  await fixture(
    '<iframe style="width:300px;height:200px" src="/child"></iframe>',
    async (page) => {
      const frame = page.frameLocator("iframe");
      await frame.locator("#root").click({ timeout: 2000 });
      const result = await frame.locator("#root").evaluate(() => ({
        expanded: document.querySelector("#control").getAttribute("aria-expanded"),
        y: scrollY,
        top: document.querySelector("#root").getBoundingClientRect().top,
        height: innerHeight,
      }));
      assert.equal(result.expanded, "true");
      assert.ok(result.y > 0);
      assert.equal(result.height, 200);
      assert.ok(result.top >= 0 && result.top + 56 <= result.height);
      assert.equal(await page.evaluate(() => scrollY), 0);
    },
    styles + '<div id="space"></div>' + control + events,
  );
});

test("in-viewport controls keep their actual position without invoking scroll", async () => {
  await fixture(styles + control + events, async (page) => {
    await page.locator("#root").click({ timeout: 2000 });
    const result = await page.evaluate(observed);
    assert.equal(result.expanded, "true");
    assert.equal(result.scrollY, 0);
    assert.equal(result.top, 8);
  });
});

test("an unscrollable fixed target reports the outside point before dispatching events", async () => {
  await fixture(
    styles + "<style>#root{position:fixed;top:808px}</style>" + control + events,
    async (page) => {
      await assert.rejects(
        page.locator("#root").click({ timeout: 2000 }),
        /outside the visible scrollport/,
      );
      assert.deepEqual(await page.evaluate(() => events), []);
    },
  );
});

test("explicit click positions are computed after real scrolling and cannot escape the scrollport", async () => {
  await fixture(styles + '<div id="space"></div>' + control + events, async (page) => {
    await page.locator("#root").click({ position: { x: 100, y: 40 }, timeout: 2000 });
    assert.equal((await page.evaluate(observed)).expanded, "true");
    await assert.rejects(
      page.locator("#root").click({ position: { x: 2000, y: 40 }, timeout: 2000 }),
      /outside the visible scrollport/,
    );
    assert.equal(await page.evaluate(() => events.length), 1);
  });
});
