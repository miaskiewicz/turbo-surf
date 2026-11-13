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

async function fixture(run) {
  const received = [];
  const server = createServer((request, response) => {
    received.push(request.url);
    response.end(request.url === "/" ? "<body>routes</body>" : "peer");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  try {
    await page.goto(origin);
    await run(page, origin, received);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

const fetchText = (page, url) =>
  page.evaluate((target) => fetch(target).then((r) => r.text()), url);

test("URL predicates retain Node closures and false matches reach older routes or real HTTP", async () => {
  await fixture(async (page, origin, received) => {
    const category = "CONTRACT";
    const calls = [];
    await page.route("**/api**", (route) => route.fulfill({ body: "older" }));
    await page.route(
      function (url) {
        assert.equal(this, undefined);
        assert.ok(url instanceof URL);
        calls.push(url.pathname + url.search);
        return url.searchParams.get("category") === category;
      },
      (route) => route.fulfill({ body: "predicate" }),
    );
    assert.equal(await fetchText(page, `${origin}/api?category=CONTRACT`), "predicate");
    assert.equal(await fetchText(page, `${origin}/api?category=TAX`), "older");
    assert.equal(await fetchText(page, `${origin}/other`), "peer");
    assert.deepEqual(calls, ["/api?category=CONTRACT", "/api?category=TAX", "/other"]);
    assert.ok(received.includes("/other"));
    assert.ok(!received.includes("/api?category=CONTRACT"));
  });
});

test("predicate fallbacks match the original URL while preserving request overrides", async () => {
  await fixture(async (page, origin) => {
    const order = [];
    const path = "/original";
    await page.route(
      (url) => url.pathname === path,
      (route) => {
        order.push("older");
        assert.equal(new URL(route.request().url()).pathname, "/rewritten");
        return route.fulfill({ body: "retained" });
      },
    );
    await page.route(
      (url) => url.pathname === path,
      (route) => {
        order.push("newer");
        return route.fallback({ url: `${origin}/rewritten` });
      },
    );
    assert.equal(await fetchText(page, `${origin}/original`), "retained");
    assert.deepEqual(order, ["newer", "older"]);
  });
});

test("unroute uses predicate identity and optional handler identity", async () => {
  await fixture(async (page, origin) => {
    const predicate = () => true;
    const otherPredicate = () => true;
    const first = (route) => route.fulfill({ body: "first" });
    const second = (route) => route.fulfill({ body: "second" });
    await page.route(predicate, first);
    await page.route(predicate, second);
    await page.unroute(otherPredicate);
    assert.equal(await fetchText(page, `${origin}/api`), "second");
    await page.unroute(predicate, second);
    assert.equal(await fetchText(page, `${origin}/api`), "first");
    await page.unroute(predicate);
    assert.equal(await fetchText(page, `${origin}/api`), "peer");
  });
});

test("a thrown predicate rejects the request instead of silently falling through", async () => {
  await fixture(async (page, origin, received) => {
    await page.route(
      () => {
        throw new Error("predicate fixture rejected");
      },
      () => {},
    );
    assert.equal(
      await page.evaluate(
        (url) =>
          fetch(url).then(
            () => "unexpected",
            (error) => error.message,
          ),
        `${origin}/api`,
      ),
      "predicate fixture rejected",
    );
    assert.ok(!received.includes("/api"));
  });
});

test("XHR predicate matching and paused unroute preserve the original request chain", async () => {
  await fixture(async (page, origin) => {
    const path = "/api";
    const predicate = (url) => url.pathname === path;
    let entered;
    let release;
    const started = new Promise((resolve) => {
      entered = resolve;
    });
    const gate = new Promise((resolve) => {
      release = resolve;
    });
    await page.route(predicate, (route) => route.fulfill({ body: "older" }));
    await page.route(predicate, async (route) => {
      entered();
      await gate;
      await route.fallback();
    });
    const response = page.evaluate(
      (url) =>
        new Promise((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          xhr.onload = () => resolve(xhr.responseText);
          xhr.onerror = () => reject(new Error("XHR fixture failed"));
          xhr.open("GET", url);
          xhr.send();
        }),
      `${origin}/api`,
    );
    response.catch(() => {});
    try {
      await started;
      await page.unroute(predicate);
      release();
      assert.equal(await response, "older");
      assert.equal(await fetchText(page, `${origin}/api`), "peer");
    } finally {
      release();
    }
  });
});
