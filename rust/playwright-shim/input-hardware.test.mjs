// Input-device entry points through the shim: page.mouse, page.touchscreen,
// locator.dragTo, page.dragAndDrop. There's no browser compositor, but the live
// isolate runs the page's JS, so pointer/mouse/touch handlers are real — the
// shim just has to dispatch the events (turbo-surf#227). Offline (localhost),
// skips cleanly if the napi addon isn't built.

import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { runInNewContext } from "node:vm";

let shim;
try {
  shim = await import("./index.mjs");
} catch (e) {
  console.log("shim: native addon not built, skipping —", e.message);
  process.exit(0);
}
const { newPage } = shim;

function serveHtml(body) {
  const server = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(body);
  });
  return server;
}
async function listen(server) {
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return `http://127.0.0.1:${server.address().port}`;
}

// Attaches capture-phase listeners for every input event type and writes each
// fire into #log so the test can read the event sequence out through the shim.
const WITH_LOG = `<!doctype html><html><body>
  <div id="log"></div>
  <div id="src" style="width:100px;height:100px">SRC</div>
  <div id="dst" style="width:100px;height:100px;margin-left:200px">DST</div>
  <script>
    const log = document.getElementById('log');
    const rec = (e) => { log.textContent += e.type + (e.target && e.target.id ? ':' + e.target.id : '') + ','; };
    const types = ['pointerdown','pointermove','pointerup','mousedown','mousemove','mouseup','click','dblclick','wheel','touchstart','touchend','dragstart','dragenter','dragover','drop','dragend'];
    for (const t of types) window.addEventListener(t, rec, true);
    document.getElementById('dst').addEventListener('dragover', e => e.preventDefault());
  </script>
</body></html>`;

async function fixture() {
  const srv = serveHtml(WITH_LOG);
  const base = await listen(srv);
  const page = newPage();
  await page.goto(`${base}/`, { waitUntil: "networkidle" });
  return {
    page,
    async teardown() {
      await page.close();
      srv.closeAllConnections();
      await new Promise((r) => srv.close(r));
    },
  };
}

test("page.mouse: move/down/up/click dispatch pointer+mouse events into the live isolate", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.mouse.move(10, 20);
    await page.mouse.down();
    await page.mouse.up();
    const log = await page.locator("#log").textContent();
    for (const ev of [
      "pointermove",
      "mousemove",
      "pointerdown",
      "mousedown",
      "pointerup",
      "mouseup",
      "click",
    ]) {
      assert.match(log, new RegExp(ev), `missing ${ev} in: ${log}`);
    }
  } finally {
    await teardown();
  }
});

test("page.mouse.wheel fires a wheel event", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.mouse.wheel(0, 100);
    const log = await page.locator("#log").textContent();
    assert.match(log, /wheel/, `missing wheel in: ${log}`);
  } finally {
    await teardown();
  }
});

test("page.touchscreen.tap keeps native payloads and suppresses canceled activation (#442)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      window.nativeTouchRows = [];
      window.cancelNativeTouch = false;
      const target = document.getElementById("src");
      for (const type of [
        "pointerdown",
        "touchstart",
        "pointerup",
        "touchend",
        "mousedown",
        "mouseup",
        "click",
      ]) {
        target.addEventListener(
          type,
          (event) => {
            const touch = event.changedTouches?.[0];
            window.nativeTouchRows.push(
              touch
                ? [
                    event.type,
                    event.touches.length,
                    event.targetTouches.length,
                    event.changedTouches.length,
                    touch instanceof Touch,
                    touch.target === target,
                    touch.clientX,
                    touch.clientY,
                  ]
                : [event.type],
            );
            if (window.cancelNativeTouch && event.type === "touchstart") event.preventDefault();
          },
          { passive: false },
        );
      }
    });
    await page.touchscreen.tap(30, 40);
    const ordinary = await page.evaluate(() => window.nativeTouchRows);
    assert.deepEqual(
      ordinary.map(([type]) => type),
      ["pointerdown", "touchstart", "pointerup", "touchend", "mousedown", "mouseup", "click"],
    );
    assert.deepEqual(ordinary[1], ["touchstart", 1, 1, 1, true, true, 30, 40]);
    assert.deepEqual(ordinary[3], ["touchend", 0, 0, 1, true, true, 30, 40]);
    await page.evaluate(() => {
      window.nativeTouchRows = [];
      window.cancelNativeTouch = true;
    });
    await page.touchscreen.tap(30, 40);
    assert.deepEqual(await page.evaluate(() => window.nativeTouchRows.map(([type]) => type)), [
      "pointerdown",
      "touchstart",
      "pointerup",
      "touchend",
    ]);
  } finally {
    await teardown();
  }
});

test("locator.dragTo fires a down→move→up sequence with HTML5 drag events", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.locator("#src").dragTo(page.locator("#dst"));
    const log = await page.locator("#log").textContent();
    // Pointer/mouse sequence on source then target.
    assert.match(log, /pointerdown:src/, `missing pointerdown on src: ${log}`);
    assert.match(log, /mousedown:src/, `missing mousedown on src: ${log}`);
    assert.match(log, /pointermove:dst|mousemove:dst/, `missing move on dst: ${log}`);
    assert.match(log, /pointerup:dst|mouseup:dst/, `missing up on dst: ${log}`);
    // HTML5 drag-and-drop lifecycle.
    assert.match(log, /dragstart:src/, `missing dragstart on src: ${log}`);
    assert.match(log, /drop:dst/, `missing drop on dst: ${log}`);
    assert.match(log, /dragend/, `missing dragend: ${log}`);
  } finally {
    await teardown();
  }
});

test("page.dragAndDrop(source, target) wraps dragTo", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.dragAndDrop("#src", "#dst");
    const log = await page.locator("#log").textContent();
    assert.match(log, /dragstart:src/);
    assert.match(log, /drop:dst/);
  } finally {
    await teardown();
  }
});

test("dragTo respects canceled dragstart and rejected dragover (#471)", async () => {
  const server = serveHtml(`<!doctype html><div id="src" draggable="true">source</div>
    <div id="dst">target</div><script>
    window.events = [];
    window.cancelStart = false;
    window.acceptDrop = true;
    const source = document.getElementById('src');
    const target = document.getElementById('dst');
    source.addEventListener('dragstart', e => {
      events.push('dragstart');
      if (cancelStart) e.preventDefault();
      else e.dataTransfer.setData('text/plain', 'payload');
    });
    target.addEventListener('dragover', e => {
      events.push('dragover');
      if (acceptDrop) e.preventDefault();
    });
    target.addEventListener('drop', e => events.push('drop:' + e.dataTransfer.getData('text/plain')));
    source.addEventListener('dragend', () => events.push('dragend'));
    </script>`);
  const base = await listen(server);
  const page = newPage();
  try {
    for (const [mode, expected] of [
      ["control", ["dragstart", "dragover", "drop:payload", "dragend"]],
      ["cancelStart", ["dragstart"]],
      ["rejectDrop", ["dragstart", "dragover", "dragend"]],
    ]) {
      await page.goto(base, { waitUntil: "networkidle" });
      await page.evaluate((value) => {
        window.cancelStart = value === "cancelStart";
        window.acceptDrop = value !== "rejectDrop";
      }, mode);
      await page.locator("#src").dragTo(page.locator("#dst"));
      assert.deepEqual(await page.evaluate(() => window.events), expected, mode);
    }
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("mouse clickCount preserves incremental details and dblclick (#441)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      window.clickDetails = [];
      for (const type of ["mousedown", "mouseup", "click", "dblclick"])
        window.addEventListener(type, (event) => window.clickDetails.push([type, event.detail]));
    });
    await page.mouse.click(10, 20, { clickCount: 3 });
    assert.deepEqual(await page.evaluate(() => window.clickDetails), [
      ["mousedown", 1],
      ["mouseup", 1],
      ["click", 1],
      ["mousedown", 2],
      ["mouseup", 2],
      ["click", 2],
      ["dblclick", 2],
      ["mousedown", 3],
      ["mouseup", 3],
      ["click", 3],
    ]);
    await page.evaluate(() => {
      window.clickDetails = [];
    });
    await page.mouse.dblclick(10, 20);
    assert.deepEqual(
      await page.evaluate(() =>
        window.clickDetails.filter(([type]) => type === "click" || type === "dblclick"),
      ),
      [
        ["click", 1],
        ["click", 2],
        ["dblclick", 2],
      ],
    );
  } finally {
    await teardown();
  }
});

test("mouse buttons retain chords and require a matching press (#440)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      window.buttonEvents = [];
      for (const type of [
        "pointerdown",
        "pointermove",
        "pointerup",
        "mousedown",
        "mouseup",
        "click",
        "contextmenu",
        "auxclick",
      ])
        window.addEventListener(type, (event) =>
          window.buttonEvents.push([type, event.button, event.buttons]),
        );
    });
    await page.mouse.click(10, 20, { button: "right" });
    const right = await page.evaluate(() =>
      window.buttonEvents.filter(([type]) => type !== "pointermove"),
    );
    assert.deepEqual(right, [
      ["pointerdown", 2, 2],
      ["mousedown", 2, 2],
      ["contextmenu", 2, 2],
      ["pointerup", 2, 0],
      ["mouseup", 2, 0],
      ["auxclick", 2, 0],
    ]);
    await page.evaluate(() => {
      window.buttonEvents = [];
    });
    await page.mouse.down({ button: "left" });
    await page.mouse.down({ button: "right" });
    await page.mouse.up({ button: "right" });
    await page.mouse.up({ button: "left" });
    const chord = await page.evaluate(() => window.buttonEvents);
    assert.deepEqual(
      chord.filter(([type]) => type === "mousedown").map(([, , buttons]) => buttons),
      [1, 3],
    );
    assert.deepEqual(
      chord.filter(([type]) => type === "mouseup").map(([, , buttons]) => buttons),
      [1, 0],
    );
    assert.equal(chord.filter(([type]) => type === "pointerdown").length, 1);
    assert.equal(chord.filter(([type]) => type === "pointerup").length, 1);
    await page.evaluate(() => {
      window.buttonEvents = [];
    });
    await page.mouse.up();
    assert.equal(
      await page.evaluate(() => window.buttonEvents.some(([type]) => type === "click")),
      false,
    );
    await page.mouse.down({ button: "middle" });
    assert.deepEqual(
      await page.evaluate(() => window.buttonEvents.find(([type]) => type === "mousedown")),
      ["mousedown", 1, 4],
    );
    await page.mouse.up({ button: "middle" });
  } finally {
    await teardown();
  }
});

test("locator click forwards position modifiers button and clickCount (#439)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      window.locatorClicks = [];
      document.getElementById("src").style.cssText =
        "position:absolute;left:20px;top:100px;width:100px;height:100px";
      for (const type of ["click", "contextmenu", "auxclick"])
        document.getElementById("src").addEventListener(type, (event) =>
          window.locatorClicks.push({
            type,
            x: event.clientX,
            y: event.clientY,
            button: event.button,
            detail: event.detail,
            shift: event.shiftKey,
          }),
        );
    });
    await page
      .locator("#src")
      .click({ position: { x: 10, y: 15 }, modifiers: ["Shift"], clickCount: 2 });
    const result = await page.evaluate(() => window.locatorClicks);
    assert.deepEqual(
      result.map(({ type, button, detail, shift }) => ({ type, button, detail, shift })),
      [
        { type: "click", button: 0, detail: 1, shift: true },
        { type: "click", button: 0, detail: 2, shift: true },
      ],
    );
    const rect = await page.evaluate(() => {
      const r = document.getElementById("src").getBoundingClientRect();
      return { x: r.x, y: r.y };
    });
    assert.deepEqual(
      result.map(({ x, y }) => ({ x, y })),
      [
        { x: rect.x + 10, y: rect.y + 15 },
        { x: rect.x + 10, y: rect.y + 15 },
      ],
    );
    await page.evaluate(() => {
      window.locatorClicks = [];
    });
    await page.locator("#src").click({ button: "right" });
    assert.deepEqual(
      await page.evaluate(() =>
        window.locatorClicks.map(({ type, button, shift }) => ({ type, button, shift })),
      ),
      [
        { type: "contextmenu", button: 2, shift: false },
        { type: "auxclick", button: 2, shift: false },
      ],
    );
  } finally {
    await teardown();
  }
});

test("touch taps honor cancellation and genuine payload ordering (#442)", async () => {
  const page = newPage();
  page._session = 1;
  const records = [];
  let cancel = false;
  class InputEvent {
    constructor(type, init) {
      this.type = type;
      Object.assign(this, init);
    }
    preventDefault() {
      if (this.cancelable) this.defaultPrevented = true;
    }
  }
  class Touch {
    constructor(init) {
      Object.assign(this, init);
    }
  }
  const target = {
    dispatchEvent(event) {
      records.push([event.type, event.touches?.length, event.changedTouches?.length]);
      if (cancel && event.type === "touchstart") event.preventDefault();
      return !event.defaultPrevented;
    },
  };
  page._sessionEval = async (source) =>
    runInNewContext(source, {
      document: { elementFromPoint: () => target },
      Touch,
      TouchEvent: InputEvent,
      PointerEvent: InputEvent,
      MouseEvent: InputEvent,
      Event: InputEvent,
    });
  await page.touchscreen.tap(10, 20);
  assert.deepEqual(
    records.map(([type]) => type),
    ["pointerdown", "touchstart", "pointerup", "touchend", "mousedown", "mouseup", "click"],
  );
  assert.deepEqual(
    records.find(([type]) => type === "touchstart"),
    ["touchstart", 1, 1],
  );
  assert.deepEqual(
    records.find(([type]) => type === "touchend"),
    ["touchend", 0, 1],
  );
  records.length = 0;
  cancel = true;
  await page.touchscreen.tap(10, 20);
  assert.deepEqual(
    records.map(([type]) => type),
    ["pointerdown", "touchstart", "pointerup", "touchend"],
  );
  page._session = null;
});

test("dispatchEvent targets the matched node without descending (#372)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      const parent = document.getElementById("src");
      parent.innerHTML = '<span id="child">child</span>';
      window.dispatchTargets = [];
      parent.addEventListener("custom", (event) =>
        window.dispatchTargets.push(["parent", event.target.id]),
      );
      document
        .getElementById("child")
        .addEventListener("custom", (event) =>
          window.dispatchTargets.push(["child", event.target.id]),
        );
    });
    await page.locator("#src").dispatchEvent("custom");
    assert.deepEqual(await page.evaluate(() => window.dispatchTargets), [["parent", "src"]]);
    await page.evaluate(() => {
      window.dispatchTargets = [];
    });
    await page.locator("#child").dispatchEvent("custom");
    assert.deepEqual(await page.evaluate(() => window.dispatchTargets), [
      ["child", "child"],
      ["parent", "child"],
    ]);
  } finally {
    await teardown();
  }
});

test("dispatchEvent preserves initializer fields and propagation (#371)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      window.initializedEvents = [];
      const target = document.getElementById("src");
      for (const type of ["click", "keydown", "pointerdown"])
        target.addEventListener(type, (event) =>
          window.initializedEvents.push({
            type: event.type,
            button: event.button,
            x: event.clientX,
            detail: event.detail,
            key: event.key,
            pointerType: event.pointerType,
            bubbles: event.bubbles,
            cancelable: event.cancelable,
            composed: event.composed,
          }),
        );
      document.body.addEventListener("click", () =>
        window.initializedEvents.push({ type: "parent-click" }),
      );
    });
    await page.locator("#src").dispatchEvent("click", {
      button: 2,
      clientX: 37,
      detail: 7,
      bubbles: false,
      cancelable: false,
      composed: true,
    });
    assert.deepEqual(await page.evaluate(() => window.initializedEvents), [
      {
        type: "click",
        button: 2,
        x: 37,
        detail: 7,
        bubbles: false,
        cancelable: false,
        composed: true,
      },
    ]);
    await page.evaluate(() => {
      window.initializedEvents = [];
    });
    await page.locator("#src").dispatchEvent("keydown", { key: "Enter" });
    await page.locator("#src").dispatchEvent("pointerdown", { pointerType: "pen", clientX: 13 });
    const events = await page.evaluate(() => window.initializedEvents);
    assert.equal(events[0].key, "Enter");
    assert.equal(events[0].composed, true);
    assert.equal(events[1].pointerType, "pen");
    assert.equal(events[1].x, 13);
  } finally {
    await teardown();
  }
});

test("locator dblclick emits both clicks and dblclick with options (#370)", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.evaluate(() => {
      window.doubleEvents = [];
      const target = document.getElementById("src");
      for (const type of ["click", "dblclick"])
        target.addEventListener(type, (event) =>
          window.doubleEvents.push([event.type, event.detail, event.shiftKey, event.composed]),
        );
    });
    await page.locator("#src").dblclick({ modifiers: ["Shift"] });
    assert.deepEqual(await page.evaluate(() => window.doubleEvents), [
      ["click", 1, true, true],
      ["click", 2, true, true],
      ["dblclick", 2, true, true],
    ]);
  } finally {
    await teardown();
  }
});
