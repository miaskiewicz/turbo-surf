import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("assertion background IO: native addon not built, skipping");
  process.exit(0);
}

async function withHeldResponse(run) {
  let held;
  let requested;
  const entered = new Promise((resolve) => {
    requested = resolve;
  });
  const server = createServer((request, response) => {
    if (request.url === "/held") {
      held = response;
      response.writeHead(200, { "content-type": "text/plain" });
      response.write("pending body");
      requested();
    } else {
      response.end('<!doctype html><p id="state" data-state="pending">Waiting</p>');
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const context = new shim.BrowserContext();
  try {
    const page = await context.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.evaluate(() => {
      void fetch("/held").then((response) => response.text());
    });
    await entered;
    await run(page, held);
  } finally {
    held?.end();
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function boundedObservation(promise, held) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => {
          held.end();
          reject(new Error("assertion joined the held response"));
        }, 1500);
      }),
    ]);
  } finally {
    clearTimeout(timer);
    held.end();
    await promise.catch(() => {});
  }
}

test("attribute assertions observe a timer update while an earlier response body stays open", async () => {
  await withHeldResponse(async (page, held) => {
    await page.evaluate(() => {
      setTimeout(() => document.querySelector("#state").setAttribute("data-state", "done"), 150);
    });
    await boundedObservation(
      shim
        .expect(page.locator("#state"))
        .toHaveAttribute("data-state", "done", { timeout: 1000 })
        .then(() => {
          assert.equal(held.writableEnded, false);
          assert.equal(held.destroyed, false);
        }),
      held,
    );
  });
});

test("an unsatisfied assertion expires without waiting for an earlier response body", async () => {
  await withHeldResponse(async (page, held) => {
    await boundedObservation(
      assert
        .rejects(
          shim
            .expect(page.locator("#state"))
            .toHaveAttribute("data-state", "absent", { timeout: 200 }),
          /expected attribute data-state=absent, got pending/,
        )
        .then(() => {
          assert.equal(held.writableEnded, false);
          assert.equal(held.destroyed, false);
        }),
      held,
    );
  });
});
