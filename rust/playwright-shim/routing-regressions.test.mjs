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

test("locator press dispatches key events on its focused element", async () => {
  const page = shim.newPage();
  try {
    await page.setContent(
      "<input id=\"field\" onkeydown=\"document.querySelector('output').textContent+=event.key+':'\"><output></output>",
    );
    await page._ensureLive();
    await page.locator("#field").press("Enter");
    await page.locator("#field").press("Escape");
    assert.equal(
      await page.evaluate(() => document.querySelector("output").textContent),
      "Enter:Escape:",
    );
    assert.equal(await page.evaluate(() => document.activeElement.id), "field");
  } finally {
    await page.close();
  }
});

test("XHR route abort emits an error and continue reaches the real response", async () => {
  const server = createServer((_request, response) => {
    response.setHeader("access-control-allow-origin", "*");
    response.end("upstream");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${server.address().port}/api`;
  try {
    for (const action of ["abort", "continue"]) {
      const page = shim.newPage();
      try {
        if (action === "abort")
          await page.route("**/api", (route) => route.abort("connectionreset"));
        else await page.route("**/api", (route) => route.continue());
        await page.setContent("<body></body>");
        await page._ensureLive();
        const result = await page.evaluate(
          (url) =>
            new Promise((resolve) => {
              const xhr = new XMLHttpRequest();
              xhr.open("GET", url);
              xhr.onload = () =>
                resolve({ event: "load", status: xhr.status, body: xhr.responseText });
              xhr.addEventListener("error", () =>
                resolve({ event: "error", status: xhr.status, body: xhr.responseText }),
              );
              xhr.send();
            }),
          endpoint,
        );
        assert.deepEqual(
          result,
          action === "abort"
            ? { event: "error", status: 0, body: "" }
            : { event: "load", status: 200, body: "upstream" },
        );
      } finally {
        await page.close();
      }
    }
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("route fallback visits older handlers and unroute updates one live wrapper", async () => {
  const server = createServer((_request, response) => {
    response.setHeader("access-control-allow-origin", "*");
    response.end("upstream");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${server.address().port}/api`;
  const page = shim.newPage();
  const older = (route) => route.fulfill({ body: "older" });
  const newer = (route) => route.fallback();
  try {
    await page.route("**/api", older);
    await page.route("**/api", newer);
    await page.setContent("<body></body>");
    await page._ensureLive();
    const read = () => page.evaluate(async (url) => (await fetch(url)).text(), endpoint);
    assert.equal(await read(), "older");
    assert.equal(
      await page.evaluate(
        (url) =>
          new Promise((resolve) => {
            const xhr = new XMLHttpRequest();
            xhr.open("GET", url);
            xhr.onload = () => resolve(xhr.responseText);
            xhr.send();
          }),
        endpoint,
      ),
      "older",
    );
    await page.unroute("**/api", newer);
    assert.equal(await read(), "older");
    await page.unroute("**/api", older);
    assert.equal(await read(), "upstream");
    await page.route("**/api", (route) => route.fulfill({ body: "replaced" }));
    assert.equal(await read(), "replaced");
    await page.unroute("**/api");
    assert.equal(await read(), "upstream");
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("a missed web-first assertion hydrates default navigation while commit stays static", async () => {
  const server = createServer((_request, response) =>
    response.end(
      '<body><script>setTimeout(()=>{document.body.innerHTML="<input id=ready>"},100)</script></body>',
    ),
  );
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${server.address().port}/`;
  try {
    for (const waitUntil of ["load", "commit"]) {
      const page = shim.newPage();
      try {
        await page.goto(endpoint, { waitUntil });
        assert.equal(page._live, false);
        const assertion = shim.expect(page.locator("#ready")).toBeVisible({ timeout: 1000 });
        if (waitUntil === "load") {
          await assertion;
          assert.equal(page._live, true);
        } else {
          await assert.rejects(assertion);
          assert.equal(page._live, false);
        }
      } finally {
        await page.close();
      }
    }
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("unsupported offline emulation rejects instead of silently leaving networking online", async () => {
  const context = new shim.BrowserContext();
  try {
    await assert.rejects(context.setOffline(true), /offline emulation.*unavailable/i);
    await context.setOffline(false);
    assert.equal(context._offline, false);
    assert.throws(
      () => new shim.BrowserContext({ offline: true }),
      /offline emulation.*unavailable/i,
    );
  } finally {
    await context.close();
  }
});
