import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { scrollClickTarget } from "./click-scroll.mjs";
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

function standardsFixture(html, run) {
  return fixture("<!doctype html>" + html, run);
}

test("BODY overflow propagated to its viewport does not become a short independent clip", async () => {
  await standardsFixture(
    styles +
      '<style>body{height:40px;overflow:hidden}</style><div style="height:100px"></div>' +
      control +
      events,
    async (page) => {
      await page.locator("#root").click({ timeout: 1000 });
      assert.equal((await page.evaluate(observed)).expanded, "true");
      assert.equal(await page.locator("body").evaluate((body) => body.scrollTop), 0);
    },
  );
});

test("root nonvisible overflow retains the BODY element's actual independent scrollport", async () => {
  await standardsFixture(
    styles +
      '<style>html{overflow:hidden}body{height:60px;overflow:hidden}</style><div style="height:100px"></div>' +
      control +
      events,
    async (page) => {
      await page.locator("#root").click({ timeout: 1000 });
      assert.equal((await page.evaluate(observed)).expanded, "true");
      assert.ok((await page.locator("body").evaluate((body) => body.scrollTop)) > 0);
    },
  );
});

test("containment cannot turn a genuine BODY clip into an allowed click", async () => {
  for (const containment of ["body{contain:paint}", "html{contain:paint}"]) {
    await standardsFixture(
      styles +
        "<style>body{height:40px;overflow:clip}" +
        containment +
        '</style><div style="height:100px"></div>' +
        control +
        events,
      async (page) => {
        await assert.rejects(
          page.locator("#root").click({ timeout: 1000 }),
          /outside the visible scrollport/,
        );
        assert.deepEqual(await page.evaluate(() => events), []);
      },
    );
  }
});

test("fixed targets escape only ordinary intermediate overflow ancestors", async () => {
  const html =
    styles +
    '<style>#clip{height:40px;overflow:clip}#root{position:fixed;top:300px}</style><div id="clip">' +
    control +
    "</div>" +
    events;
  await standardsFixture(html, async (page) => {
    await page.locator("#root").click({ timeout: 1000 });
    assert.equal((await page.evaluate(observed)).expanded, "true");
    assert.equal(await page.evaluate(() => scrollY), 0);
  });
});

test("a fixed containing block retains its genuine ancestor clip", async () => {
  await standardsFixture(
    styles +
      '<style>#clip{height:40px;overflow:clip;contain:paint}#root{position:fixed;top:100px}</style><div id="clip">' +
      control +
      "</div>" +
      events,
    async (page) => {
      await assert.rejects(
        page.locator("#root").click({ timeout: 1000 }),
        /outside the visible scrollport/,
      );
      assert.deepEqual(await page.evaluate(() => events), []);
    },
  );
});

test("query containers do not capture viewport-fixed click targets", async () => {
  for (const type of ["size", "inline-size"]) {
    await standardsFixture(
      styles +
        `<style>#clip{height:40px;overflow:clip;container-type:${type}}#root{position:fixed;top:300px}</style><div id="clip">` +
        control +
        "</div>" +
        events,
      async (page) => {
        await page.locator("#root").click({ timeout: 1000 });
        assert.equal((await page.evaluate(observed)).expanded, "true");
        assert.equal(await page.evaluate(() => scrollY), 0);
      },
    );
  }
});

test("root and BODY query containment retain BODY's own click scrollport", async () => {
  for (const owner of ["html", "body"]) {
    for (const rule of [
      "content-visibility:auto",
      "container-type:size",
      "container-type:inline-size",
    ]) {
      await standardsFixture(
        styles +
          `<style>body{height:40px;overflow:hidden}${owner}{${rule}}</style><div style="height:100px"></div>` +
          control +
          events,
        async (page) => {
          await page.locator("#root").click({ timeout: 1000 });
          assert.equal((await page.evaluate(observed)).expanded, "true", `${owner} ${rule}`);
          assert.ok(
            (await page.locator("body").evaluate((body) => body.scrollTop)) > 0,
            `${owner} ${rule}`,
          );
          assert.equal(await page.evaluate(() => events.length), 1);
        },
      );
    }
  }
});

function clippingFixture() {
  const rect = (left, top, right, bottom) => ({ left, top, right, bottom });
  const document = {
    defaultView: { innerWidth: 200, innerHeight: 180, getComputedStyle: (node) => node.style },
  };
  const node = (localName, parent, style, box) => ({
    ownerDocument: document,
    namespaceURI: "http://www.w3.org/1999/xhtml",
    localName,
    parent,
    style: { overflowX: "visible", overflowY: "visible", contain: "", ...style },
    getBoundingClientRect: () => box,
    clientTop: 2,
    clientLeft: 3,
    clientWidth: 100,
    clientHeight: 80,
    isConnected: true,
  });
  const root = node("html", null);
  document.documentElement = root;
  const body = node(
    "body",
    root,
    { overflowX: "hidden", overflowY: "hidden" },
    rect(0, 0, 120, 95),
  );
  document.body = body;
  const viewport = node(
    "section",
    body,
    { overflowX: "auto", overflowY: "clip" },
    rect(10, 20, 130, 120),
  );
  const initial = rect(140, 130, 160, 145);
  const moved = rect(30, 40, 50, 55);
  const target = node("button", viewport, {}, initial);
  return { root, body, viewport, target, initial, moved, rect };
}

test("click clipping distinguishes viewport propagation, fixed positioning and containing blocks", (t) => {
  const { body, viewport, target, initial, moved, rect } = clippingFixture();
  let calls = 0;
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "__tsNs");
  t.after(() => {
    if (descriptor) Object.defineProperty(globalThis, "__tsNs", descriptor);
    else delete globalThis.__tsNs;
  });
  globalThis.__tsNs = {
    tcLocatorParent: (node) => node.parent,
    shimNativeClickScroll: (element) => {
      assert.equal(element, target);
      calls++;
      target.getBoundingClientRect = () => moved;
    },
  };

  target.style.position = "fixed";
  assert.deepEqual(scrollClickTarget(target), { rect: initial, clip: rect(0, 0, 200, 180) });
  assert.equal(calls, 0);

  viewport.style.transform = "translateX(1px)";
  assert.deepEqual(scrollClickTarget(target), { rect: moved, clip: rect(13, 22, 113, 102) });
  assert.equal(calls, 1);

  body.style.contain = "paint";
  assert.deepEqual(scrollClickTarget(target), { rect: moved, clip: rect(13, 22, 103, 82) });
  assert.equal(calls, 1);

  target.getBoundingClientRect = () => initial;
  target.isConnected = false;
  assert.throws(() => scrollClickTarget(target), /detached during scrolling/);
  assert.equal(calls, 2);
});
