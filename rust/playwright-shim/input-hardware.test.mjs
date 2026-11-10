// Input-device entry points through the shim: page.mouse, page.touchscreen,
// locator.dragTo, page.dragAndDrop. There's no browser compositor, but the live
// isolate runs the page's JS, so pointer/mouse/touch handlers are real — the
// shim just has to dispatch the events (turbo-surf#227). Offline (localhost),
// skips cleanly if the napi addon isn't built.

import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

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

test("page.touchscreen.tap fires pointer+touch events and a click", async () => {
  const { page, teardown } = await fixture();
  try {
    await page.touchscreen.tap(30, 40);
    const log = await page.locator("#log").textContent();
    for (const ev of ["pointerdown", "touchstart", "pointerup", "touchend", "click"]) {
      assert.match(log, new RegExp(ev), `missing ${ev} in: ${log}`);
    }
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
