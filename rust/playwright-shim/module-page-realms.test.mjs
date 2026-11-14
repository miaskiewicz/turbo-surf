import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let BrowserContext;
try {
  ({ BrowserContext } = await import("./index.mjs"));
} catch (error) {
  if (!String(error).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(handler) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    base: `http://127.0.0.1:${server.address().port}`,
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}

test("replacement native Page modules execute in their current document", async () => {
  const host = await fixture((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(
      '<body><script type="module">const owner=await Promise.resolve(location.pathname);globalThis.moduleOwner=owner;document.body.dataset.moduleOwner=owner;</script></body>',
    );
  });
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    for (const path of ["/first", "/next"]) {
      await page.goto(host.base + path, { waitUntil: "networkidle" });
      assert.deepEqual(
        await page.evaluate(() => ({
          location: location.pathname,
          module: globalThis.moduleOwner,
          dom: document.body.dataset.moduleOwner,
        })),
        { location: path, module: path, dom: path },
      );
    }
  } finally {
    await context.close();
    await host.close();
  }
});

test("a cross-origin native Page imports its own graph but cannot reuse another document's inline source", async () => {
  const root = await fixture((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(
      '<body><script type="module">globalThis.inlineSource=import.meta.url;export const secret="root-only";</script></body>',
    );
  });
  const peer = await fixture((request, response) => {
    if (["/own.mjs", "/after-close.mjs"].includes(request.url)) {
      response.setHeader("content-type", "application/javascript");
      response.end('document.body.dataset.moduleOwner="peer";export const owner="peer";');
    } else {
      response.setHeader("content-type", "text/html");
      response.end("<body>Peer document</body>");
    }
  });
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    await page.goto(root.base, { waitUntil: "networkidle" });
    const source = await page.evaluate(() => globalThis.inlineSource);
    assert.equal(typeof source, "string");
    assert.equal(
      await page.evaluate(() =>
        import("ext:core/01_core.js").then(
          () => "infrastructure-module-exposed",
          (error) => error.name,
        ),
      ),
      "TypeError",
    );
    const popup = page.waitForEvent("popup");
    await page.evaluate((url) => {
      window.open(url);
    }, peer.base);
    const child = await popup;
    await child.waitForURL(peer.base + "/");
    await child.waitForLoadState("load");
    assert.equal(
      await child.evaluate(
        (url) => import(url).then((module) => module.owner),
        peer.base + "/own.mjs",
      ),
      "peer",
    );
    assert.equal(await child.locator("body").getAttribute("data-module-owner"), "peer");
    assert.deepEqual(
      await child.evaluate(
        (url) =>
          import(url).then(
            (module) => ({ leaked: module.secret, rejected: false }),
            (error) => ({
              leaked: null,
              rejected:
                error.name === "TypeError" &&
                error instanceof TypeError &&
                error.constructor === TypeError,
            }),
          ),
        source,
      ),
      { leaked: null, rejected: true },
    );
    assert.equal(await page.evaluate(() => typeof globalThis.secret), "undefined");
    await page.close();
    assert.equal(
      await child.evaluate(
        (url) => import(url).then((module) => module.owner),
        peer.base + "/after-close.mjs",
      ),
      "peer",
    );
  } finally {
    await context.close();
    await root.close();
    await peer.close();
  }
});

test("native Page module credentials use the calling document's nonce policy", async () => {
  const requests = [];
  const host = await fixture((request, response) => {
    requests.push(request.url);
    if (request.url.endsWith(".mjs")) {
      response.setHeader("content-type", "application/javascript");
      response.setHeader("cache-control", "no-store");
      response.end("document.body.dataset.moduleOwner=location.pathname;");
      return;
    }
    response.setHeader("content-type", "text/html");
    response.setHeader("content-security-policy", "script-src 'nonce-guest';");
    response.end('<body><script type="module" nonce="guest" src="/allowed.mjs"></script></body>');
  });
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    for (const path of ["/first", "/next"]) {
      await page.goto(host.base + path, { waitUntil: "networkidle" });
      assert.equal(await page.locator("body").getAttribute("data-module-owner"), path);
      assert.equal(
        await page.evaluate(
          (url) =>
            import(url).then(
              () => "unauthorized-module-loaded",
              (error) => error.name,
            ),
          host.base + "/blocked.mjs",
        ),
        "TypeError",
      );
    }
    assert.equal(requests.filter((url) => url === "/allowed.mjs").length, 2);
    assert.equal(requests.filter((url) => url === "/blocked.mjs").length, 0);
  } finally {
    await context.close();
    await host.close();
  }
});

test("borrowed and adopted module scripts use their actual target Page policy", async () => {
  const requests = [];
  const host = await fixture((request, response) => {
    requests.push(request.url);
    response.setHeader("cache-control", "no-store");
    if (request.url === "/borrowed.mjs") {
      response.setHeader("content-type", "application/javascript");
      response.end(
        "globalThis.externalOwner=location.pathname;document.body.dataset.externalOwner=location.pathname;",
      );
      return;
    }
    response.setHeader("content-type", "text/html");
    const nonce = request.url === "/child" ? "child" : "root";
    response.setHeader("content-security-policy", `script-src 'nonce-${nonce}';`);
    response.end("<body>Document policy fixture</body>");
  });
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    await page.goto(host.base + "/root", { waitUntil: "networkidle" });
    const popup = page.waitForEvent("popup");
    await page.evaluate((url) => {
      globalThis.moduleChild = window.open(url);
    }, host.base + "/child");
    const child = await popup;
    await child.waitForURL(host.base + "/child");
    await child.waitForLoadState("load");
    assert.deepEqual(
      await page.evaluate(async (url) => {
        const childDocument = moduleChild.document;
        const load = (script) =>
          new Promise((resolve, reject) => {
            script.addEventListener("load", resolve, { once: true });
            script.addEventListener(
              "error",
              () => reject(new Error("Target Page module rejected")),
              { once: true },
            );
            childDocument.body.appendChild(script);
          });
        const inlineReady = new Promise((resolve) => {
          globalThis.moduleInlineReady = resolve;
        });
        const adopted = document.createElement("script");
        adopted.type = "module";
        adopted.nonce = "child";
        adopted.textContent =
          "globalThis.inlineOwner=location.pathname;document.body.dataset.inlineOwner=location.pathname;opener.moduleInlineReady();";
        childDocument.body.appendChild(adopted);
        await inlineReady;
        delete globalThis.moduleInlineReady;
        const borrowed = childDocument.createElement("script");
        borrowed.type = "module";
        borrowed.nonce = "child";
        borrowed.src = url;
        await load(borrowed);
        return [adopted.ownerDocument === childDocument, borrowed.ownerDocument === childDocument];
      }, host.base + "/borrowed.mjs"),
      [true, true],
    );
    assert.deepEqual(await child.evaluate(() => [location.pathname, inlineOwner, externalOwner]), [
      "/child",
      "/child",
      "/child",
    ]);
    assert.deepEqual(await page.evaluate(() => [typeof inlineOwner, typeof externalOwner]), [
      "undefined",
      "undefined",
    ]);
    assert.equal(requests.filter((url) => url === "/borrowed.mjs").length, 1);
  } finally {
    await context.close();
    await host.close();
  }
});
