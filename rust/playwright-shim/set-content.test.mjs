/* eslint-disable no-underscore-dangle */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let shim;
let native;
try {
  shim = await import("./index.mjs");
  native = (await import("../crates/turbo-surf-napi/index.js")).default;
} catch (error) {
  console.log("setContent: native addon not built, skipping —", error.message);
  process.exit(0);
}
const { newPage } = shim;

test("setContent preserves the live realm and upgrades attached custom elements (#386)", async () => {
  const page = newPage();
  const calls = [];
  try {
    await page.setContent(
      '<div id="target">old</div><iframe name="before" srcdoc="<b>old frame</b>"></iframe>',
    );
    await page._ensureLive();
    const session = page._session;
    const mainFrame = page.mainFrame();
    const previousFrame = page.frame({ name: "before" });
    await page.exposeFunction("replacementHost", (value) => calls.push(value));
    await page.evaluate(() => {
      globalThis.retained = {
        document,
        prototype: HTMLElement.prototype,
        node: document.querySelector("#target"),
      };
      globalThis.replacementReactions = [];
      document.addEventListener("retained-listener", () =>
        document.body.setAttribute("data-listener", "yes"),
      );
      customElements.define(
        "replacement-check",
        class extends HTMLElement {
          constructor() {
            super();
            replacementReactions.push([
              "construct",
              document.querySelector("#target")?.textContent,
              this.ownerDocument === document,
            ]);
          }
          connectedCallback() {
            replacementReactions.push([
              "connected",
              document.querySelector("#target")?.textContent,
              this.isConnected,
            ]);
          }
        },
      );
    });
    await page.setContent(
      '<!doctype html><body><div id="target">new</div><replacement-check></replacement-check><iframe name="after" srcdoc="<b>new frame</b>"></iframe><script>globalThis.replacementMarker=44;document.dispatchEvent(new Event("retained-listener"));replacementHost("replacement")</script></body>',
    );
    assert.equal(await page.evaluate(() => document.querySelector("#target").textContent), "new");
    assert.equal(await page.locator("#target").textContent(), "new");
    assert.equal(page._session, session);
    assert.equal(page.mainFrame(), mainFrame);
    assert.equal(previousFrame.isDetached(), true);
    const currentFrame = page.frame({ name: "after" });
    assert.ok(currentFrame);
    assert.equal(page.frame({ name: "after" }), currentFrame);
    assert.deepEqual(calls, ["replacement"]);
    assert.deepEqual(
      await page.evaluate(() => ({
        marker: replacementMarker,
        document: retained.document === document,
        prototype: retained.prototype === HTMLElement.prototype,
        detached: !retained.node.isConnected && retained.node.ownerDocument === document,
        custom:
          document.querySelector("replacement-check") instanceof
          customElements.get("replacement-check"),
        listener: document.body.getAttribute("data-listener"),
        reactions: replacementReactions,
      })),
      {
        marker: 44,
        document: true,
        prototype: true,
        detached: true,
        custom: true,
        listener: "yes",
        reactions: [
          ["construct", "new", true],
          ["connected", "new", true],
        ],
      },
    );
    await page.setContent('<p id="again">again</p><script>globalThis.replacementMarker++</script>');
    assert.equal(await page.evaluate(() => replacementMarker), 45);
    assert.equal(await page.locator("#again").textContent(), "again");
  } finally {
    await page.close();
  }
});

test("setContent uses parser CSP resource ordering and load completion (#386)", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push(req.url);
    if (req.url === "/blocking.js" || req.url === "/deferred.js") {
      res.setHeader("content-type", "text/javascript");
      setTimeout(() => res.end(`order.push(${JSON.stringify(req.url)})`), 15);
    } else if (req.url === "/sheet.css") {
      res.setHeader("content-type", "text/css");
      setTimeout(() => res.end("#target{color:rgb(11,22,33)}"), 40);
    } else {
      res.setHeader("content-type", "text/html");
      res.end("<body>old</body>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      globalThis.order = [];
      document.addEventListener("DOMContentLoaded", () => order.push("dcl"));
      window.addEventListener("load", () => order.push("load"));
    });
    await page.setContent(
      '<head><link rel="stylesheet" href="/sheet.css"><script src="/blocking.js"></script><script>order.push("inline")</script><script defer src="/deferred.js"></script></head><body><div id="target">new</div></body>',
    );
    assert.deepEqual(await page.evaluate(() => order), [
      "/blocking.js",
      "inline",
      "/deferred.js",
      "dcl",
      "load",
    ]);
    assert.equal(await page.evaluate(() => document.readyState), "complete");
    assert.equal(
      await page.evaluate(() => getComputedStyle(document.querySelector("#target")).color),
      "rgb(11, 22, 33)",
    );
    for (const path of ["/blocking.js", "/deferred.js", "/sheet.css"])
      assert.equal(seen.filter((entry) => entry === path).length, 1);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("setContent domcontentloaded returns before load resources and later completes (#386)", async () => {
  let response;
  let background;
  let requested;
  const requestSeen = new Promise((resolve) => {
    requested = resolve;
  });
  const server = createServer((req, res) => {
    if (req.url === "/background") {
      background = res;
    } else if (req.url === "/held.css") {
      response = res;
      requested();
    } else {
      res.setHeader("content-type", "text/html");
      res.end("<body>old</body>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  let timer;
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      globalThis.loadCount = 0;
      window.addEventListener("load", () => loadCount++);
      document.addEventListener("DOMContentLoaded", () =>
        document.body.setAttribute("data-dcl", document.readyState),
      );
    });
    await native.liveEvalEngineExpression(page._session, "(fetch('/background'), 'started')");
    const operation = page.setContent(
      '<link rel="stylesheet" href="/held.css"><body id="new">new</body>',
      { waitUntil: "domcontentloaded", timeout: 5000 },
    );
    await requestSeen;
    assert.equal(
      await Promise.race([
        operation.then(() => true),
        new Promise((resolve) => {
          timer = setTimeout(() => resolve(false), 3000);
        }),
      ]),
      true,
    );
    clearTimeout(timer);
    assert.match(page._html, /data-dcl="interactive"/);
    response.writeHead(200, { "content-type": "text/css" });
    response.end("body{color:rgb(11,22,33)}");
    await page.waitForLoadState("load");
    assert.deepEqual(
      JSON.parse(
        await native.liveEvalEngineExpression(
          page._session,
          "JSON.stringify([document.readyState,loadCount])",
        ),
      ),
      ["complete", 1],
    );
  } finally {
    clearTimeout(timer);
    if (response && !response.writableEnded) response.end("");
    if (background && !background.writableEnded) background.end("");
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("setContent preserves installed CSP and rejects load deadlines without replacing the realm (#386)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/slow.css") {
      res.setHeader("content-type", "text/css");
      setTimeout(() => res.end("body{color:red}"), 120);
    } else {
      if (req.url !== "/plain")
        res.setHeader("content-security-policy", "script-src 'nonce-ok'; style-src 'none'");
      res.end("<body>old</body>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
    const session = page._session;
    await page.evaluate(() => {
      globalThis.persisted = { document };
    });
    await page.setContent(
      '<style>body{color:red}</style><button id="button" onclick="globalThis.disallowedHandler=true">press</button><script>globalThis.disallowed=true</script><script nonce="ok">globalThis.allowed=44</script>',
    );
    await page.locator("#button").click();
    assert.deepEqual(
      await page.evaluate(() => [
        allowed,
        typeof disallowed,
        typeof disallowedHandler,
        persisted.document === document,
      ]),
      [44, "undefined", "undefined", true],
    );
    assert.equal(page._session, session);
    await assert.rejects(page.setContent("ignored", { waitUntil: "commit" }), /waitUntil/);
    // A separate unrestricted document exercises the actual resource deadline.
    const plain = newPage();
    try {
      await plain.goto(`http://127.0.0.1:${server.address().port}/plain`, { waitUntil: "commit" });
      await plain._ensureLive();
      const retained = plain._session;
      await assert.rejects(
        plain.setContent('<link rel="stylesheet" href="/slow.css"><p>new</p>', { timeout: 10 }),
        (error) => error.name === "TimeoutError",
      );
      assert.equal(plain._session, retained);
    } finally {
      await plain.close();
    }
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("setContent clears old intrinsic metadata before same-URL image reloads (#386)", async () => {
  const svg = (width) =>
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="400"><rect width="100%" height="100%" fill="red"/></svg>`;
  const held = [];
  let imageRequests = 0;
  let release = false;
  const server = createServer((req, res) => {
    if (req.url.endsWith(".svg")) {
      res.setHeader("content-type", "image/svg+xml");
      res.setHeader("cache-control", "no-store");
      if (req.url === "/image.svg" && imageRequests++ === 0) res.end(svg(600));
      else {
        const width = req.url === "/image.svg" ? 900 : 100;
        if (release) res.end(svg(width));
        else held.push({ res, width });
      }
    } else {
      res.setHeader("content-type", "text/html");
      res.end("<body>old</body>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  const metadata = async () =>
    JSON.parse(
      await native.liveEvalEngineExpression(
        page._session,
        "JSON.stringify(__tsNs.screenshotImageMetadata())",
      ),
    );
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
    await page.setContent('<img src="/image.svg">');
    assert.deepEqual(
      Object.values(await metadata()).map((entry) => entry.width),
      [600],
    );
    const session = page._session;
    await page.setContent('<img id="same" src="/image.svg"><img src="/fresh.svg">', {
      waitUntil: "domcontentloaded",
    });
    assert.equal(page._session, session);
    assert.deepEqual(await metadata(), {});
    release = true;
    for (const { res, width } of held) res.end(svg(width));
    await page.waitForLoadState("load");
    assert.deepEqual(
      Object.values(await metadata())
        .map((entry) => entry.width)
        .sort((a, b) => a - b),
      [100, 900],
    );
  } finally {
    release = true;
    for (const { res } of held) if (!res.writableEnded) res.end("");
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("setContent retires old resources while activating replacement frames and resources (#386)", async () => {
  const held = [],
    seen = [];
  const server = createServer((req, res) => {
    seen.push(req.url);
    if (req.url === "/old.svg" || req.url === "/old.js") held.push({ url: req.url, res });
    else if (req.url === "/new.svg") {
      res.setHeader("content-type", "image/svg+xml");
      res.end('<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"/>');
    } else if (req.url === "/new.js") {
      res.setHeader("content-type", "text/javascript");
      res.end("globalThis.freshContentScript=44");
    } else res.end("<body>old</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      globalThis.contentEvents = [];
      document.addEventListener("DOMContentLoaded", () => contentEvents.push("dcl"));
      window.addEventListener("load", () => contentEvents.push("load"));
    });
    await page.setContent(
      '<iframe name="old" srcdoc="<b>old</b>"></iframe><img src="/old.svg"><script async src="/old.js"></script>',
      { waitUntil: "domcontentloaded" },
    );
    assert.deepEqual(held.map(({ url }) => url).sort(), ["/old.js", "/old.svg"]);
    const previous = page.frame({ name: "old" });
    await page.setContent(
      `<p>replacement</p><iframe name="new" srcdoc="<script>globalThis.childMarker=44</script>"></iframe><img src="/new.svg"><script src="/new.js"></script>`,
      { timeout: 5000 },
    );
    assert.equal(previous.isDetached(), true);
    for (const { url, res } of held) {
      res.setHeader("content-type", url.endsWith(".js") ? "text/javascript" : "image/svg+xml");
      res.end(
        url.endsWith(".js")
          ? "globalThis.retiredContentScript=true"
          : '<svg xmlns="http://www.w3.org/2000/svg" width="700" height="400"/>',
      );
    }
    assert.deepEqual(
      await page.evaluate(() => [
        typeof retiredContentScript,
        freshContentScript,
        document.readyState,
        contentEvents,
      ]),
      ["undefined", 44, "complete", ["dcl", "dcl", "load"]],
    );
    assert.equal(await page.frame({ name: "new" }).evaluate(() => childMarker), 44);
    const metadata = JSON.parse(
      await native.liveEvalEngineExpression(
        page._session,
        "JSON.stringify(__tsNs.screenshotImageMetadata())",
      ),
    );
    assert.deepEqual(
      Object.values(metadata).map(({ width }) => width),
      [120],
    );
    for (const path of ["/new.svg", "/new.js"])
      assert.equal(seen.filter((url) => url === path).length, 1);
  } finally {
    for (const { res } of held) if (!res.writableEnded) res.end("");
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("setContent activates imported srcdoc frames and retires replaced children (#386)", async () => {
  const page = newPage();
  try {
    await page.setContent('<iframe name="before" srcdoc="<b>before</b>"></iframe>');
    const before = page.frame({ name: "before" });
    const main = page.mainFrame();
    await page.setContent(
      `<iframe name="after" srcdoc="<b id='child'>child</b><script>globalThis.childMarker=44</script>"></iframe>`,
    );
    const after = page.frame({ name: "after" });
    assert.equal(page.mainFrame(), main);
    assert.equal(before.isDetached(), true);
    assert.equal(page.frame({ name: "after" }), after);
    assert.deepEqual(
      await after.evaluate(() => [
        document.querySelector("#child")?.textContent,
        globalThis.childMarker,
      ]),
      ["child", 44],
    );
  } finally {
    await page.close();
  }
});

test("replacement parser preparation stays ordered beside custom-element dynamic scripts (#386)", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push(req.url);
    if (req.url.endsWith(".js")) {
      res.setHeader("content-type", "text/javascript");
      setTimeout(
        () => res.end(`order.push(${JSON.stringify(req.url)})`),
        req.url === "/blocking.js" ? 30 : 5,
      );
    } else res.end("<body>old</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      globalThis.order = [];
      document.addEventListener("DOMContentLoaded", () => order.push("dcl"));
      window.addEventListener("load", () => order.push("load"));
      customElements.define(
        "dynamic-resource-check",
        class extends HTMLElement {
          connectedCallback() {
            const inline = document.createElement("script");
            inline.textContent = "order.push('ce-inline')";
            document.head.append(inline);
            const external = document.createElement("script");
            external.src = "/dynamic.js";
            external.async = false;
            document.head.append(external);
          }
        },
      );
    });
    await page.setContent(
      '<dynamic-resource-check></dynamic-resource-check><script src="/blocking.js"></script><script>order.push("parser-inline")</script><script defer src="/deferred.js"></script>',
    );
    const order = await page.evaluate(() => globalThis.order);
    assert.deepEqual(
      order.filter((entry) => !["ce-inline", "/dynamic.js"].includes(entry)),
      ["/blocking.js", "parser-inline", "/deferred.js", "dcl", "load"],
    );
    assert.equal(order.filter((entry) => entry === "ce-inline").length, 1);
    assert.equal(order.filter((entry) => entry === "/dynamic.js").length, 1);
    assert.ok(order.indexOf("/dynamic.js") < order.indexOf("load"));
    for (const path of ["/dynamic.js", "/blocking.js", "/deferred.js"])
      assert.equal(seen.filter((url) => url === path).length, 1);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});
