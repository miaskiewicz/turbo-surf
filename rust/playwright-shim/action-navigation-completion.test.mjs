import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("action navigation: native addon not built, skipping");
  process.exit(0);
}

async function withForm(kind, run) {
  let documents = 0;
  const server = createServer((request, response) => {
    documents++;
    response.setHeader("content-type", "text/html");
    if (request.url.startsWith("/result")) {
      response.end(`<p id="complete">${request.url}</p>`);
      return;
    }
    const form =
      '<form action="/result" target="_top"><input name="q" value="keyboard"><button>Go</button></form>';
    response.end(
      kind === "frame"
        ? `<iframe srcdoc="${form.replaceAll("&", "&amp;").replaceAll('"', "&quot;")}"></iframe>`
        : form,
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
    await run(page);
    assert.equal(documents, 2, "one native form submission must fetch one replacement");
    assert.match(page.url(), /\/result\?q=keyboard$/);
    assert.match(await page.content(), /id="complete"/);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("keyboard Enter awaits its native form replacement", { timeout: 15000 }, async () => {
  await withForm("root", async (page) => {
    await page.locator("input").press("Enter");
  });
});

test("frame click awaits its native top-level form replacement", { timeout: 15000 }, async () => {
  await withForm("frame", async (page) => {
    await page.frameLocator("iframe").locator("button").click();
  });
});

function bounded(promise, message) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(message)), 3000);
    }),
  ]).finally(() => clearTimeout(timer));
}

test(
  "ordinary evaluate does not await an unrelated popup's held load",
  { timeout: 15000 },
  async () => {
    let entered;
    const held = new Promise((resolve) => {
      entered = resolve;
    });
    let script;
    const server = createServer((request, response) => {
      if (request.url === "/held.js") {
        script = response;
        response.setHeader("content-type", "application/javascript");
        response.write("/* held child script */");
        entered();
      } else if (request.url === "/popup") {
        response.end('<script src="/held.js"></script>');
      } else {
        response.end("<button id=\"open\" onclick=\"window.open('/popup','child')\">Open</button>");
      }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const context = new shim.BrowserContext();
    try {
      const page = await context.newPage();
      await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
      const opened = page.waitForEvent("popup", { timeout: 3000 });
      await page.locator("#open").click();
      const popup = await opened;
      await bounded(held, "popup script was not requested");
      assert.equal(
        await bounded(
          page.evaluate(() => 42),
          "ordinary evaluate waited for the popup load",
        ),
        42,
      );
      assert.equal(popup._readEngine("return globalThis.childFinished === true;"), false);
      script.end("globalThis.childFinished=true;");
    } finally {
      script?.end();
      await context.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

for (const { name, base, policy, expected, blank, parentBase } of [
  { name: "srcdoc fallback", expected: "/owner/next?q=inherited" },
  {
    name: "srcdoc inherits parent document base",
    parentBase: "/parent-base/",
    expected: "/parent-base/next?q=inherited",
  },
  {
    name: "srcdoc relative child base uses accepted parent base",
    parentBase: "/parent-base/",
    base: "child/",
    expected: "/parent-base/child/next?q=inherited",
  },
  {
    name: "srcdoc CSP-rejected parent base uses native fallback",
    parentBase: "/rejected-parent/",
    policy: "base-uri 'none'",
    expected: "/owner/next?q=inherited",
  },
  { name: "srcdoc explicit base", base: "/approved/", expected: "/approved/next?q=inherited" },
  {
    name: "srcdoc CSP-rejected base",
    base: "/rejected/",
    policy: "base-uri 'none'",
    expected: "/owner/next?q=inherited",
  },
  { name: "initial about:blank fallback", blank: true, expected: "/owner/next?q=inherited" },
]) {
  test(`native inherited form resolution: ${name}`, { timeout: 15000 }, async () => {
    const requests = [];
    const form = `${base ? `<base href="${base}">` : ""}<form action="next" target="_top"><input name="q" value="inherited"><button>Submit</button></form>`;
    const server = createServer((request, response) => {
      requests.push(request.url);
      response.setHeader("content-type", "text/html");
      if (policy) response.setHeader("content-security-policy", policy);
      response.end(
        request.url === "/owner/index"
          ? blank
            ? `${parentBase ? `<base href="${parentBase}">` : ""}<iframe></iframe>`
            : `${parentBase ? `<base href="${parentBase}">` : ""}<iframe srcdoc="${form.replaceAll("&", "&amp;").replaceAll('"', "&quot;")}"></iframe>`
          : '<p id="complete">replacement</p>',
      );
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const page = shim.newPage();
    try {
      await page.goto(`http://127.0.0.1:${server.address().port}/owner/index`, {
        waitUntil: "networkidle",
      });
      await page.evaluate(() => {
        Object.defineProperty(document, "baseURI", { get: () => "http://127.0.0.1/parent-spoof/" });
      });
      const child = page.frames().find((frame) => frame !== page);
      if (blank)
        await child.evaluate((html) => {
          document.body.innerHTML = html;
        }, form);
      await child.evaluate(() => {
        Object.defineProperty(document, "baseURI", { get: () => "http://127.0.0.1/spoof/" });
      });
      await child.locator("button").click();
      assert.deepEqual(requests, ["/owner/index", expected]);
      assert.equal(new URL(page.url()).pathname + new URL(page.url()).search, expected);
      assert.match(await page.content(), /id="complete"/);
    } finally {
      await page.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  });
}

test(
  "native inherited form resolution: about:blank popup uses its captured opener base",
  { timeout: 15000 },
  async () => {
    const requests = [];
    const server = createServer((request, response) => {
      requests.push(request.url);
      response.setHeader("content-type", "text/html");
      response.end(
        request.url === "/owner/index"
          ? '<base href="/parent-base/"><p>opener</p>'
          : '<p id="complete">popup replacement</p>',
      );
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const page = shim.newPage();
    let popup;
    try {
      await page.goto(`http://127.0.0.1:${server.address().port}/owner/index`, {
        waitUntil: "networkidle",
      });
      const opened = page.waitForEvent("popup", { timeout: 3000 });
      await page.evaluate(() => {
        window.open("about:blank");
      });
      popup = await opened;
      await popup.setContent(
        '<form action="next" target="_top"><input name="q" value="popup"><button>Submit</button></form>',
      );
      await popup.evaluate(() => {
        Object.defineProperty(document, "baseURI", { get: () => "http://127.0.0.1/spoof/" });
      });
      await popup.locator("button").click();
      assert.deepEqual(requests, ["/owner/index", "/parent-base/next?q=popup"]);
      assert.equal(new URL(popup.url()).pathname, "/parent-base/next");
      assert.match(await popup.content(), /id="complete"/);
    } finally {
      await popup?.close();
      await page.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

test(
  "frame Enter finishes its raw key sequence after native top navigation",
  { timeout: 15000 },
  async () => {
    await withForm("frame", async (page) => {
      await page.frameLocator("iframe").locator("input").press("Enter");
    });
  },
);

test(
  "frame Enter follows document identity across same-URL top navigation",
  { timeout: 15000 },
  async () => {
    const requests = [];
    const server = createServer((request, response) => {
      requests.push(request.method);
      response.setHeader("content-type", "text/html");
      const form =
        '<form method="post" action="/" target="_top"><input name="q" value="same"><button>Submit</button></form>';
      response.end(
        request.method === "POST"
          ? '<p id="complete">same URL replacement</p>'
          : `<iframe srcdoc="${form.replaceAll("&", "&amp;").replaceAll('"', "&quot;")}"></iframe>`,
      );
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const page = shim.newPage();
    try {
      const url = `http://127.0.0.1:${server.address().port}/`;
      await page.goto(url, { waitUntil: "networkidle" });
      const oldFrame = page.frames().find((frame) => frame !== page);
      await oldFrame.locator("input").press("Enter");
      assert.deepEqual(requests, ["GET", "POST"]);
      assert.equal(page.url(), url);
      assert.match(await page.content(), /id="complete"/);
      await assert.rejects(oldFrame.locator("input").press("x"), /detached/);
    } finally {
      await page.close();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);

for (const chord of ["Enter", "Shift+Enter"]) {
  test(
    `frame ${chord} sends every keyup before its held top replacement`,
    { timeout: 15000 },
    async () => {
      const keyups = [];
      const expected =
        chord === "Enter"
          ? [["Enter", false]]
          : [
              ["Enter", true],
              ["Shift", false],
            ];
      let held;
      let mainRequested;
      const received = new Promise((resolve) => {
        mainRequested = resolve;
      });
      let keysReceived;
      const keys = new Promise((resolve) => {
        keysReceived = resolve;
      });
      const server = createServer((request, response) => {
        const url = new URL(request.url, "http://localhost");
        response.setHeader("content-type", "text/html");
        if (url.pathname === "/keyup") {
          keyups.push([url.searchParams.get("key"), url.searchParams.get("shift") === "true"]);
          response.end("ok");
          if (keyups.length === expected.length) keysReceived();
        } else if (request.method === "POST") {
          held = response;
          response.write('<p id="complete">held replacement');
          mainRequested();
        } else {
          const form = `<form method="post" action="/" target="_top"><input onkeyup="fetch('/keyup?key='+encodeURIComponent(event.key)+'&shift='+event.shiftKey)"><button>Submit</button></form>`;
          response.end(
            `<iframe srcdoc="${form.replaceAll("&", "&amp;").replaceAll('"', "&quot;")}"></iframe>`,
          );
        }
      });
      await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
      const page = shim.newPage();
      let action;
      try {
        await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
        const frame = page.frames().find((item) => item !== page);
        await frame.locator("input").focus();
        let settled = false;
        action = frame
          .locator("input")
          .press(chord)
          .finally(() => {
            settled = true;
          });
        action.catch(() => {});
        await bounded(received, "frame form never reached the real server");
        await bounded(keys, "frame press skipped or delayed a physical keyup");
        assert.equal(settled, false, "frame press must wait for its own document commit");
        assert.deepEqual(keyups, expected);
        held.end("</p>");
        await bounded(action, "frame press did not finish after main commit");
        const current = page._readEngine("return JSON.parse(__tsNs.requestFrameSnapshot());");
        assert.equal(
          frame.isDetached(),
          true,
          JSON.stringify({ mirror: page._requestContextId, native: current.id }),
        );
        assert.equal(
          page._requestContextId,
          current.id,
          "the committed root document mirror must be current",
        );
        await assert.rejects(frame.locator("input").press("x"), /detached/);
      } finally {
        held?.end();
        await action?.catch(() => {});
        await page.close();
        server.closeAllConnections();
        await new Promise((resolve) => server.close(resolve));
      }
    },
  );
}

test(
  "frame automation identities never invoke authored WeakMap hooks",
  { timeout: 15000 },
  async () => {
    const page = shim.newPage();
    try {
      await page.addInitScript(() => {
        const Original = WeakMap,
          get = Original.prototype.get,
          set = Original.prototype.set;
        const apply = Reflect.apply;
        globalThis.frameMapHooks = { constructor: 0, get: 0, set: 0 };
        function Replacement(...values) {
          frameMapHooks.constructor++;
          return new Original(...values);
        }
        Replacement.prototype = Original.prototype;
        globalThis.WeakMap = Replacement;
        Original.prototype.get = function (value) {
          frameMapHooks.get++;
          return apply(get, this, [value]);
        };
        Original.prototype.set = function (key, value) {
          frameMapHooks.set++;
          return apply(set, this, [key, value]);
        };
      });
      await page.setContent('<iframe srcdoc="&lt;input&gt;"></iframe>');
      await page._ensureLive();
      await page.frameLocator("iframe").locator("input").press("x");
      assert.equal(await page.frameLocator("iframe").locator("input").inputValue(), "x");
      assert.deepEqual(
        await page.evaluate(async () => {
          const face = new FontFace("map-private", "url(data:application/font-woff;base64,AA==)");
          const ready = await document.fonts.ready;
          return {
            setBrand: document.fonts instanceof FontFaceSet,
            faceBrand: face instanceof FontFace,
            family: face.family,
            faceStatus: face.status,
            size: document.fonts.size,
            ready: ready === document.fonts,
          };
        }),
        {
          setBrand: true,
          faceBrand: true,
          family: "map-private",
          faceStatus: "unloaded",
          size: 0,
          ready: true,
        },
      );
      assert.deepEqual(await page.evaluate(() => frameMapHooks), {
        constructor: 0,
        get: 0,
        set: 0,
      });
    } finally {
      await page.close();
    }
  },
);

test(
  "a same-path replacement cannot forge an old frame identity through WeakMap hooks",
  { timeout: 15000 },
  async () => {
    const page = shim.newPage();
    try {
      await page.setContent('<iframe srcdoc="&lt;input&gt;"></iframe>');
      await page._ensureLive();
      const oldFrame = page.frames().find((frame) => frame !== page);
      page._readEngine(
        `document.querySelector('iframe').outerHTML = '<iframe srcdoc="&lt;input onfocus=&quot;globalThis.foreignFocus=true&quot; onkeydown=&quot;globalThis.foreignKeys=true&quot;&gt;"></iframe>'; return null;`,
      );
      const token = oldFrame._node;
      page._readEngine(`(() => {
      const Original = WeakMap, get = Original.prototype.get, set = Original.prototype.set;
      globalThis.frameMapHooks = { constructor: 0, get: 0, set: 0 };
      function Replacement(...values) { frameMapHooks.constructor++; return new Original(...values); }
      Replacement.prototype = Original.prototype;
      globalThis.WeakMap = Replacement;
      Original.prototype.get = function () { frameMapHooks.get++; return ${JSON.stringify(token)}; };
      Original.prototype.set = function () { frameMapHooks.set++; return this; };
      globalThis.restoreFrameMap = () => { globalThis.WeakMap = Original; Original.prototype.get = get; Original.prototype.set = set; };
    })(); return null;`);
      await assert.rejects(oldFrame.locator("input").press("x"));
      const hooks = await page.evaluate(() => {
        const value = { ...frameMapHooks };
        restoreFrameMap();
        return value;
      });
      assert.deepEqual(hooks, { constructor: 0, get: 0, set: 0 });
      const current = page.frames().find((frame) => frame !== page);
      assert.equal(await current.locator("input").inputValue(), "");
      assert.deepEqual(
        await current.evaluate(() => [
          globalThis.foreignFocus === true,
          globalThis.foreignKeys === true,
        ]),
        [false, false],
      );
    } finally {
      await page.close();
    }
  },
);
