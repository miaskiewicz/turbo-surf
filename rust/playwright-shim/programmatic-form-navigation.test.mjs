import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("programmatic form navigation: native addon not built, skipping");
  process.exit(0);
}

function bounded(promise, message) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(message)), 1000);
    }),
  ]).finally(() => clearTimeout(timer));
}

async function fixture(heldBody, run) {
  const requests = [];
  let heldResponse, entered;
  const received = new Promise((resolve) => {
    entered = resolve;
  });
  const server = createServer((request, response) => {
    requests.push([request.method, request.url]);
    response.setHeader("content-type", "text/html");
    if (request.url.startsWith("/result")) {
      if (heldBody) {
        heldResponse = response;
        response.flushHeaders();
      } else response.end('<p id="replacement">new document</p>');
      entered();
    } else response.end('<form action="/result"><input name=q value=programmatic></form>');
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  const release = () => {
    const response = heldResponse;
    heldResponse = null;
    response?.end('<p id="replacement">new document</p>');
  };
  try {
    await page.goto(origin, { waitUntil: "networkidle" });
    await run({ page, origin, requests, received, release });
    assert.deepEqual(requests, [
      ["GET", "/"],
      ["GET", "/result?q=programmatic"],
    ]);
    assert.equal(
      page._readEngine('return document.querySelector("#replacement")?.textContent;'),
      "new document",
    );
  } finally {
    release();
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

function submit(page) {
  return page.evaluate(() => {
    document.forms[0].submit();
    return "submitted";
  });
}

test(
  "programmatic form admission progresses through ordinary URL waiting",
  { timeout: 6000 },
  async () => {
    await fixture(false, async ({ page }) => {
      assert.equal(await submit(page), "submitted");
      await page.waitForURL(/\/result\?q=programmatic$/, { timeout: 1000 });
    });
  },
);

test(
  "a readonly command does not cancel or replay an admitted form's held response",
  { timeout: 6000 },
  async () => {
    await fixture(true, async ({ page, origin, requests, received, release }) => {
      assert.equal(
        await bounded(submit(page), "evaluate awaited its navigation body"),
        "submitted",
      );
      await bounded(received, "accepted form was not admitted to native transport");
      assert.equal(
        await bounded(
          page.evaluate(() => 42),
          "evaluate awaited the held navigation",
        ),
        42,
      );
      assert.equal(page._readEngine("return document.forms.length;"), 1);
      assert.equal(page.url(), `${origin}/`);
      assert.equal(requests.length, 2);
      release();
      await page.waitForURL(/\/result\?q=programmatic$/, { timeout: 1000 });
    });
  },
);
