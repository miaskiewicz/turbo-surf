import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
let newPage;
try {
  ({ newPage } = await import("./index.mjs"));
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run, options) {
  const received = [];
  let releaseBody;
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    received.push({
      url: req.url,
      method: req.method,
      headers: req.headers,
      body: Buffer.concat(chunks),
    });
    if (req.url === "/api/held") {
      res.writeHead(200, { "content-type": "text/plain" });
      res.flushHeaders();
      releaseBody = () => res.end("released");
      return;
    }
    if (req.url === "/release") {
      releaseBody?.();
      res.end("peer-release");
      return;
    }
    if (req.url === "/api/auth" && req.headers.authorization !== "Basic dXNlcjpwYXNz") {
      res.writeHead(401, { "www-authenticate": 'Basic realm="route-fixture"' });
      res.end("auth-required");
      return;
    }
    if (req.url === "/api/redirect") {
      res.writeHead(302, { location: "/api/final", "set-cookie": "redirected=yes; Path=/" });
      res.end();
      return;
    }
    if (req.url === "/child-route") {
      res.setHeader("content-type", "text/html");
      res.end(
        '<script>fetch("/api").then(r=>r.text()).then(t=>globalThis.initialChildResult=t)</script>',
      );
      return;
    }
    if (req.url === "/module-page") {
      res.setHeader("content-type", "text/html");
      res.end(
        '<script type="module">globalThis.moduleRoute = await (await fetch("/api")).text()</script>',
      );
      return;
    }
    if (req.url.startsWith("/api")) {
      res.setHeader("x-upstream", "original");
      res.setHeader("content-type", "text/plain");
      res.end(req.url === "/api/binary" ? Buffer.from([0, 255, 1, 128]) : "upstream");
    } else {
      res.setHeader("content-type", "text/html");
      if (req.url === "/denied")
        res.setHeader("content-security-policy", "script-src 'unsafe-inline'; connect-src 'none'");
      res.end(
        "<title>host routes</title><button id=\"fetch\" onclick=\"fetch('/api').then(r=>r.text()).then(t=>document.querySelector('output').textContent=t)\">fetch</button><output></output>",
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage(options);
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    await run(page, origin, received);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("route callbacks retain Node closures and await host work", async () =>
  fixture(async (page, origin, received) => {
    const prefix = "captured";
    let hits = 0;
    await page.route("**/api", async (route, request) => {
      await new Promise((resolve) => setImmediate(resolve));
      hits++;
      assert.equal(request.url(), origin + "/api");
      assert.equal(request.frame(), page.mainFrame());
      await route.fulfill({ body: prefix + ":" + hits });
    });
    await page.goto(origin, { waitUntil: "networkidle" });
    assert.equal(await page.evaluate(async () => (await fetch("/api")).text()), "captured:1");
    assert.equal(hits, 1);
    assert.equal(received.filter((r) => r.url === "/api").length, 0);
  }));

test("route.fetch preserves upstream bytes and permits response rewrites", async () =>
  fixture(async (page, origin, received) => {
    let seen;
    await page.goto(origin, { waitUntil: "networkidle" });
    await page.route("**/api", async (route) => {
      seen = route.request();
      const upstream = await route.fetch({
        method: "POST",
        postData: Buffer.from([0, 255, 128]),
        headers: { "x-host": "closure" },
      });
      assert.equal(upstream.status(), 200);
      assert.equal(upstream.headers()["x-upstream"], "original");
      await route.fulfill({
        response: upstream,
        body: (await upstream.text()) + " amended",
        headers: { "x-result": "rewritten" },
      });
    });
    assert.deepEqual(
      await page.evaluate(async () => {
        const r = await fetch("/api", { headers: { "x-original": "yes" } });
        return [await r.text(), r.headers.get("x-result"), r.status];
      }),
      ["upstream amended", "rewritten", 200],
    );
    assert.equal(seen.headers()["x-original"], "yes");
    const wire = received.find((r) => r.url === "/api");
    assert.equal(wire.method, "POST");
    assert.equal(wire.headers["x-host"], "closure");
    assert.deepEqual(wire.body, Buffer.from([0, 255, 128]));
    await page.unroute("**/api");
    await page.route("**/api/binary", async (route) =>
      route.fulfill({ response: await route.fetch() }),
    );
    assert.deepEqual(
      await page.evaluate(async () =>
        Array.from(new Uint8Array(await (await fetch("/api/binary")).arrayBuffer())),
      ),
      [0, 255, 1, 128],
    );
  }));

test("host route errors reject fetch and fallback overrides reach older closures", async () =>
  fixture(async (page, origin, received) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    const order = [];
    const older = async (route, request) => {
      order.push("older");
      assert.equal(request.method(), "POST");
      assert.equal(request.postData(), "updated");
      await route.fulfill({ json: { ok: true } });
    };
    await page.route("**/api", older);
    await page.route("**/api", async (route) => {
      order.push("newer");
      await route.fallback({ method: "POST", postData: "updated" });
    });
    assert.deepEqual(await page.evaluate(async () => (await fetch("/api")).json()), { ok: true });
    assert.deepEqual(order, ["newer", "older"]);
    await page.unroute("**/api");
    await page.route("**/api", () => {
      throw new TypeError("host-route-marker");
    });
    await assert.rejects(
      page.evaluate(async () => (await fetch("/api")).text()),
      /host-route-marker/,
    );
    assert.equal(received.filter((r) => r.url === "/api").length, 0);
  }));

test("initial scripts, XHR, and child frames invoke the original host callbacks", async () =>
  fixture(async (page, origin) => {
    const calls = [];
    await page.route("**/api", async (route, request) => {
      calls.push(request.frame());
      await route.fulfill({ body: "host-" + calls.length });
    });
    await page.goto(origin, { waitUntil: "networkidle" });
    // The next full navigation executes the request during the initial parser turn.
    await page.route("**/initial-page", (route) => route.continue());
    await page.addInitScript(() => {
      if (location.pathname === "/initial-page")
        fetch("/api")
          .then((r) => r.text())
          .then((t) => (globalThis.initialRoute = t));
    });
    await page.goto(origin + "/initial-page", { waitUntil: "networkidle" });
    assert.equal(await page.evaluate(() => globalThis.initialRoute), "host-1");
    assert.equal(
      await page.evaluate(
        () =>
          new Promise((resolve, reject) => {
            const xhr = new XMLHttpRequest();
            xhr.open("GET", "/api");
            xhr.onload = () => resolve(xhr.responseText);
            xhr.onerror = () => reject(new Error("XHR failed"));
            xhr.send();
          }),
      ),
      "host-2",
    );
    await page.evaluate(() => {
      const frame = document.createElement("iframe");
      frame.src = "/child";
      document.body.appendChild(frame);
    });
    const child = page.frames().find((frame) => frame !== page.mainFrame());
    assert.ok(child);
    assert.equal(await child.evaluate(async () => (await fetch("/api")).text()), "host-3");
    assert.equal(calls[2], child);
  }));

test("concurrent routes and host callbacks can evaluate the page without deadlock", async () =>
  fixture(async (page, origin) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    let resolveSecond;
    const second = new Promise((resolve) => {
      resolveSecond = resolve;
    });
    await page.route("**/api/**", async (route, request) => {
      if (request.url().endsWith("/first")) await second;
      else resolveSecond();
      const title = await page.evaluate(() => document.title);
      await route.fulfill({ body: title + request.url().slice(request.url().lastIndexOf("/")) });
    });
    assert.deepEqual(
      await page.evaluate(async () =>
        Promise.all(["/api/first", "/api/second"].map(async (url) => (await fetch(url)).text())),
      ),
      ["host routes/first", "host routes/second"],
    );
  }));

test("continued requests retain cookies and context headers with host overrides", async () =>
  fixture(async (page, origin, received) => {
    await page.context().addCookies([{ name: "session", value: "host-cookie", url: origin }]);
    await page.context().setExtraHTTPHeaders({ "x-context": "shared" });
    await page.goto(origin, { waitUntil: "networkidle" });
    await page.route("**/api", (route) =>
      route.continue({
        url: origin + "/api/continued",
        method: "POST",
        postData: "wire-body",
        headers: { ...route.request().headers(), "x-route": "override" },
      }),
    );
    assert.equal(await page.evaluate(async () => (await fetch("/api")).text()), "upstream");
    const wire = received.find((r) => r.url === "/api/continued");
    assert.equal(wire.method, "POST");
    assert.equal(wire.body.toString(), "wire-body");
    assert.equal(wire.headers["x-route"], "override");
    assert.equal(wire.headers["x-context"], "shared");
    assert.equal(wire.headers.cookie, "session=host-cookie");
    await page.unroute("**/api");
    await page.route("**/api", async (route) => route.fulfill({ response: await route.fetch() }));
    assert.equal(await page.evaluate(async () => (await fetch("/api")).text()), "upstream");
    assert.equal(received.find((r) => r.url === "/api").headers.cookie, "session=host-cookie");
  }));

test("closing a page cancels paused routing without settling into another session", async () =>
  fixture(async (page, origin) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    let started;
    const entered = new Promise((resolve) => {
      started = resolve;
    });
    let release;
    const hold = new Promise((resolve) => {
      release = resolve;
    });
    await page.route("**/api", async (route) => {
      started();
      await hold;
      await route.fulfill({ body: "retired" });
    });
    const request = page.evaluate(async () => (await fetch("/api")).text());
    const failed = assert.rejects(request, /closed|navigated/i);
    await entered;
    await page.close();
    await failed;
    release();
  }));

test("aborted guest fetches do not resume a continued host request", async () =>
  fixture(async (page, origin, received) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    for (const action of ["continue", "fetch"]) {
      await page.unroute("**/api");
      await page.route("**/api", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 40));
        if (action === "fetch") await route.fetch();
        else await route.continue();
      });
      assert.equal(
        await page.evaluate(async () => {
          const controller = new AbortController();
          const request = fetch("/api", { signal: controller.signal }).then(
            () => "unexpected",
            (error) => error.name,
          );
          controller.abort();
          return request;
        }),
        "AbortError",
      );
    }
    assert.equal(received.filter((r) => r.url === "/api").length, 0);
  }));

test("CSP admission precedes host callbacks and fulfilled responses", async () =>
  fixture(async (page, origin, received) => {
    let calls = 0;
    await page.route("**/api", async (route) => {
      calls++;
      await route.fulfill({ body: "must not bypass CSP" });
    });
    await page.goto(origin + "/denied", { waitUntil: "networkidle" });
    await assert.rejects(
      page.evaluate(async () => (await fetch("/api")).text()),
      /Failed to fetch/,
    );
    assert.equal(calls, 0);
    assert.equal(received.filter((r) => r.url === "/api").length, 0);
  }));

test("route.fetch retains native HTTP authentication and redirect cookie handling", async () =>
  fixture(
    async (page, origin, received) => {
      await page.goto(origin, { waitUntil: "networkidle" });
      await page.route("**/api/**", async (route) => {
        const response = await route.fetch();
        if (route.request().url().endsWith("/redirect"))
          assert.equal(response.url(), origin + "/api/final");
        await route.fulfill({ response });
      });
      assert.equal(await page.evaluate(async () => (await fetch("/api/auth")).text()), "upstream");
      assert.ok(
        received.some(
          (r) => r.url === "/api/auth" && r.headers.authorization === "Basic dXNlcjpwYXNz",
        ),
      );
      assert.equal(
        await page.evaluate(async () => (await fetch("/api/redirect")).text()),
        "upstream",
      );
      assert.ok(
        received.find((r) => r.url === "/api/final").headers.cookie.includes("redirected=yes"),
      );
    },
    { httpCredentials: { username: "user", password: "pass" } },
  ));

test("initial module top-level awaits can complete host fulfillment and route.fetch", async () =>
  fixture(async (page, origin) => {
    for (const kind of ["fulfill", "fetch"]) {
      let hits = 0;
      await page.unroute("**/api");
      await page.route("**/api", async (route) => {
        hits++;
        const body =
          kind === "fetch" ? (await (await route.fetch()).text()) + " host" : "module-host";
        await route.fulfill({ body });
      });
      await page.goto(origin + "/module-page", { waitUntil: "networkidle", timeout: 5000 });
      assert.equal(
        await page.evaluate(() => globalThis.moduleRoute),
        kind === "fetch" ? "upstream host" : "module-host",
      );
      assert.equal(hits, 1);
    }
  }));

test("host fulfillment reads local files and HAR base64 without exposing host objects", async () =>
  fixture(async (page, origin) => {
    const dir = await mkdtemp(join(tmpdir(), "surf-route-"));
    const bytes = Buffer.from([0, 255, 3, 128]);
    const file = join(dir, "fixture.bin");
    const har = join(dir, "fixture.har");
    try {
      await writeFile(file, bytes);
      await writeFile(
        har,
        JSON.stringify({
          log: {
            entries: [
              {
                request: { url: origin + "/api/har" },
                response: {
                  status: 200,
                  content: {
                    mimeType: "application/octet-stream",
                    encoding: "base64",
                    text: bytes.toString("base64"),
                  },
                },
              },
            ],
          },
        }),
      );
      await page.route("**/api/file", (route) => {
        route.fulfill({ path: file, headers: { "Content-Type": "application/octet-stream" } });
      });
      await page.routeFromHAR(har);
      await page.goto(origin, { waitUntil: "networkidle" });
      for (const url of ["/api/file", "/api/har"])
        assert.deepEqual(
          await page.evaluate(
            async (url) => Array.from(new Uint8Array(await (await fetch(url)).arrayBuffer())),
            url,
          ),
          Array.from(bytes),
        );
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  }));

test("host route misuse reports errors rather than silently reaching upstream", async () =>
  fixture(async (page, origin, received) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    await page.route("**/api", () => {});
    await assert.rejects(
      page.evaluate(async () => fetch("/api")),
      /without handling/,
    );
    await page.unroute("**/api");
    await page.route("**/api", async (route) => {
      await assert.rejects(route.fetch({ maxRedirects: 4 }), /Unsupported route.fetch option/);
      await route.fulfill({ body: "once" });
      await assert.rejects(route.fulfill({ body: "twice" }), /already handled/);
    });
    assert.equal(await page.evaluate(async () => (await fetch("/api")).text()), "once");
    assert.equal(received.filter((r) => r.url === "/api").length, 0);
  }));

test("document replacement retires a paused route without poisoning the next request", async () =>
  fixture(async (page, origin) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    let entered;
    const started = new Promise((resolve) => {
      entered = resolve;
    });
    let release;
    const hold = new Promise((resolve) => {
      release = resolve;
    });
    let late;
    let calls = 0;
    await page.route("**/api", async (route) => {
      if (++calls === 1) {
        entered();
        await hold;
        late = route.fulfill({ body: "retired" });
        await late;
      } else await route.fulfill({ body: "fresh" });
    });
    const request = page.evaluate(async () => (await fetch("/api")).text());
    const rejected = assert.rejects(request, /replaced/);
    await started;
    await page.setContent("<body><p>replacement</p>");
    await rejected;
    release();
    await new Promise((resolve) => setImmediate(resolve));
    await assert.rejects(late, /replaced/);
    assert.equal(await page.evaluate(async () => (await fetch("/api")).text()), "fresh");
  }));

test("navigation deadlines bound a host route that never settles", async () =>
  fixture(async (page, origin) => {
    let entered = false;
    await page.route("**/api", async () => {
      entered = true;
      await new Promise(() => {});
    });
    await page.addInitScript(() => {
      fetch("/api").catch(() => {});
    });
    await assert.rejects(
      page.goto(origin, { waitUntil: "networkidle", timeout: 1500 }),
      (error) => error.name === "TimeoutError",
    );
    assert.equal(entered, true);
    assert.equal(page._live, false);
  }));

test("late host registrations service a new child's initial request in its actual frame", async () =>
  fixture(async (page, origin) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    let source;
    await page.route("**/api", async (route, request) => {
      source = request.frame();
      await route.fulfill({ body: "child-initial" });
    });
    await page.evaluate(() => {
      const frame = document.createElement("iframe");
      frame.src = "/child-route";
      document.body.appendChild(frame);
    });
    const child = page.frames().find((frame) => frame !== page.mainFrame());
    assert.ok(child);
    assert.equal(source, child);
    assert.equal(await child.evaluate(() => globalThis.initialChildResult), "child-initial");
  }));

test("route.fetch does not hold the actor while another frame releases its response body", async () =>
  fixture(async (page, origin, received) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      const frame = document.createElement("iframe");
      frame.src = "/child";
      document.body.appendChild(frame);
    });
    const child = page.frames().find((frame) => frame !== page.mainFrame());
    assert.ok(child);
    await page.route("**/api/**", async (route) => {
      if (route.request().url().endsWith("/held"))
        await route.fulfill({ response: await route.fetch() });
      else await route.fulfill({ body: "peer-ready" });
    });
    assert.deepEqual(
      await Promise.all([
        page.evaluate(async () => (await fetch("/api/held")).text()),
        child.evaluate(async () => {
          const text = await (await fetch("/api/peer")).text();
          await fetch("/release");
          return text;
        }),
      ]),
      ["released", "peer-ready"],
    );
    assert.equal(received.filter((r) => r.url === "/api/held").length, 1);
    assert.equal(received.filter((r) => r.url === "/api/peer").length, 0);
    assert.equal(received.filter((r) => r.url === "/release").length, 1);
  }));

test("guest abort timers still run while an asynchronous host route is paused", async () =>
  fixture(async (page, origin, received) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    await page.route("**/api", async () => {
      await new Promise(() => {});
    });
    assert.equal(
      await page.evaluate(async () => {
        const controller = new AbortController();
        const result = fetch("/api", { signal: controller.signal }).then(
          () => "unexpected",
          (error) => error.name,
        );
        setTimeout(() => controller.abort(), 10);
        return result;
      }),
      "AbortError",
    );
    assert.equal(received.filter((r) => r.url === "/api").length, 0);
  }));

test("unroute retains an already-paused request's original fallback chain", async () =>
  fixture(async (page, origin) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    let entered;
    const ready = new Promise((resolve) => {
      entered = resolve;
    });
    let release;
    const wait = new Promise((resolve) => {
      release = resolve;
    });
    await page.route("**/api", (route) => route.fulfill({ body: "original-chain" }));
    await page.route("**/api", async (route) => {
      entered();
      await wait;
      await route.fallback();
    });
    const request = page.evaluate(async () => (await fetch("/api")).text());
    await ready;
    await page.unroute("**/api");
    release();
    assert.equal(await request, "original-chain");
    assert.equal(await page.evaluate(async () => (await fetch("/api")).text()), "upstream");
  }));

test("retiring a frame cancels its paused host route while the owner remains usable", async () =>
  fixture(async (page, origin) => {
    await page.goto(origin, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      const frame = document.createElement("iframe");
      frame.src = "/child";
      document.body.appendChild(frame);
    });
    const child = page.frames().find((frame) => frame !== page.mainFrame());
    assert.ok(child);
    let entered;
    const ready = new Promise((resolve) => {
      entered = resolve;
    });
    await page.route("**/api", async () => {
      entered();
      await new Promise(() => {});
    });
    const request = child.evaluate(async () => (await fetch("/api")).text());
    const canceled = assert.rejects(request, /detached/i);
    await ready;
    await page.evaluate(() => document.querySelector("iframe").remove());
    await canceled;
    assert.equal(await page.evaluate(() => document.title), "host routes");
  }));
