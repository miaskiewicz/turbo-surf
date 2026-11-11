/* eslint-disable no-underscore-dangle, eqeqeq, prefer-const, no-await-in-loop, unicorn/consistent-function-scoping, no-unused-vars, no-array-reverse, no-shadow */
// Exhaustive surface coverage of the napi-backed Playwright shim — one assertion
// cluster per Locator method, per expect matcher (all 5 assertion classes), per
// Page method, plus BrowserContext, fixtures, waiters, events, and the honest
// "can't do that" throws. Offline (setContent / a localhost server). Skips
// cleanly if the native addon isn't built.

import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (e) {
  console.log("shim: native addon not built, skipping —", e.message);
  process.exit(0);
}
const { BrowserContext, chromium, devices, expect, newPage, request, test: pw } = shim;

test("uncaught live handler exceptions emit Error payloads", async () => {
  const page = newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error));
  try {
    await page.setContent(
      "<button onclick=\"this.textContent='started';throw new TypeError('uncaught-click-marker')\">Run</button>",
    );
    await page._ensureLive();
    await page.locator("button").click();
    assert.equal(await page.locator("button").textContent(), "started");
    assert.equal(errors.length, 1);
    assert.ok(errors[0] instanceof Error);
    assert.equal(errors[0].name, "TypeError");
    assert.equal(errors[0].message, "uncaught-click-marker");
    assert.match(errors[0].stack, /uncaught-click-marker/);
  } finally {
    await page.close();
  }
});

test("response waits poll a newly available native log within ten milliseconds", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout", "setInterval"] });
  const page = newPage();
  page._drainNetworkEvents = async () => {
    page._emit("response", { url: () => "https://example.test/ready" });
  };
  let settled = false;
  const waiting = page.waitForResponse("**/ready").then(() => {
    settled = true;
  });
  t.mock.timers.tick(10);
  await new Promise((resolve) => setImmediate(resolve));
  const settledWithinTen = settled;
  t.mock.timers.tick(50);
  await waiting;
  await page.close();
  assert.equal(
    settledWithinTen,
    true,
    "ready responses must not wait for a fifty-millisecond poll",
  );
});

// A tiny site so navigation/forms/headers/cookies are exercised end to end.
function site() {
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    if (url.pathname === "/login") {
      res.writeHead(200, { "set-cookie": "sid=abc; Path=/", "content-type": "text/html" });
      return res.end("<title>in</title><a href='/home'>home</a>");
    }
    if (url.pathname === "/echo") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>echo</title><p id='c'>${req.headers.cookie ?? ""}</p><p id='h'>${req.headers["x-test"] ?? ""}</p>`,
      );
    }
    if (url.pathname === "/search") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(`<title>results</title><p>q=${url.searchParams.get("q")}</p>`);
    }
    if (url.pathname === "/spa") {
      // Client-rendered: #root is EMPTY in the shipped shell and only filled once the
      // page's own JS runs (a live session). Distinguishes a re-hydrated reload from a
      // raw shell re-fetch.
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>spa</title><div id="root"></div><script>document.getElementById('root').textContent='HYDRATED';</script>`,
      );
    }
    res.writeHead(200, { "content-type": "text/html" });
    res.end(`<title>${url.pathname}</title><main><h1>Page ${url.pathname}</h1></main>`);
  });
  return server;
}
async function listen(server) {
  await new Promise((r) => server.listen(0, r));
  return `http://127.0.0.1:${server.address().port}`;
}

const DOC = `<html><head><title>Doc</title></head><body>
  <main>
    <h1>Title</h1>
    <p class="lead">lead text</p>
    <ul id="list"><li>one</li><li>two</li><li>three</li></ul>
    <button class="btn primary" id="go">Go</button>
    <button disabled>Off</button>
    <input id="name" value="init" placeholder="your name">
    <input id="agree" type="checkbox">
    <select id="sel"><option value="a">A</option><option value="b">B</option></select>
    <label for="email">Email</label><input id="email">
    <a href="/next" title="goto">link</a>
    <img alt="logo" src="/l.png">
    <div id="hidden" style="display:none">secret</div>
    <nav><a href="/">Home</a></nav>
  </main>
</body></html>`;

async function fresh() {
  const page = newPage();
  await page.setContent(DOC);
  return page;
}

test("response events drain host records while opaque bodies stay hidden in page JS", async () => {
  const target = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("host-only-response");
  });
  await new Promise((done) => target.listen(0, "127.0.0.1", done));
  const endpoint = `http://127.0.0.1:${target.address().port}/private`;
  const source = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(
      `<body><script>fetch(${JSON.stringify(endpoint)},{mode:'no-cors'}).then(r=>r.text()).then(text=>document.body.setAttribute('data-body',text));</script></body>`,
    );
  });
  await new Promise((done) => source.listen(0, "127.0.0.1", done));
  const page = newPage();
  const observed = [];
  page.on("response", (response) => {
    if (response.url() === endpoint) observed.push(response);
  });
  try {
    await page.goto(`http://127.0.0.1:${source.address().port}/`, { waitUntil: "networkidle" });
    assert.equal(await page.locator("body").getAttribute("data-body"), "");
    assert.equal(
      await page.evaluate(() => String(globalThis[Symbol.for("ts")] !== undefined)),
      "false",
    );
    assert.equal(observed.length, 1);
    assert.equal(await observed[0].text(), "host-only-response");
    await page._drainNetworkEvents();
    assert.equal(observed.length, 1, "host records drain once");
  } finally {
    await page.close();
    source.closeAllConnections();
    target.closeAllConnections();
    await Promise.all([source, target].map((server) => new Promise((done) => server.close(done))));
  }
});

test("request events drain host records alongside responses", async () => {
  const target = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("host-only-response");
  });
  await new Promise((done) => target.listen(0, "127.0.0.1", done));
  const endpoint = `http://127.0.0.1:${target.address().port}/private`;
  const source = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(
      `<body><script>fetch(${JSON.stringify(endpoint)},{mode:'no-cors'}).then(r=>r.text());</script></body>`,
    );
  });
  await new Promise((done) => source.listen(0, "127.0.0.1", done));
  const page = newPage();
  const requests = [];
  page.on("request", (request) => {
    if (request.url() === endpoint) requests.push(request);
  });
  try {
    await page.goto(`http://127.0.0.1:${source.address().port}/`, { waitUntil: "networkidle" });
    assert.equal(requests.length, 1);
    assert.equal(requests[0].url(), endpoint);
  } finally {
    await page.close();
    source.closeAllConnections();
    target.closeAllConnections();
    await Promise.all([source, target].map((server) => new Promise((done) => server.close(done))));
  }
});

// --- Locator: composition (pure JS) ---------------------------------------
test("Locator composition: count/first/last/nth/all", async () => {
  const p = await fresh();
  assert.equal(await p.locator("#list li").count(), 3);
  assert.equal(await p.locator("#list li").first().textContent(), "one");
  assert.equal(await p.locator("#list li").last().textContent(), "three");
  assert.equal(await p.locator("#list li").nth(1).textContent(), "two");
  assert.equal(await p.locator("#list li").nth(-1).textContent(), "three");
  assert.equal((await p.locator("#list li").all()).length, 3);
});

test("Locator filter / and / or", async () => {
  const p = await fresh();
  assert.equal(await p.locator("#list li").filter({ hasText: "tw" }).count(), 1);
  assert.equal(await p.locator("#list li").filter({ hasNotText: "two" }).count(), 2);
  assert.equal(await p.locator("button").and(p.locator(".primary")).count(), 1);
  assert.equal(await p.locator("h1").or(p.locator(".lead")).count(), 2);
});

test("Locator text reads: textContent/innerText/innerHTML/allTextContents/allInnerTexts", async () => {
  const p = await fresh();
  assert.equal(await p.locator(".lead").textContent(), "lead text");
  assert.equal(await p.locator(".lead").innerText(), "lead text");
  assert.match(await p.locator("#list").innerHTML(), /<li>one<\/li>/);
  assert.deepEqual(await p.locator("#list li").allTextContents(), ["one", "two", "three"]);
  assert.deepEqual(await p.locator("#list li").allInnerTexts(), ["one", "two", "three"]);
});

// --- Locator: accessors (cross to Rust) -----------------------------------
test("Locator accessors: attr/value/role/name/visible/checked/enabled/editable/empty/css", async () => {
  const p = await fresh();
  assert.equal(await p.locator("a[title]").getAttribute("href"), "/next");
  assert.equal(await p.locator("#name").inputValue(), "init");
  assert.equal(await p.locator("a[title]").ariaRole(), "link");
  assert.equal(await p.locator("#go").isVisible(), true);
  assert.equal(await p.locator("#hidden").isVisible(), false);
  assert.equal(await p.locator("#hidden").isHidden(), true);
  assert.equal(await p.locator("button[disabled]").isEnabled(), false);
  assert.equal(await p.locator("button[disabled]").isDisabled(), true);
  assert.equal(await p.locator("#name").isEditable(), true);
  assert.equal(typeof (await p.locator("#go").accessibleName()), "string");
  assert.equal(typeof (await p.locator("#go").cssValue("display")), "string");
});

test("Locator selectedValues + selectOption", async () => {
  const p = await fresh();
  await p.locator("#sel").selectOption("b");
  assert.deepEqual(await p.locator("#sel").selectedValues(), ["b"]);
});

// --- Locator: actions ------------------------------------------------------
test("Locator actions: fill/clear/type/pressSequentially/check/uncheck/setChecked", async () => {
  const p = await fresh();
  await p.locator("#name").fill("alice");
  assert.equal(await p.locator("#name").inputValue(), "alice");
  await p.locator("#name").clear();
  assert.equal(await p.locator("#name").inputValue(), "");
  await p.locator("#name").type("bob");
  assert.equal(await p.locator("#name").inputValue(), "bob");
  await p.locator("#agree").check();
  assert.equal(await p.locator("#agree").isChecked(), true);
  await p.locator("#agree").setChecked(false);
  assert.equal(await p.locator("#agree").isChecked(), false);
});

test("Locator.setInputFiles selects genuine Files through the input's FileList", async () => {
  const p = newPage();
  await p.setContent(
    "<input type=file id=f multiple><p id=log></p><script>document.querySelector('#f').addEventListener('change',(e)=>{const f=e.target.files;document.querySelector('#log').textContent=[f instanceof FileList,f.length,f[0] instanceof File,f[0].name,f[0].type,f[0].size,f[1].size,e.target.value].join('|');});</script>",
  );
  await p.locator("#f").setInputFiles([
    { name: "a.bin", mimeType: "application/x-test", buffer: Buffer.from([0, 255, 128]) },
    { name: "b.txt", mimeType: "text/plain", buffer: Buffer.from("hi") },
  ]);
  assert.equal(
    await p.locator("#log").textContent(),
    "true|2|true|a.bin|application/x-test|3|2|C:\\fakepath\\a.bin",
  );
});

test("Locator no-op actions resolve quietly (focus/blur/scroll/dispatch/waitFor/highlight)", async () => {
  const p = await fresh();
  await p.locator("#go").focus();
  await p.locator("#go").blur();
  await p.locator("#go").scrollIntoViewIfNeeded();
  await p.locator("#go").dispatchEvent("click");
  await p.locator("#go").highlight();
  await p.locator("#go").waitFor();
  assert.ok(true);
});

test("Locator.evaluate runs page JS for a CSS-backed locator", async () => {
  const p = await fresh();
  const len = await p.locator("#list").evaluate((el) => el.querySelectorAll("li").length);
  assert.equal(Number(len), 3);
  assert.equal(await p.getByText("one").evaluate((el) => el.textContent), "one");
  await p.close();
});

// --- Locator: getBy* -------------------------------------------------------
test("Page getBy*: role/text/label/testid/placeholder/alt/title", async () => {
  const p = newPage();
  await p.setContent(
    "<button>Save</button><span>hello</span><label for='q'>Q</label><input id='q'>" +
      "<div data-testid='card'>c</div><input placeholder='search'><img alt='pic'><a title='tip'>t</a>",
  );
  assert.equal(await p.getByRole("button", { name: "Save" }).count(), 1);
  assert.equal(await p.getByText("hello").count(), 1);
  assert.equal(await p.getByLabel("Q").count(), 1);
  assert.equal(await p.getByTestId("card").count(), 1);
  assert.equal(await p.getByPlaceholder("search").count(), 1);
  assert.equal(await p.getByAltText("pic").count(), 1);
  assert.equal(await p.getByTitle("tip").count(), 1);
});

test("getByTestId honours a custom testIdAttribute", async () => {
  const ctx = new BrowserContext({ testIdAttribute: "data-test-id" });
  const p = await ctx.newPage();
  await p.setContent("<button data-test-id='submit'>S</button>");
  assert.equal(await p.getByTestId("submit").count(), 1);
});

// --- LocatorAssertions -----------------------------------------------------
test("LocatorAssertions: visibility / state matchers", async () => {
  const p = await fresh();
  await expect(p.locator("#go")).toBeVisible();
  await expect(p.locator("#hidden")).toBeHidden();
  await expect(p.locator("#go")).toBeAttached();
  await expect(p.locator("#go")).toBeEnabled();
  await expect(p.locator("button[disabled]")).toBeDisabled();
  await expect(p.locator("#name")).toBeEditable();
  await expect(p.locator("#go")).toBeInViewport();
  await expect(p.locator("#missing")).not.toBeVisible();
});

test("LocatorAssertions: text / value / count matchers", async () => {
  const p = await fresh();
  await expect(p.locator("h1")).toHaveText("Title");
  await expect(p.locator("h1")).toHaveText(/Tit/);
  await expect(p.locator(".lead")).toContainText("lead");
  await expect(p.locator(".lead")).toContainText(/lead/);
  await expect(p.locator("#list li")).toHaveCount(3);
  await expect(p.locator("#name")).toHaveValue("init");
  await expect(p.locator("#name")).toHaveValue(/ini/);
});

test("LocatorAssertions: attribute / class / role / id / aria-name matchers", async () => {
  const p = await fresh();
  await expect(p.locator("a[title]")).toHaveAttribute("href", "/next");
  await expect(p.locator("a[title]")).toHaveAttribute("href");
  await expect(p.locator("#go")).toHaveClass("primary");
  await expect(p.locator("#go")).toContainClass("btn");
  await expect(p.locator("#go")).toHaveId("go");
  await expect(p.locator("a[title]")).toHaveRole("link");
  await expect(p.locator("#go")).toHaveAccessibleName(/Go/);
});

test("LocatorAssertions: toHaveValues / toMatchAriaSnapshot", async () => {
  const p = await fresh();
  await p.locator("#sel").selectOption("b");
  await expect(p.locator("#sel")).toHaveValues(["b"]);
  await expect(p.locator("nav")).toMatchAriaSnapshot('- link "Home"');
});

test("LocatorAssertions: toHaveScreenshot throws (unsupported)", async () => {
  const p = await fresh();
  await assert.rejects(() => expect(p.locator("#go")).toHaveScreenshot(), /unavailable/);
});

// --- PageAssertions --------------------------------------------------------
test("PageAssertions: toHaveURL / toHaveTitle", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  await p.goto(`${base}/abc`);
  await expect(p).toHaveURL(/\/abc$/);
  await expect(p).toHaveURL(`${base}/abc`);
  await expect(p).toHaveTitle("/abc");
  await expect(p).not.toHaveTitle("nope");
  server.close();
});

// --- GenericAssertions -----------------------------------------------------
test("GenericAssertions: equality / membership / numeric / type", async () => {
  expect(1).toBe(1);
  expect({ a: [1, 2] }).toEqual({ a: [1, 2] });
  expect({ a: 1 }).toStrictEqual({ a: 1 });
  expect("abc").toContain("b");
  expect([1, 2, 3]).toContain(2);
  expect([{ x: 1 }]).toContainEqual({ x: 1 });
  expect("hello").toMatch(/ell/);
  expect({ a: 1, b: 2 }).toMatchObject({ a: 1 });
  expect(null).toBeNull();
  expect(undefined).toBeUndefined();
  expect(1).toBeDefined();
  expect(1).toBeTruthy();
  expect(0).toBeFalsy();
  expect(Number.NaN).toBeNaN();
  expect(5).toBeGreaterThan(4);
  expect(5).toBeGreaterThanOrEqual(5);
  expect(3).toBeLessThan(4);
  expect(3).toBeLessThanOrEqual(3);
  expect(3.14159).toBeCloseTo(3.14, 2);
  expect([]).toBeInstanceOf(Array);
  expect([1, 2]).toHaveLength(2);
  expect({ a: { b: 1 } }).toHaveProperty("a.b", 1);
  expect(() => {
    throw new Error("x");
  }).toThrow();
  expect(() => {
    throw new Error("boom");
  }).toThrowError("boom");
  expect(1).not.toBe(2);
});

test("GenericAssertions: jest-mock matchers + toMatchSnapshot throws", async () => {
  const calls = [];
  const spy = (...a) => calls.push(a);
  spy.mock = { calls };
  spy(1, 2);
  expect(spy).toHaveBeenCalled();
  expect(spy).toHaveBeenCalledWith(1, 2);
  await assert.rejects(async () => expect("x").toMatchSnapshot(), /unavailable/);
});

test("expect.poll / expect.soft", async () => {
  expect.poll(() => 7).toBe(7);
  await expect.soft(newPage().locator("nothing")).not.toBeVisible();
});

// --- Page: navigation + history -------------------------------------------
test("Page navigation: goto / reload / goBack / goForward", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  await p.goto(`${base}/a`);
  await p.goto(`${base}/b`);
  assert.equal(await p.title(), "/b");
  await p.reload();
  assert.equal(await p.title(), "/b");
  await p.goBack();
  assert.equal(await p.title(), "/a");
  await p.goForward();
  assert.equal(await p.title(), "/b");
  server.close();
});

test("Page.reload({waitUntil:'networkidle'}) re-hydrates the live SPA", async () => {
  // The settings language-survives-reload spec did `page.reload({waitUntil:'networkidle'})`
  // and read a client-rendered select; reload IGNORED the option and never re-opened a live
  // session, so the reloaded doc stayed the raw shell (empty select → value ""). A networkidle
  // reload must re-hydrate, like a networkidle goto.
  const server = site();
  const base = await listen(server);
  const p = newPage();
  await p.goto(`${base}/spa`, { waitUntil: "networkidle" });
  assert.equal(await p.locator("#root").textContent(), "HYDRATED");
  await p.reload({ waitUntil: "networkidle" });
  assert.equal(await p.locator("#root").textContent(), "HYDRATED");
  server.close();
});

test("Page baseURL: relative goto resolves against context baseURL", async () => {
  const server = site();
  const base = await listen(server);
  const ctx = new BrowserContext({ baseURL: base });
  const p = await ctx.newPage();
  await p.goto("/xyz");
  assert.equal(await p.title(), "/xyz");
  server.close();
});

test("Page setExtraHTTPHeaders sends real headers (not a no-op)", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  await p.setExtraHTTPHeaders({ "x-test": "wired" });
  await p.goto(`${base}/echo`);
  assert.equal(await p.locator("#h").textContent(), "wired");
  server.close();
});

test("Page cookies persist across navigations (storageState shape)", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  await p.goto(`${base}/login`);
  await p.goto(`${base}/echo`);
  assert.match(await p.locator("#c").textContent(), /sid=abc/);
  assert.equal(p.storageState().cookies.length, 1);
  server.close();
});

// --- Page: read selector-shortcuts ----------------------------------------
test("Page selector shortcuts: innerText/innerHTML/textContent/getAttribute/inputValue/is*", async () => {
  const p = await fresh();
  assert.equal(await p.innerText("h1"), "Title");
  assert.match(await p.innerHTML("#list"), /<li>/);
  assert.equal(await p.textContent(".lead"), "lead text");
  assert.equal(await p.getAttribute("a[title]", "href"), "/next");
  assert.equal(await p.inputValue("#name"), "init");
  assert.equal(await p.isVisible("#go"), true);
  assert.equal(await p.isHidden("#hidden"), true);
  assert.equal(await p.isEnabled("#go"), true);
  assert.equal(await p.isDisabled("button[disabled]"), true);
});

// --- Page: actions submit forms -------------------------------------------
test("Page click submits a GET form to the server", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  await p.goto(`${base}/`);
  await p.setContent(
    `<form action="${base}/search" method="get"><input name="q" value="rust"><button>Go</button></form>`,
  );
  await p.click("button");
  assert.match(await p.content(), /q=rust/);
  server.close();
});

// --- Page: frames + waiters ------------------------------------------------
test("Page without iframes: mainFrame + frames() + frameLocator over host", async () => {
  const p = await fresh();
  assert.equal(p.mainFrame(), p);
  assert.deepEqual(p.frames(), [p]);
  assert.equal(p.frame(), p);
  assert.ok(p.frameLocator("main"));
});

// Engine iframe contexts exposed as Frame/FrameLocator. The host-vs-frame scope is
// the whole point: a `#inside` button inside an iframe MUST NOT match on the host
// page, and `frameLocator('iframe')` queries must land in the child document.
test("Page.frames / frame({name}) / frameLocator: iframe srcdoc", async () => {
  const p = newPage();
  await p.setContent(`<html><body>
    <h1>Host</h1>
    <iframe name="child" srcdoc="<html><body><button id='inside'>Pay</button><p class='lead'>child lead</p></body></html>"></iframe>
  </body></html>`);
  // frames() lists the page + each iframe with content
  const frames = p.frames();
  assert.equal(frames.length, 2);
  assert.equal(frames[0], p);
  assert.notEqual(frames[1], p);
  // frame({name}) finds the child
  const child = p.frame({ name: "child" });
  assert.ok(child, "frame({name}) resolves");
  assert.equal(child.name(), "child");
  assert.notEqual(child, p);
  // child-scoped queries see the iframe content, not the host
  assert.equal(await child.locator("#inside").count(), 1);
  assert.equal(await child.locator("#inside").textContent(), "Pay");
  assert.equal(await child.locator(".lead").textContent(), "child lead");
  // host doesn't see the frame's content
  assert.equal(await p.locator("#inside").count(), 0);
  // frameLocator resolves inside the iframe
  const fl = p.frameLocator("iframe[name=child]");
  assert.equal(await fl.locator("#inside").count(), 1);
  assert.equal(await fl.locator("#inside").textContent(), "Pay");
  assert.equal(await fl.getByRole("button").count(), 1);
  // host's p.locator('.lead') is the host's own lead if any (none here), not the frame's
  assert.equal(await p.locator("p.lead").count(), 0);
});

// Regression for #283: FrameLocator.locator().fill() mutates were being discarded
// because _resolveFrame() returned a throwaway Frame each call. Cache keyed on
// parent HTML identity keeps the mutated Frame alive across successive queries.
test("frameLocator.locator().fill: iframe DOM reflects the write (#283)", async () => {
  const p = newPage();
  await p.setContent(`<body>
    <iframe name="form" srcdoc="<input id='name' value='' /><input id='other' value='' />"></iframe>
  </body>`);
  const fl = p.frameLocator("iframe[name=form]");
  await fl.locator("#name").fill("Alice");
  assert.equal(await fl.locator("#name").inputValue(), "Alice");
  // A fresh FrameLocator over the same parent must also see the write.
  const fl2 = p.frameLocator("iframe[name=form]");
  assert.equal(await fl2.locator("#name").inputValue(), "Alice");
  // Independent field stays empty — the cache holds the WHOLE frame, not just one node.
  assert.equal(await fl.locator("#other").inputValue(), "");
});

test("Page.frame by string name", async () => {
  const p = newPage();
  await p.setContent(
    `<body><iframe name="payment" srcdoc="<button id='b'>Go</button>"></iframe></body>`,
  );
  const f = p.frame("payment");
  assert.ok(f);
  assert.equal(f.name(), "payment");
  assert.equal(await f.locator("#b").textContent(), "Go");
});

test("Page waiters resolve on the static DOM", async () => {
  const p = await fresh();
  await p.waitForLoadState("networkidle");
  await p.waitForTimeout(1);
  assert.ok(await p.waitForSelector("h1"));
  assert.equal(await p.waitForSelector("nope", { state: "hidden" }), null);
  assert.equal(Number(await p.waitForFunction(() => 1 + 1)), 2);
  await assert.rejects(p.waitForResponse(undefined, { timeout: 10 }), { name: "TimeoutError" });
  await assert.rejects(p.waitForRequest(undefined, { timeout: 10 }), { name: "TimeoutError" });
  await p.waitForURL(/Doc|about/);
});

// --- Page: events ----------------------------------------------------------
test("Page events: on/once/off load + close", async () => {
  const p = newPage();
  let loads = 0;
  let closed = 0;
  const onLoad = () => loads++;
  p.on("load", onLoad);
  p.once("close", () => closed++);
  await p.setContent("<p>x</p>"); // fires load
  p.off("load", onLoad);
  await p.setContent("<p>y</p>"); // load no longer counted
  assert.equal(loads, 1);
  await p.close();
  assert.equal(closed, 1);
  assert.equal(p.isClosed(), true);
});

// --- Page: config / state --------------------------------------------------
test("Page config: viewport / context / video / workers / no-op emulateMedia", async () => {
  const ctx = new BrowserContext({ viewport: { width: 800, height: 600 } });
  const p = await ctx.newPage();
  assert.deepEqual(p.viewportSize(), { width: 800, height: 600 });
  await p.setViewportSize({ width: 1024, height: 768 });
  assert.deepEqual(p.viewportSize(), { width: 1024, height: 768 });
  assert.equal(p.context(), ctx);
  assert.equal(p.video(), null);
  assert.deepEqual(p.workers(), []);
  await p.emulateMedia({ media: "print" });
  p.setDefaultTimeout(1000);
  p.setDefaultNavigationTimeout(1000);
});

test("Page evaluate (string + function + arg) and render hydrate the DOM", async () => {
  const p = newPage();
  await p.setContent("<div id='n'>40</div><div id='app'></div>");
  assert.equal(await p.evaluate("document.querySelector('#n').textContent"), "40");
  assert.equal(
    Number(await p.evaluate((s) => Number(document.querySelector(s).textContent) + 2, "#n")),
    42,
  );
  await p.render("document.getElementById('app').innerHTML='<b>hi</b>'");
  assert.match(await p.content(), /<b>hi<\/b>/);
});

// --- BrowserContext --------------------------------------------------------
test("BrowserContext: pages / cookies / storageState / addCookies / clearCookies", async () => {
  const ctx = new BrowserContext();
  const p = await ctx.newPage();
  assert.deepEqual(ctx.pages(), [p]);
  ctx.addCookies([{ name: "a", value: "1", domain: "x", path: "/" }]);
  assert.equal((await ctx.cookies()).length, 1);
  assert.equal(ctx.storageState().cookies.length, 1);
  await ctx.clearCookies();
  assert.equal((await ctx.cookies()).length, 0);
});

test("BrowserContext: browser()/request() + no-op permission/geo/offline", async () => {
  const ctx = new BrowserContext();
  assert.equal(ctx.browser().version(), "turbo-surf");
  assert.ok(ctx.request);
  await ctx.grantPermissions(["clipboard-read"]);
  await ctx.clearPermissions();
  await ctx.setGeolocation({ latitude: 0, longitude: 0 });
  await ctx.setOffline(true);
  await ctx.close();
});

test("chromium.launch → browser → context → page (drop-in shape)", async () => {
  const browser = await chromium.launch();
  assert.equal(browser.isConnected(), true);
  const ctx = await browser.newContext({ baseURL: "http://x" });
  const p = await ctx.newPage();
  assert.ok(p);
  await browser.close();
  assert.equal(chromium.name(), "chromium");
});

// --- @playwright/test surface ---------------------------------------------
test("devices proxy returns an empty descriptor", () => {
  assert.deepEqual(devices["iPhone 13"], {});
});

test("request.newContext().get/post/fetch", async () => {
  const server = site();
  const base = await listen(server);
  const ctx = await request.newContext();
  const r = await ctx.get(`${base}/abc`);
  assert.equal(r.status(), 200);
  assert.match(await r.text(), /Page \/abc/);
  await ctx.dispose();
  server.close();
});

test("request methods pass the HTTP method + body through", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      seen.push({ method: req.method, body });
      res.writeHead(200, { "content-type": "text/html" });
      res.end(`<title>${req.method}</title>`);
    });
  });
  const base = await listen(server);
  const ctx = await request.newContext();
  assert.equal(
    (await (await ctx.post(`${base}/`, { data: { a: 1 } })).text()).match(/POST/)?.[0],
    "POST",
  );
  await ctx.put(`${base}/`);
  await ctx.delete(`${base}/`);
  assert.deepEqual(
    seen.map((s) => s.method),
    ["POST", "PUT", "DELETE"],
  );
  assert.match(seen[0].body, /"a":1/);
  server.close();
});

test("request.newContext baseURL resolves relative paths + JSON content-type for object data", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      seen.push({ url: req.url, method: req.method, ctype: req.headers["content-type"], body });
      res.writeHead(200, { "content-type": "application/json" });
      res.end(`{"ok":true}`);
    });
  });
  const base = await listen(server);
  // baseURL set on the context → a relative path must resolve against it (was a
  // reqwest "builder error" before). Object `data` → JSON body + content-type.
  const ctx = await request.newContext({ baseURL: base });
  const res = await ctx.post("/api/thing?x=1", { data: { user: "greg" } });
  assert.equal(res.status(), 200);
  assert.equal(seen[0].url, "/api/thing?x=1");
  assert.equal(seen[0].ctype, "application/json");
  assert.match(seen[0].body, /"user":"greg"/);
  // per-request header overrides, string body sent as-is
  await ctx.post("/raw", { data: "plain", headers: { "content-type": "text/plain" } });
  assert.equal(seen[1].ctype, "text/plain");
  assert.equal(seen[1].body, "plain");
  server.close();
});

test("BrowserContext reads baseURL/testIdAttribute from env (config use{} mapping)", () => {
  process.env.TURBO_SHIM_BASE_URL = "http://env.example";
  process.env.TURBO_SHIM_TESTID_ATTR = "data-qa";
  try {
    const ctx = new BrowserContext();
    assert.equal(ctx._baseURL, "http://env.example");
    assert.equal(ctx._testIdAttribute, "data-qa");
    // explicit opts still win over env
    const ctx2 = new BrowserContext({ baseURL: "http://opt", testIdAttribute: "data-x" });
    assert.equal(ctx2._baseURL, "http://opt");
    assert.equal(ctx2._testIdAttribute, "data-x");
  } finally {
    // DELETE (not `= undefined`) — assigning undefined coerces to the STRING "undefined",
    // which then leaks: later `new BrowserContext()` reads testIdAttribute="undefined" (breaks
    // getByTestId) and baseURL="undefined" (makes `new URL(url,"undefined")` throw in goto).
    delete process.env.TURBO_SHIM_BASE_URL;
    delete process.env.TURBO_SHIM_TESTID_ATTR;
  }
});

test("GenericAssertions: .resolves / .rejects", async () => {
  await expect(Promise.resolve(5)).resolves.toBe(5);
  await expect(Promise.resolve([1, 2])).resolves.toEqual([1, 2]);
  await expect(Promise.reject(new Error("boom"))).rejects.toThrow(/boom/);
  await expect(Promise.reject(new Error("boom"))).rejects.toThrow("boom");
});

pw("fixture { page } is injected and isolated", async ({ page }) => {
  await page.setContent("<h1>fx</h1>");
  await expect(page.locator("h1")).toHaveText("fx");
});

pw("fixtures: context + request injected", async ({ context, request: req }) => {
  assert.ok(context instanceof BrowserContext);
  assert.ok(req);
});

const extended = pw.extend({
  greeting: async (_f, use) => use("hi"),
  named: async ({ page }, use) => {
    await page.setContent("<h2>custom</h2>");
    await use(page);
  },
});
extended("test.extend injects custom fixtures", async ({ greeting, named }) => {
  assert.equal(greeting, "hi");
  await expect(named.locator("h2")).toHaveText("custom");
});

// The payroll pattern: override `page` with a fixture that depends on the base
// `page` (same name). The override must SEE the base page, not undefined.
const wrapped = pw.extend({
  page: async ({ page }, use) => {
    page._wrapped = true; // tag the base page the override received
    await page.setContent("<h4>wrapped</h4>");
    await use(page);
  },
});
wrapped("page-override fixture receives + wraps the base page", async ({ page }) => {
  assert.equal(page._wrapped, true);
  await expect(page.locator("h4")).toHaveText("wrapped");
});

pw.describe("describe + hooks", () => {
  let order = [];
  pw.beforeAll(() => order.push("beforeAll"));
  pw.afterAll(() => order.push("afterAll"));
  pw.beforeEach(() => order.push("beforeEach"));
  pw.afterEach(() => order.push("afterEach"));
  pw("a nested test with steps + testInfo", async ({ page }, info) => {
    assert.ok(page);
    assert.equal(typeof info.outputPath(), "string");
    assert.equal(info.project.name, "turbo-surf");
    await pw.step("a step", async () => assert.ok(true));
    order.push("test");
  });
});

pw.skip("skipped test does not run", async () => {
  throw new Error("should not execute");
});

// --- honest "can't do that" throws ----------------------------------------
test("page.screenshot renders synthetic pixels (no browser)", async () => {
  const p = await fresh();
  const png = await p.screenshot();
  assert.ok(Buffer.isBuffer(png) && png.length > 0);
  assert.deepEqual([...png.subarray(0, 4)], [0x89, 0x50, 0x4e, 0x47]);
});

test("honest throws: pixel / input / network-interception", async () => {
  const p = await fresh();
  await assert.rejects(() => p.pdf(), /unavailable/);
  await assert.rejects(() => p.locator("#go").screenshot(), /unavailable/);
  const box = await p.locator("#go").boundingBox();
  assert.ok(box && box.width > 0 && box.height > 0);
  await assert.rejects(() => p.locator("#name").selectText(), /input/);
  // page.route(fulfill) + page.routeFromHAR (HAR playback) became real in #220;
  // exposeFunction / exposeBinding became real in #226. context.route still
  // isn't — covered elsewhere. Nothing left to assert as an honest throw here.
});

// --- waitForResponse staleness (Playwright semantics) ---------------------
// The live engine drains network in batches into a buffer. waitForResponse must NOT
// return a response that drained in an EARLIER interaction — only one produced by the
// current action (or later). Regression guard for the bulk-upload bug: a loose predicate
// like url.includes('/bulk-upload') would otherwise grab the prior step's template GET
// (.../bulk-upload/template) instead of waiting for this step's POST (.../bulk-upload).
test("waitForResponse ignores a response that drained in an earlier step", async () => {
  const p = newPage();
  const mk = (url, status, seq) => ({ url: () => url, status: () => status, _seq: seq });
  // Step 1 (action seq 1) drained the template GET. Step 2 is now current (seq 2).
  p._recentResponses = [mk("https://x/leave-requests/bulk-upload/template", 200, 1)];
  p._actionSeq = 2;
  const matcher = (r) => r.url().includes("/leave-requests/bulk-upload");
  // The stale template GET must be skipped; the fresh POST (seq 2) arrives via emit.
  const fresh = mk("https://x/leave-requests/bulk-upload", 201, 2);
  const pending = p.waitForResponse(matcher, { timeout: 2000 });
  p._recentResponses.push(fresh);
  p._emit("response", fresh);
  const got = await pending;
  assert.equal(got.status(), 201, "should resolve to the fresh POST, not the stale GET");
});

// Rest of #229: non-'networkidle' load states, waitForNavigation, generic
// waitForEvent('response'|'request'). The old shim returned a synthetic 200
// for waitForNavigation without ever waiting, and only 'networkidle' did
// anything in waitForLoadState — anything else was a silent no-op.
test("waitForLoadState('load') / 'domcontentloaded' hydrates the page like 'networkidle'", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(`${base}/spa`, { waitUntil: "commit" });
    assert.equal(p._session, null, "commit must NOT open a live session on its own");
    await p.waitForLoadState("load");
    assert.ok(p._session != null, "'load' must hydrate like 'networkidle'");
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("waitForNavigation resolves when the URL actually changes", async () => {
  const p = newPage();
  p._url = "http://x/start";
  const pending = p.waitForNavigation({ timeout: 500 });
  setTimeout(() => {
    p._url = "http://x/next";
  }, 20);
  const r = await pending;
  assert.equal(r.url(), "http://x/next", "must return a response pointing at the new URL");
});

test("waitForEvent('response') / ('request') resolve from the listener bus", async () => {
  const p = newPage();
  const fakeResp = {
    url: () => "https://x/api/a",
    status: () => 200,
    request: () => ({ url: () => "https://x/api/a", method: () => "GET" }),
  };
  const pending = p.waitForEvent("response", { timeout: 500 });
  setTimeout(() => p._emit("response", fakeResp), 10);
  const r = await pending;
  assert.equal(r.status(), 200);
  const pendingReq = p.waitForEvent("request", { timeout: 500 });
  setTimeout(() => p._emit("request", fakeResp.request()), 10);
  const req = await pendingReq;
  assert.equal(req.method(), "GET");
});

// Payroll parity v3: real integration still timed out at 15 s even after the
// unit-level drain-polling fix. Root cause: a default `goto(url)` (no
// `waitUntil:'networkidle'`) doesn't open a live session, so an inline
// `<script>fetch('/api/...')</script>` inside the document never runs —
// there's no real response to short-circuit on, and waitForResponse falls
// through to the synthetic timeout. Auto-hydrate when the listener bus is
// already subscribed so the inline-script fetches actually fire.
test("Promise.all([waitForResponse(...), goto(url)]) short-circuits on the real response", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/page") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>p</title><div id="out"></div><script>
           fetch('/api/data').then(r => r.json()).then(j => {
             document.getElementById('out').textContent = j.v;
           });
         </script>`,
      );
    }
    if (req.url === "/api/data") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end('{"v":"ok"}');
    }
    res.writeHead(404);
    res.end();
  });
  const base = await listen(server);
  const p = newPage();
  try {
    const started = Date.now();
    const [resp] = await Promise.all([
      p.waitForResponse((r) => r.url().includes("/api/data"), { timeout: 3000 }),
      p.goto(`${base}/page`),
    ]);
    const elapsed = Date.now() - started;
    assert.equal(resp.status(), 200);
    assert.ok(
      resp.url().includes("/api/data"),
      "must resolve the REAL /api/data response, not the synthetic 200",
    );
    assert.ok(elapsed < 2000, `waitForResponse must short-circuit, took ${elapsed}ms`);
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Payroll parity v2 found that `Promise.all([waitForResponse(...), goto(...)])`
// still took the full 15s timeout. Root cause: `waitForResponse` gated its
// drain-poll on `this._live` at CALL time, but when the call races with the
// goto that will later establish the live session, `_live` is false then —
// so poll was null and the drain never ran. Must keep polling once live.
test("waitForResponse short-circuits even when _live comes up AFTER the call", async () => {
  const p = newPage();
  p._session = null; // not live yet (as if called before goto resolves)
  p._actionSeq = 1;
  p._recentResponses = [];
  let drainCalls = 0;
  p._drainNetworkEvents = async () => {
    if (p._session == null) return; // the real drain early-returns too
    drainCalls++;
    if (drainCalls >= 2) {
      const resp = { url: () => "https://x/api/ready", status: () => 200, _seq: 1 };
      p._recentResponses.push(resp);
      p._emit("response", resp);
    }
  };
  const pending = p.waitForResponse((r) => r.url().includes("/api/ready"), { timeout: 500 });
  // Simulate goto establishing the live session a moment later.
  setTimeout(() => {
    p._session = 1;
  }, 30);
  const got = await pending;
  assert.equal(got.status(), 200, "must resolve the drained response, not the timeout synthetic");
  assert.ok(drainCalls >= 2, `drain must run once live (got ${drainCalls})`);
});

// Gap C from payroll parity v2: static `page.evaluate` still returned
// "[object Object]" because the JSON round-trip was only wired for the live
// path. The static engine returns the trailing expression as a string, so
// wrap the user script similarly.
test("Page.evaluate returns typed values from a STATIC page too", async () => {
  const p = newPage();
  await p.setContent("<title>Home</title><h1>Welcome</h1>");
  assert.deepEqual(
    await p.evaluate(() => ({
      title: document.title,
      h1: document.querySelector("h1").textContent,
    })),
    { title: "Home", h1: "Welcome" },
  );
  assert.equal(await p.evaluate(() => 42), 42);
  assert.deepEqual(await p.evaluate(() => [1, 2, 3]), [1, 2, 3]);
});

// APIRequestContext (page.request / context.request) used to use an empty
// cookie jar, so specs that logged in via page.goto + session cookie and then
// hit an API with `context.request.get(...)` sent no cookies at all.
test("page.request / context.request share the BrowserContext cookie jar (#230)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/login") {
      res.writeHead(200, { "set-cookie": "sid=abc123; Path=/", "content-type": "text/html" });
      return res.end("<title>in</title>");
    }
    if (req.url === "/api/me") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ cookie: req.headers.cookie ?? "" }));
    }
    res.writeHead(404);
    res.end();
  });
  const base = await listen(server);
  const ctx = new BrowserContext();
  try {
    const p = await ctx.newPage();
    await p.goto(`${base}/login`);
    const r = await p.request.fetch(`${base}/api/me`);
    const body = await r.json();
    assert.equal(body.cookie, "sid=abc123", "page.request must send the login cookie");
    // context.request() mirrors page.request().
    const r2 = await ctx.request.fetch(`${base}/api/me`);
    const body2 = await r2.json();
    assert.equal(body2.cookie, "sid=abc123", "context.request must send the login cookie");
  } finally {
    await ctx.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Focus model: `.focus()`, `.blur()`, `toBeFocused` were static no-ops or
// always-fail assertions. The live isolate already tracks `document.activeElement`
// per DOM spec; drive focus/blur into it and read activeElement back for the
// matcher so keyboard-navigation specs don't get false negatives.
test("locator.focus() + expect.toBeFocused() reflect document.activeElement in the live app", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(`${base}/spa`, { waitUntil: "networkidle" });
    await p.setContent("<input id='a'><input id='b'><button id='go'>Go</button>");
    await p._openLiveSession();
    await p.locator("#a").focus();
    await expect(p.locator("#a")).toBeFocused();
    await expect(p.locator("#b")).not.toBeFocused();
    await p.locator("#b").focus();
    await expect(p.locator("#b")).toBeFocused();
    await expect(p.locator("#a")).not.toBeFocused();
    await p.locator("#b").blur();
    await expect(p.locator("#b")).not.toBeFocused();
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// page.route(fulfill) is the single most-used interception shape in payroll-app
// specs: stub /api/foo with a fixture so the SPA hydrates with known data.
// With no native request-interception hook, do it shim-side by monkey-patching
// `fetch` in the live isolate (via an init script, before the app runs) and
// route that single call to the collected fulfill response.
test("page.route(url, fulfill) returns a fixture to the live app's fetch", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/app") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>app</title><div id="root"></div><script>
           fetch('/api/foo')
             .then((r) => r.json())
             .then((j) => { document.getElementById('root').textContent = j.hello; })
             .catch((e) => { document.getElementById('root').textContent = 'ERR:' + e.message; });
         </script>`,
      );
    } else {
      // The real server MUST NOT be hit on /api/foo — the route must intercept.
      res.writeHead(500, { "content-type": "application/json" });
      res.end('{"hello":"real-server"}');
    }
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.route("/api/foo", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ hello: "mocked" }),
      });
    });
    await p.goto(`${base}/app`, { waitUntil: "networkidle" });
    assert.equal(
      await p.locator("#root").textContent(),
      "mocked",
      "route.fulfill fixture must win over the real server response",
    );
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Payroll-style glob: `page.route("**/api/x**", ...)` with the fetch fired from
// a parser-synchronous inline <script> and a URL that carries a query string.
// The glob translator must escape `*` so the two-star prefix/suffix become `.*`
// (matching origin + query), otherwise the init-script's matcher rejects every
// URL and the real server wins (turbo-surf#240).
test("page.route glob with ** matches an inline-script fetch carrying a query string", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/countries") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>app</title><div id="root"></div><script>
           fetch('/api/tax-store-data?code=42&tenant=acme')
             .then((r) => r.json())
             .then((j) => { document.getElementById('root').textContent = j.earningCode; })
             .catch((e) => { document.getElementById('root').textContent = 'ERR:' + e.message; });
         </script>`,
      );
    }
    res.writeHead(500, { "content-type": "application/json" });
    res.end('{"earningCode":"REAL"}');
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.route("**/api/tax-store-data**", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ earningCode: "FAKE" }),
      });
    });
    await p.goto(`${base}/countries`, { waitUntil: "networkidle" });
    assert.equal(
      await p.locator("#root").textContent(),
      "FAKE",
      "** glob pattern must intercept the inline <script> fetch",
    );
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Regression for turbo-surf#281: a handler that branches on `req.url()` must
// see the LIVE request URL, not the registration-time pattern. The pre-fix
// shim ran the handler once at registration with `req.url()` returning `**/*`,
// so `includes('/api')` was false, the fulfill branch never ran, and the route
// became a silent no-op for real requests.
test("page.route handler branches on live req.url() instead of the registration-time pattern (#281)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/app") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>app</title><div id="root"></div><script>
           fetch('/api/foo')
             .then((r) => r.json())
             .then((j) => { document.getElementById('root').textContent = j.who; })
             .catch((e) => { document.getElementById('root').textContent = 'ERR:' + e.message; });
         </script>`,
      );
    }
    res.writeHead(500, { "content-type": "application/json" });
    res.end('{"who":"real-server"}');
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.route("**/*", async (route, req) => {
      if (req.url().includes("/api")) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ who: "mocked" }),
        });
      } else {
        await route.continue();
      }
    });
    await p.goto(`${base}/app`, { waitUntil: "networkidle" });
    assert.equal(
      await p.locator("#root").textContent(),
      "mocked",
      "branching handler must see the live URL and fulfill /api requests",
    );
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// page.unroute(url) must remove a previously-registered route so the next live
// session sees the real server again (turbo-surf#220). Regression guard: without
// a refreshed init-script snapshot, the fixture would keep winning after unroute.
test("page.unroute drops a previously-registered route and lets the real server win", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/app") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>app</title><div id="root"></div><script>
           fetch('/api/foo')
             .then((r) => r.json())
             .then((j) => { document.getElementById('root').textContent = j.hello; })
             .catch((e) => { document.getElementById('root').textContent = 'ERR:' + e.message; });
         </script>`,
      );
    }
    res.writeHead(200, { "content-type": "application/json" });
    res.end('{"hello":"real-server"}');
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.route("/api/foo", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ hello: "mocked" }),
      });
    });
    await p.unroute("/api/foo");
    await p.goto(`${base}/app`, { waitUntil: "networkidle" });
    assert.equal(
      await p.locator("#root").textContent(),
      "real-server",
      "unroute must drop the fixture so the next session reaches the real server",
    );
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// page.route() registered AFTER goto must inject into the CURRENT live isolate
// so a subsequent fetch is intercepted — without re-navigation (turbo-surf#282).
// Regression guard: pre-#282, buildRouteInitScript only landed on the NEXT live
// session, so the post-goto fetch reached the real server.
test("page.route after goto intercepts a subsequent fetch in the current session", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/app") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(`<title>app</title><div id="root">init</div>`);
    }
    res.writeHead(200, { "content-type": "application/json" });
    res.end('{"hello":"real-server"}');
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(`${base}/app`, { waitUntil: "networkidle" });
    await p.route("**/api/orders", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ hello: "mocked-post-goto" }),
      });
    });
    const got = await p.evaluate(
      async () => (await fetch("/api/orders").then((r) => r.json())).hello,
    );
    assert.equal(
      got,
      "mocked-post-goto",
      "route registered after goto must inject into the live isolate (turbo-surf#282)",
    );
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// page.routeFromHAR(path) replays a Playwright/Chrome HAR file as a set of
// fixtures: for every entry in `log.entries`, the request URL is registered as
// a route returning that entry's response body/status/content-type — the live
// app's fetch/XHR resolves against those fixtures instead of the real server.
test("page.routeFromHAR plays back HAR entries as route fulfillments", async () => {
  const { writeFile: wf, mkdtemp } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const server = createServer((req, res) => {
    if (req.url === "/app") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<title>app</title><div id="root"></div><script>
           fetch('/api/countries')
             .then((r) => r.json())
             .then((j) => { document.getElementById('root').textContent = j.country; })
             .catch((e) => { document.getElementById('root').textContent = 'ERR:' + e.message; });
         </script>`,
      );
    }
    res.writeHead(500, { "content-type": "application/json" });
    res.end('{"country":"real-server"}');
  });
  const base = await listen(server);
  const dir = await mkdtemp(join(tmpdir(), "turbo-surf-har-"));
  const harPath = join(dir, "replay.har");
  const har = {
    log: {
      version: "1.2",
      entries: [
        {
          request: { method: "GET", url: `${base}/api/countries` },
          response: {
            status: 200,
            headers: [{ name: "Content-Type", value: "application/json" }],
            content: {
              mimeType: "application/json",
              text: JSON.stringify({ country: "FROM-HAR" }),
            },
          },
        },
      ],
    },
  };
  await wf(harPath, JSON.stringify(har));
  const p = newPage();
  try {
    await p.routeFromHAR(harPath);
    await p.goto(`${base}/app`, { waitUntil: "networkidle" });
    assert.equal(
      await p.locator("#root").textContent(),
      "FROM-HAR",
      "routeFromHAR must inject the HAR entry's response body",
    );
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Mirror of the waitForResponse drain-polling guard: payroll-style flows attach
// `waitForRequest(/api\/x/)` to the step that fires the request, and the stub
// used to resolve immediately with a synthetic page-URL GET — never seeing the
// real request. waitForRequest must drain the native log and return the actual
// matching request (not the synthetic placeholder) when one is produced.
test("waitForRequest polls the native drain and returns the matching request", async () => {
  const p = newPage();
  p._session = 1;
  p._actionSeq = 1;
  p._recentResponses = [];
  let drainCalls = 0;
  const fakeReq = () => ({ url: () => "https://x/api/widgets", method: () => "POST" });
  p._drainNetworkEvents = async () => {
    drainCalls++;
    if (drainCalls >= 2) {
      const req = fakeReq();
      p._emit("request", req);
    }
  };
  const got = await p.waitForRequest((r) => r.url().includes("/api/widgets"), { timeout: 500 });
  assert.equal(got.method(), "POST");
  assert.equal(got.url(), "https://x/api/widgets");
  assert.ok(drainCalls >= 2, `drain must be polled while waiting (got ${drainCalls})`);
});

// Without an action to drive `_drainNetworkEvents`, the native network log keeps
// sitting on the matching entry while waitForResponse burns its full timeout —
// payroll-app saw ~15 s for a response that landed in ~5 ms. In a live session
// waitForResponse must poll the drain itself, not just wait for someone else to
// push into the listener bus.
test("waitForResponse polls the native drain while waiting (live session)", async () => {
  const p = newPage();
  p._session = 1; // pretend a live session is open
  p._actionSeq = 1;
  p._recentResponses = [];
  let drainCalls = 0;
  p._drainNetworkEvents = async () => {
    drainCalls++;
    if (drainCalls >= 2) {
      const resp = { url: () => "https://x/api/data", status: () => 418, _seq: 1 };
      p._recentResponses.push(resp);
      p._emit("response", resp);
    }
  };
  const got = await p.waitForResponse((r) => r.url().includes("/api/data"), { timeout: 500 });
  assert.equal(got.status(), 418, "must resolve the drained response, not the synthetic timeout");
  assert.ok(drainCalls >= 2, `drain must be polled while waiting (got ${drainCalls})`);
});

test("waitForResponse rejects previously buffered responses from the current action", async () => {
  const p = newPage();
  const mk = (url, status, seq) => ({ url: () => url, status: () => status, _seq: seq });
  p._actionSeq = 3;
  p._recentResponses = [mk("https://x/api/details", 200, 3)]; // drained THIS action, pre-call
  await assert.rejects(
    p.waitForResponse((r) => r.url().includes("/api/details"), { timeout: 10 }),
    { name: "TimeoutError" },
  );
});

// Locator.getByTestId must scope to the parent locator's subtree (like .locator()), not the
// whole document — e.g. a UserMenu wrapper that also has a same-test-id twin in a mobile menu.
// Regression guard: it used to delegate to page.getByTestId and return BOTH.
test("Locator.getByTestId scopes to the parent locator subtree", async () => {
  const p = newPage();
  await p.setContent(
    "<div data-testid='wrap'><button data-testid='go'>in</button></div>" +
      "<button data-testid='go'>out</button>",
  );
  assert.equal(await p.getByTestId("go").count(), 2, "unscoped sees both");
  const scoped = p.getByTestId("wrap").getByTestId("go");
  assert.equal(await scoped.count(), 1, "scoped sees only the descendant");
  assert.equal(await scoped.textContent(), "in", "scoped resolves the in-subtree match");
});

// Locator.getByRole/getByText/getByLabel scope to the parent locator's subtree (descendant
// matching), not the whole document — e.g. `step.getByTestId('x').getByRole('combobox')`.
test("Locator.getByRole scopes to the parent locator subtree", async () => {
  const p = newPage();
  await p.setContent("<div data-testid='a'><button>in</button></div><button>out</button>");
  assert.equal(await p.getByRole("button").count(), 2, "unscoped sees both buttons");
  const scoped = p.getByTestId("a").getByRole("button");
  assert.equal(await scoped.count(), 1, "scoped sees only the in-subtree button");
  assert.equal(await scoped.textContent(), "in", "scoped resolves the descendant");
});

// about:blank (and other about: URLs) are not network resources — a browser shows an empty
// document with no request. The shim used to `fetchWithCookies('about:blank')`, which hit the
// net layer and errored ("builder error for url (about://blank)"); a reload of a blank page
// (e.g. login helper's waitForLoginForm reload before any navigation) then threw/hung. goto +
// reload of about:blank must yield a clean empty document with no fetch.
test("goto('about:blank') yields a blank document with no network fetch", async () => {
  const p = newPage();
  const resp = await p.goto("about:blank");
  assert.equal(p.url(), "about:blank");
  assert.equal(resp.status(), 200);
  const html = await p.content();
  assert.match(html, /<body>\s*<\/body>/, "blank body, no error markup");
  // reload of the blank page must also be a no-fetch no-op (the failing real-world path)
  await p.reload();
  assert.equal(p.url(), "about:blank");
});

test("request metadata and client hints survive hydration and stay context scoped", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push({ path: req.url, headers: req.headers });
    if (req.url === "/start") {
      res.writeHead(200, {
        "content-type": "text/html",
        "accept-ch": "Sec-CH-UA-Arch",
        "set-cookie": "sid=context; Path=/",
      });
      return res.end(
        '<html><head><link rel="stylesheet" href="/site.css"></head><body><img src="/image.png"><script src="/bundle.js"></script></body></html>',
      );
    }
    if (req.url === "/bundle.js") {
      res.writeHead(200, { "content-type": "application/javascript" });
      return res.end(
        'fetch("/api").then(r=>r.text()).then(t=>document.body.setAttribute("data-api",t));',
      );
    }
    res.writeHead(200, { "content-type": req.url === "/site.css" ? "text/css" : "text/html" });
    res.end(
      req.url === "/api"
        ? "hydrated"
        : req.url === "/site.css"
          ? "body{color:red}"
          : "<body>next</body>",
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const context = new BrowserContext();
  const isolated = new BrowserContext();
  try {
    assert.ok(context._requestState != null, "build the current addon with request-state support");
    const page = await context.newPage();
    await page.goto(`${base}/start`, { waitUntil: "networkidle" });
    assert.equal(await page.locator("body").getAttribute("data-api"), "hydrated");
    await page._fetchLinkedCss();
    await page._fetchImages();
    await page.goto(`${base}/next`, { waitUntil: "commit" });
    const other = await isolated.newPage();
    await other.goto(`${base}/isolated`, { waitUntil: "commit" });
    for (const [path, dest, mode] of [
      ["/bundle.js", "script", "no-cors"],
      ["/api", "empty", "cors"],
      ["/site.css", "style", "no-cors"],
      ["/image.png", "image", "no-cors"],
      ["/next", "document", "navigate"],
    ]) {
      const headers = seen.find((entry) => entry.path === path)?.headers;
      assert.ok(headers, `missing ${path}`);
      assert.equal(headers["sec-fetch-dest"], dest, path);
      assert.equal(headers["sec-fetch-mode"], mode, path);
      assert.equal(headers["sec-fetch-site"], mode === "navigate" ? "none" : "same-origin", path);
      assert.equal(headers["sec-ch-ua-arch"], '"arm"', path);
      assert.equal(headers.cookie, "sid=context", path);
      if (mode !== "navigate") assert.equal(headers["sec-fetch-user"], undefined, path);
    }
    assert.equal(seen.find((entry) => entry.path === "/start").headers["sec-fetch-site"], "none");
    assert.equal(
      seen.find((entry) => entry.path === "/isolated").headers["sec-ch-ua-arch"],
      undefined,
    );
  } finally {
    await context.close();
    await isolated.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("live hydration retains native response policy, status and selected referrer", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/source") {
      res.writeHead(200, { "content-type": "text/html", "referrer-policy": "no-referrer" });
      res.end("<title>source</title>");
      return;
    }
    res.writeHead(404, {
      "content-type": "text/html",
      "content-security-policy": "script-src 'nonce-good'; style-src 'none'",
    });
    res.end(`<head></head><body><script>document.body.setAttribute('data-denied','ran')</script><script nonce=good>
      document.body.setAttribute('data-referrer',document.referrer);
      document.body.setAttribute('data-init',String(globalThis.trustedInit));
      document.addEventListener('securitypolicyviolation',event=>document.body.setAttribute('data-report',event.statusCode+' '+event.referrer));
      const style=document.createElement('style');style.textContent='.denied{}';document.head.append(style);
    </script></body>`);
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.addInitScript(() => {
      globalThis.trustedInit = true;
    });
    await page.goto(`${base}/source`, { waitUntil: "commit" });
    const response = await page.goto(`${base}/target`, { waitUntil: "networkidle" });
    assert.equal(response.status(), 404);
    assert.equal(await page.locator("body").getAttribute("data-denied"), null);
    assert.equal(await page.locator("body").getAttribute("data-referrer"), "");
    assert.equal(await page.locator("body").getAttribute("data-report"), "404 ");
    assert.equal(await page.locator("body").getAttribute("data-init"), "true");
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// page.evaluate in a live session used to stringify the returned value, so an
// object came back as "[object Object]" and tests had to JSON.stringify inside
// the expression + JSON.parse outside. The payroll-app parity probe surfaced it
// on observer-style specs that read page state as objects. Returns must now be
// structured values: primitives as themselves, arrays/plain objects deep-equal.
test("Page.evaluate returns typed values (object, array, number) from live session", async () => {
  const server = site();
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(`${base}/spa`, { waitUntil: "networkidle" });
    assert.deepEqual(await p.evaluate(() => ({ count: 3, items: ["a", "b"], nested: { x: 1 } })), {
      count: 3,
      items: ["a", "b"],
      nested: { x: 1 },
    });
    assert.equal(await p.evaluate(() => 42), 42);
    assert.deepEqual(await p.evaluate(() => [1, 2, 3]), [1, 2, 3]);
    assert.equal(await p.evaluate(() => true), true);
    assert.equal(await p.evaluate(() => "hello"), "hello");
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Playwright's addInitScript runs in EVERY frame of the live context — the main
// page plus every child frame — before that frame's own scripts execute. The
// engine wires the context's init script into FrameContexts and runs it in each
// new frame realm after environment install and before authored <script>s. This
// regression covers #231: the LIMITATIONS note used to flag this as ⚠️. Four
// frame flavors (child via <iframe src>, srcdoc, nested, dynamically created,
// blob URL) each stamp a top-frame body attribute based on whether the init
// global is visible — "true" everywhere means the init script reached that
// frame before its own scripts ran.
test("addInitScript applies to every child frame (src, srcdoc, nested, dynamic, blob)", async () => {
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    if (url.pathname === "/parent") {
      res.writeHead(200, { "content-type": "text/html" });
      // One parent page with every frame flavor embedded. Each frame's inline
      // script writes an attribute on the TOP document; the parent waits until
      // every expected attribute shows up (so dynamic/blob mounts have run).
      return res.end(
        `<body>
          <iframe name="plain" src="/child?mark=plain"></iframe>
          <iframe name="srcframe" srcdoc="<script>try{window.top.document.body.setAttribute('data-srcframe', String(globalThis.trustedInit));}catch(e){}</script>"></iframe>
          <iframe name="middle" src="/middle"></iframe>
          <script>
            setTimeout(() => {
              const d = document.createElement('iframe');
              d.name = 'dyn'; d.src = '/child?mark=dyn';
              document.body.appendChild(d);
              const b = new Blob(
                ["<script>try{window.top.document.body.setAttribute('data-blobframe', String(globalThis.trustedInit));}catch(e){}</scr"+"ipt>"],
                { type: "text/html" },
              );
              const u = URL.createObjectURL(b);
              const f = document.createElement('iframe');
              f.src = u;
              document.body.appendChild(f);
            }, 0);
          </script>
        </body>`,
      );
    }
    if (url.pathname === "/child") {
      const mark = url.searchParams.get("mark") ?? "child";
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<body><script>try{window.top.document.body.setAttribute('data-${mark}', String(globalThis.trustedInit));}catch(e){}</script></body>`,
      );
    }
    if (url.pathname === "/middle") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(`<body><iframe name="grand" src="/grandchild"></iframe></body>`);
    }
    if (url.pathname === "/grandchild") {
      res.writeHead(200, { "content-type": "text/html" });
      return res.end(
        `<body><script>try{window.top.document.body.setAttribute('data-grand', String(globalThis.trustedInit));}catch(e){}</script></body>`,
      );
    }
    res.writeHead(404);
    res.end();
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.addInitScript(() => {
      globalThis.trustedInit = true;
    });
    await page.goto(`${base}/parent`, { waitUntil: "networkidle" });
    const read = async () =>
      page.evaluate(() => {
        const out = {};
        for (const attr of document.body.attributes) out[attr.name] = attr.value;
        return out;
      });
    const attrs = await read();
    // Every frame flavor saw the init script's global before its own <script>.
    assert.equal(attrs["data-plain"], "true", "static <iframe src> child");
    assert.equal(attrs["data-srcframe"], "true", "srcdoc child");
    assert.equal(attrs["data-grand"], "true", "nested grandchild");
    assert.equal(attrs["data-dyn"], "true", "dynamically created child");
    assert.equal(attrs["data-blobframe"], "true", "blob URL child");
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// A getBy handle passed to the static waitForFunction resolves through the engine
// namespace (turbo-dom#222): no live session, the handle is still the element.
test("static waitForFunction resolves a getBy element handle", async () => {
  const server = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end('<button id="go">Save</button>');
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(`${base}/`, { waitUntil: "commit" });
    const handle = await p.getByRole("button", { name: "Save" }).elementHandle();
    assert.equal(await p.waitForFunction((el) => el && el.id, handle), "go");
    assert.equal(p._session, null);
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// turbo-surf#226: page.exposeFunction installs a persistent JS→host binding
// the page can call. The host handler fires with the serialized arguments.
// Fire-and-forget by design (host return values are NOT surfaced back to the
// page in this release — the queue drains after each live interaction).
test("page.exposeFunction routes page calls to the host and survives navigation", async () => {
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    res.writeHead(200, { "content-type": "text/html" });
    if (url.pathname === "/second") {
      res.end(
        `<title>t2</title><body><script>globalThis.__telemetry({ step: "second", at: 2 });</script></body>`,
      );
      return;
    }
    res.end(
      `<title>t1</title><body><script>globalThis.__telemetry({ step: "first", at: 1 });</script></body>`,
    );
  });
  const base = await listen(server);
  const context = new BrowserContext();
  const captured = [];
  await context.exposeFunction("__telemetry", (payload) => {
    captured.push(payload);
  });
  const p = await context.newPage();
  try {
    await p.goto(`${base}/first`, { waitUntil: "networkidle" });
    // The inline script called the binding on the first page.
    assert.deepEqual(captured, [{ step: "first", at: 1 }]);
    // The second navigation re-runs the init script in the fresh isolate, so
    // the binding is there again — proving it persists across navigations.
    await p.goto(`${base}/second`, { waitUntil: "networkidle" });
    assert.deepEqual(captured, [
      { step: "first", at: 1 },
      { step: "second", at: 2 },
    ]);
    // The page can also call the binding from evaluate directly.
    await p.evaluate(() => globalThis.__telemetry({ step: "evaluate", at: 3 }));
    assert.deepEqual(captured[2], { step: "evaluate", at: 3 });
  } finally {
    await p.close();
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// page.exposeBinding passes a { page, context, frame } source record as the
// first arg to the host handler. The shim's frame is null (single-frame page
// model), but page + context are the live objects the test spec drives.
test("page.exposeBinding receives a source record with the page + context", async () => {
  const server = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(`<body><script>globalThis.__bind("hi", 7);</script></body>`);
  });
  const base = await listen(server);
  const p = newPage();
  const seen = [];
  await p.exposeBinding("__bind", (source, label, n) => {
    seen.push({ label, n, hasPage: source.page === p, hasContext: !!source.context });
  });
  try {
    await p.goto(`${base}/`, { waitUntil: "networkidle" });
    assert.deepEqual(seen, [{ label: "hi", n: 7, hasPage: true, hasContext: true }]);
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// turbo-surf#285: init scripts run per-frame, so an iframe realm keeps its
// own __tsExpose.q. The drain must walk the frame tree or iframe-scoped
// binding calls are lost.
test("page.exposeBinding fires when called from inside an iframe", async () => {
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    res.writeHead(200, { "content-type": "text/html" });
    if (url.pathname === "/child") {
      res.end(`<body><script>globalThis.__childBind("from-iframe", 42);</script></body>`);
      return;
    }
    res.end(`<body><iframe src="/child"></iframe></body>`);
  });
  const base = await listen(server);
  const p = newPage();
  const seen = [];
  await p.exposeBinding("__childBind", (_source, label, n) => {
    seen.push({ label, n });
  });
  try {
    await p.goto(`${base}/`, { waitUntil: "networkidle" });
    assert.deepEqual(seen, [{ label: "from-iframe", n: 42 }]);
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Wiring an expose AFTER the live session is up: the shim re-defines the
// binding in the running isolate so the current page can call it without a
// reload. Covers the "spec adds instrumentation between goto and the action"
// shape.
test("page.exposeFunction after goto is callable in the current session", async () => {
  const server = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(`<body><button id="go">go</button></body>`);
  });
  const base = await listen(server);
  const p = newPage();
  const beats = [];
  try {
    await p.goto(`${base}/`, { waitUntil: "networkidle" });
    await p.exposeFunction("__beat", (tag) => beats.push(tag));
    await p.evaluate(() => globalThis.__beat("after-goto"));
    assert.deepEqual(beats, ["after-goto"]);
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("successful locator assertions read each observed value once (#487)", async () => {
  const page = await fresh();
  for (const [method, args, reader] of [
    ["toHaveCount", [3], "count"],
    ["toHaveText", ["one"], "_snapshot"],
    ["toContainText", ["on"], "_snapshot"],
    ["toHaveValue", ["init"], "_snapshot"],
    ["toHaveAttribute", ["id", "name"], "getAttribute"],
  ]) {
    const loc = page.locator(
      method === "toHaveCount" ? "li" : method.includes("Text") ? "li:first-child" : "#name",
    );
    let reads = 0;
    const original = loc[reader];
    loc[reader] = function (...args) {
      reads++;
      return original.apply(this, args);
    };
    await expect(loc)[method](...args);
    assert.equal(reads, 1, method);
  }
  await page.close();
});

test("API context retains its connection pool across uncached requests (#485)", async () => {
  const connections = new Set();
  let requests = 0;
  const server = createServer((req, res) => {
    connections.add(req.socket);
    res.setHeader("cache-control", "no-store");
    res.end(JSON.stringify({ sequence: ++requests }));
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    for (let sequence = 1; sequence <= 5; sequence++) {
      const response = await api.get(base);
      assert.equal(response.status(), 200);
      assert.deepEqual(await response.json(), { sequence });
    }
    assert.equal(connections.size, 1);
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("visibility assertions request only the visibility predicate (#486)", async () => {
  const page = await fresh();
  for (const [selector, method, negate] of [
    ["#go", "toBeVisible", false],
    ["#hidden", "toBeHidden", false],
    ["#hidden", "toBeVisible", true],
    ["#go", "toBeHidden", true],
    ["#missing", "toBeHidden", false],
  ]) {
    const loc = page.locator(selector);
    loc._snapshot = () => {
      throw new Error("visibility must not materialize a snapshot");
    };
    let calls = 0;
    const original = loc.isVisible;
    loc.isVisible = function (...args) {
      calls++;
      return original.apply(this, args);
    };
    await (negate ? expect(loc).not : expect(loc))[method]({ timeout: 0 });
    assert.equal(calls, 1);
  }
  await page.close();
});

test("API string and Buffer data default to octet-stream (#435)", async () => {
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    res.end(
      JSON.stringify({ type: req.headers["content-type"], body: Buffer.concat(chunks).toString() }),
    );
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    for (const data of ["hello", Buffer.from("hello")]) {
      assert.deepEqual(await (await api.post(base, { data })).json(), {
        type: "application/octet-stream",
        body: "hello",
      });
    }
    assert.equal(
      (await (await api.post(base, { data: {}, headers: { "content-TYPE": "text/plain" } })).json())
        .type,
      "text/plain",
    );
    assert.equal((await (await api.post(base, { data: {} })).json()).type, "application/json");
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});
test("getByTestId regex preserves flags, scope and repeated global matching (#477)", async () => {
  const page = newPage();
  await page.setContent(
    '<div id="scope"><button data-testid="item-1">one</button><button data-testid="ITEM-2">two</button></div><button data-testid="item-3">outside</button><div data-testid="other">other</div>',
  );
  assert.equal(await page.getByTestId("item-1").count(), 1);
  const items = page.getByTestId(/^item-/gi);
  assert.equal(await items.count(), 3);
  assert.equal(await items.count(), 3);
  assert.equal(
    await page
      .locator("#scope")
      .getByTestId(/^item-/i)
      .count(),
    2,
  );
  assert.equal(await page.getByTestId(/missing/).count(), 0);
  await page._ensureLive();
  await page.evaluate(
    "document.querySelector('[data-testid=\"item-1\"]').onclick = () => document.body.setAttribute('data-clicked', 'yes')",
  );
  await page.getByTestId(/^item-1$/).click();
  assert.equal(await page.locator("body").getAttribute("data-clicked"), "yes");
  await page.close();
});

test("API contexts apply userAgent and scoped Basic credentials (#433)", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push(req.headers.authorization ?? null);
    if (req.url === "/auth" && !req.headers.authorization) {
      res.writeHead(401, { "www-authenticate": 'Basic realm="probe"' });
    }
    res.end(
      JSON.stringify({ agent: req.headers["user-agent"], auth: req.headers.authorization ?? null }),
    );
  });
  const base = await listen(server);
  const credentials = { username: "alice", password: "secret", origin: base };
  const auth = `Basic ${Buffer.from("alice:secret").toString("base64")}`;
  const api = await request.newContext({ userAgent: "api-probe/1", httpCredentials: credentials });
  const always = await request.newContext({ httpCredentials: { ...credentials, send: "always" } });
  const outside = await request.newContext({
    httpCredentials: { ...credentials, origin: "http://example.invalid", send: "always" },
  });
  try {
    assert.deepEqual(await (await api.get(`${base}/agent`)).json(), {
      agent: "api-probe/1",
      auth: null,
    });
    seen.length = 0;
    const response = await api.get(`${base}/auth`);
    assert.equal(response.status(), 200);
    assert.deepEqual(seen, [null, auth]);
    assert.equal((await (await always.get(base)).json()).auth, auth);
    assert.equal((await (await outside.get(base)).json()).auth, null);
  } finally {
    await api.dispose();
    await always.dispose();
    await outside.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API per-request headers override context names case-insensitively (#432)", async () => {
  const server = createServer((req, res) => res.end(req.headers["x-probe"]));
  const base = await listen(server);
  const api = await request.newContext({ extraHTTPHeaders: { "x-probe": "context" } });
  try {
    assert.equal(
      await (await api.get(base, { headers: { "X-Probe": "request" } })).text(),
      "request",
    );
    assert.equal(await (await api.get(base)).text(), "context");
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API disposal rejects requests and retained response bodies (#431)", async () => {
  let requests = 0;
  const server = createServer((req, res) => {
    requests++;
    res.end("body");
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    const response = await api.get(base);
    await api.dispose();
    await api.dispose();
    await assert.rejects(api.get(base), /disposed|closed/i);
    await assert.rejects(response.text(), /disposed|closed/i);
    assert.equal(requests, 1);
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("successive first last and nth select the existing singleton (#473)", async () => {
  const page = newPage();
  await page.setContent('<button id="one">one</button><button id="two">two</button>');
  const buttons = page.locator("button");
  assert.equal(await buttons.nth(1).filter({ hasText: "" }).count(), 1);
  assert.equal(await buttons.last().first().getAttribute("id"), "two");
  assert.equal(await buttons.first().last().getAttribute("id"), "one");
  assert.equal(await buttons.last().nth(-1).first().getAttribute("id"), "two");
  assert.equal(await buttons.last().nth(1).first().count(), 0);
  assert.equal(await buttons.first().nth(-2).last().count(), 0);
  assert.equal(await page.locator(".missing").last().first().count(), 0);
  await page._ensureLive();
  await page.evaluate(
    "document.querySelector('#two').onclick = () => document.body.setAttribute('data-clicked','two')",
  );
  await buttons.last().first().click();
  assert.equal(await page.locator("body").getAttribute("data-clicked"), "two");
  await page.close();
});

test("selector counts avoid materializing match payloads (#461)", async () => {
  const page = await fresh();
  const loc = page.locator("li");
  loc._resolve = () => {
    throw new Error("count must not serialize match payloads");
  };
  assert.equal(await loc.count(), 3);
  assert.equal(await loc.first().count(), 1);
  assert.equal(await loc.last().count(), 1);
  assert.equal(await loc.nth(3).count(), 0);
  assert.equal(await loc.nth(-4).count(), 0);
  assert.equal(await loc.nth(0.5).count(), 0);
  assert.equal(await page._locFromSelector("//li", "xpath").count(), 3);
  assert.equal(await page.locator("li").filter({ hasText: "one" }).count(), 1);
  assert.equal(await page.locator("#missing").count(), 0);
  await page.close();
});
test("screenshot encodes JPEG type, inferred paths, and quality (#467)", async () => {
  const p = newPage();
  const directory = await mkdtemp(join(tmpdir(), "surf-jpeg-"));
  await p.setContent("<style>body{background:linear-gradient(red,blue)}</style><p>jpeg output</p>");
  try {
    const png = await p.screenshot({ images: false, externalCss: false });
    assert.equal(png.subarray(0, 4).toString("hex"), "89504e47");
    const low = await p.screenshot({ type: "jpeg", quality: 10 });
    const high = await p.screenshot({ type: "jpeg", quality: 90 });
    assert.equal(low.subarray(0, 2).toString("hex"), "ffd8");
    assert.equal(high.subarray(0, 2).toString("hex"), "ffd8");
    assert.ok(!low.equals(high));
    const path = join(directory, "inferred.JPG");
    const inferred = await p.screenshot({ path });
    assert.equal(inferred.subarray(0, 2).toString("hex"), "ffd8");
    assert.deepEqual(await readFile(path), inferred);
    await assert.rejects(() => p.screenshot({ type: "jpeg", quality: 101 }), /quality/);
    await assert.rejects(() => p.screenshot({ type: "png", quality: 50 }), /quality/);
    await assert.rejects(() => p.screenshot({ type: "unsupported" }), /type/);
  } finally {
    await p.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("Frame URLs resolve relative src before URL lookup (#448)", async () => {
  const p = newPage();
  p._url = "https://example.test/parent/page";
  await p.setContent('<iframe name="child" src="../remote"></iframe>');
  assert.equal(p.frame("child").url(), "https://example.test/remote");
  assert.equal(p.frame({ url: "https://example.test/remote" }).name(), "child");
  assert.equal(p.frame({ url: (url) => url.pathname === "/remote" }).name(), "child");
  await p.close();
});

test("Frame registry preserves nested identity and detach events (#447)", async () => {
  const p = newPage();
  const events = [];
  for (const event of ["frameattached", "framenavigated", "framedetached"])
    p.on(event, (frame) => events.push([event, frame]));
  await p.setContent(
    '<iframe name="outer" srcdoc="&lt;iframe name=inner srcdoc=&quot;&amp;lt;p&amp;gt;child&amp;lt;/p&amp;gt;&quot;&gt;&lt;/iframe&gt;"></iframe>',
  );
  const outer = p.frame("outer");
  assert.equal(p.frame("outer"), outer);
  assert.equal(outer.parentFrame(), p.mainFrame());
  assert.deepEqual(p.mainFrame().childFrames(), [outer]);
  const inner = p.frame("inner");
  assert.ok(inner);
  assert.equal(inner.parentFrame(), outer);
  assert.deepEqual(
    p.frames().map((frame) => frame.name()),
    ["", "outer", "inner"],
  );
  await p.setContent("<p>replacement</p>");
  assert.equal(outer.isDetached(), true);
  assert.equal(inner.isDetached(), true);
  assert.ok(events.some(([event, frame]) => event === "frameattached" && frame === outer));
  assert.ok(events.some(([event, frame]) => event === "framedetached" && frame === inner));
  await p.close();
});

test("Live frame registry tracks nested frames and actual navigation URLs (#447)", async () => {
  const server = createServer((req, res) => {
    res.setHeader("content-type", "text/html");
    res.end(
      req.url === "/child"
        ? '<iframe name="nested" src="/nested"></iframe>'
        : req.url === "/nested"
          ? "<p>nested</p>"
          : '<iframe id="outer" name="outer" src="/child"></iframe>',
    );
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(base, { waitUntil: "networkidle" });
    const outer = p.frame("outer");
    assert.ok(p.frame("nested"));
    assert.equal(outer, p.frame("outer"));
    assert.equal(outer.parentFrame(), p);
    assert.equal(outer.url(), `${base}/child`);
    await p.evaluate(() => document.querySelector("#outer").remove());
    assert.equal(outer.isDetached(), true);
    assert.equal(p.frame("nested"), null);
  } finally {
    await p.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("FrameLocator.owner returns the indexed parent-scoped iframe locator (#446)", async () => {
  const p = newPage();
  await p.setContent(
    '<iframe id="first" srcdoc="&lt;iframe id=inner&gt;&lt;/iframe&gt;"></iframe><iframe id="second"></iframe>',
  );
  const owner = p.frameLocator("iframe").nth(1).owner();
  assert.equal(await owner.count(), 1);
  assert.equal(await owner.getAttribute("id"), "second");
  assert.equal(
    await p.frameLocator("#first").frameLocator("#inner").owner().getAttribute("id"),
    "inner",
  );
  await p.close();
});

test("Frame reads reflect source documents and later live mutations (#445)", async () => {
  const server = createServer((req, res) => {
    res.setHeader("content-type", "text/html");
    res.end(
      req.url === "/child"
        ? '<title>Child title</title><p id="label">original</p>'
        : '<iframe id="child" name="child" src="/child"></iframe>',
    );
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(base, { waitUntil: "networkidle" });
    const frame = p.frame("child");
    assert.equal(await frame.title(), "Child title");
    assert.equal(await frame.locator("#label").textContent(), "original");
    await p.evaluate(() => {
      document.querySelector("#child").contentDocument.querySelector("#label").textContent =
        "changed";
    });
    assert.equal(await frame.locator("#label").textContent(), "changed");
    assert.match(await frame.content(), /changed/);
  } finally {
    await p.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API Buffer request data preserves invalid UTF-8 bytes (#341)", async () => {
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    res.end(JSON.stringify(Array.from(Buffer.concat(chunks))));
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    const response = await api.post(base, { data: Buffer.from([0, 255, 128, 10]) });
    assert.deepEqual(await response.json(), [0, 255, 128, 10]);
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API multipart encodes fields and binary files (#434)", async () => {
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    res.end(
      JSON.stringify({
        type: req.headers["content-type"],
        body: Buffer.concat(chunks).toString("base64"),
      }),
    );
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    const response = await api.post(base, {
      multipart: {
        label: "value",
        file: {
          name: "probe.bin",
          mimeType: "application/octet-stream",
          buffer: Buffer.from([0, 255, 128, 10]),
        },
      },
    });
    const { type, body } = await response.json();
    assert.match(type ?? "", /^multipart\/form-data; boundary=/);
    const boundary = type.split("boundary=")[1];
    const bytes = Buffer.from(body, "base64");
    assert.ok(bytes.includes(Buffer.from('name="label"\r\n\r\nvalue\r\n')));
    assert.ok(
      bytes.includes(
        Buffer.from(
          'name="file"; filename="probe.bin"\r\nContent-Type: application/octet-stream\r\n\r\n',
        ),
      ),
    );
    assert.ok(bytes.includes(Buffer.from([0, 255, 128, 10])));
    assert.ok(bytes.toString().endsWith(`--${boundary}--\r\n`));
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API response headers use lowercase names and preserve status text (#338)", async () => {
  const server = createServer((req, res) => {
    res.statusCode = 429;
    res.statusMessage = "Probe Rate Limit";
    res.setHeader("Retry-After", "3");
    res.setHeader("Set-Cookie", ["a=1; Path=/", "b=2; Path=/"]);
    res.end("ok");
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    const response = await api.get(base);
    assert.equal(response.headers()["retry-after"], "3");
    assert.equal(response.headers()["set-cookie"], "a=1; Path=/\nb=2; Path=/");
    assert.equal(response.statusText(), "Probe Rate Limit");
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API request status and redirect policies are honored (#335)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/failure") res.statusCode = 500;
    if (req.url.startsWith("/redirect")) {
      res.statusCode = 302;
      res.setHeader("location", req.url === "/redirect-twice" ? "/redirect" : "/end");
    }
    res.end("body");
  });
  const base = await listen(server);
  const api = await request.newContext({ failOnStatusCode: true, maxRedirects: 1 });
  try {
    await assert.rejects(api.get(`${base}/failure`), /500/);
    assert.equal((await api.get(`${base}/failure`, { failOnStatusCode: false })).status(), 500);
    assert.equal((await api.get(`${base}/redirect`, { maxRedirects: 0 })).status(), 302);
    assert.equal((await api.get(`${base}/redirect`)).status(), 200);
    await assert.rejects(api.get(`${base}/redirect-twice`), /redirect/i);
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API params merge query strings and form fields encode as URL data (#333)", async () => {
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    res.end(
      JSON.stringify({
        path: req.url,
        type: req.headers["content-type"],
        body: Buffer.concat(chunks).toString(),
      }),
    );
  });
  const base = await listen(server);
  const api = await request.newContext({ baseURL: base });
  try {
    const query = await (
      await api.get("/echo?original=yes", { params: { search: "hello world", limit: 2 } })
    ).json();
    assert.equal(query.path, "/echo?original=yes&search=hello+world&limit=2");
    const form = await (await api.post("/echo", { form: { name: "hello world", n: 2 } })).json();
    assert.equal(form.body, "name=hello+world&n=2");
    assert.equal(form.type, "application/x-www-form-urlencoded");
    const repeated = await (
      await api.get("/echo", { params: new URLSearchParams("q=1&q=2") })
    ).json();
    assert.equal(repeated.path, "/echo?q=1&q=2");
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Standalone API contexts retain their own cookie jars and storage state (#331)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/set") res.setHeader("set-cookie", "session=token; Path=/; HttpOnly");
    res.end(req.headers.cookie ?? "");
  });
  const base = await listen(server);
  const api = await request.newContext();
  const isolated = await request.newContext();
  try {
    await api.get(`${base}/set`);
    assert.equal(await (await api.get(base)).text(), "session=token");
    assert.equal(await (await isolated.get(base)).text(), "");
    const stored = await api.storageState();
    assert.equal(stored.cookies.length, 1);
    assert.equal(stored.cookies[0].name, "session");
    const restored = await request.newContext({ storageState: stored });
    try {
      assert.equal(await (await restored.get(base)).text(), "session=token");
    } finally {
      await restored.dispose();
    }
  } finally {
    await api.dispose();
    await isolated.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Page and BrowserContext expose a shared request property with baseURL (#329)", async () => {
  const server = createServer((req, res) => res.end(req.url));
  const base = await listen(server);
  const context = new BrowserContext({ baseURL: base });
  const page = await context.newPage();
  try {
    assert.equal(typeof page.request.get, "function");
    assert.equal(page.request, context.request);
    assert.equal(await (await page.request.get("/echo")).text(), "/echo");
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

pw("Builtin request fixture supplies an API context (#329)", async ({ request: api }) => {
  assert.equal(typeof api.get, "function");
  assert.equal(typeof api.post, "function");
  assert.equal(api.newContext, undefined);
});
test("no-content navigation retains the active document and history (#428)", async () => {
  const server = createServer((req, res) => {
    if (req.url.startsWith("/empty")) {
      res.writeHead(Number(req.url.slice(6)));
      res.end();
      return;
    }
    res.end(
      '<p id="state">original</p><script>document.querySelector("#state").textContent="hydrated"</script>',
    );
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(`${base}/a`, { waitUntil: "networkidle" });
    for (const status of [204, 205]) {
      const session = page._session;
      await assert.rejects(page.goto(`${base}/empty${status}`), /ERR_ABORTED/);
      assert.equal(page.url(), `${base}/a`);
      assert.equal(await page.locator("#state").textContent(), "hydrated");
      assert.equal(page._session, session);
      assert.equal(await page.goBack(), null);
    }
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("screenshot clip crops output without changing viewport layout (#466)", async () => {
  const p = newPage({ viewport: { width: 100, height: 80 } });
  await p.setContent(
    "<style>body{margin:0}div{position:absolute;left:10px;top:10px;width:20px;height:20px;background:red}</style><div></div>",
  );
  try {
    const full = await p.screenshot();
    const clip = await p.screenshot({ clip: { x: 5, y: 5, width: 20, height: 10 } });
    assert.equal(full.readUInt32BE(16), 100);
    assert.equal(full.readUInt32BE(20), 80);
    assert.equal(clip.readUInt32BE(16), 20);
    assert.equal(clip.readUInt32BE(20), 10);
    const jpeg = await p.screenshot({ type: "jpeg", clip: { x: 5, y: 5, width: 20, height: 10 } });
    assert.equal(jpeg.subarray(0, 2).toString("hex"), "ffd8");
    for (const invalid of [
      { x: -1, y: 0, width: 20, height: 10 },
      { x: 0, y: 0, width: 0, height: 10 },
      { x: NaN, y: 0, width: 20, height: 10 },
    ])
      await assert.rejects(() => p.screenshot({ clip: invalid }), /clip/);
    await assert.rejects(
      () => p.screenshot({ clip: { x: 200, y: 200, width: 10, height: 10 } }),
      /clip/,
    );
  } finally {
    await p.close();
  }
});
test("nested scope selectors evaluate against each matched parent (#475)", async () => {
  const page = newPage();
  await page.setContent(
    '<section id="one"><button class="child">first</button><div><button class="child">nested</button></div></section><section id="two"><button class="child">outside</button></section>',
  );
  assert.equal(await page.locator("#one").locator(".child").count(), 2);
  assert.deepEqual(await page.locator("#one").locator(":scope > .child").allTextContents(), [
    "first",
  ]);
  assert.deepEqual(await page.locator("section").locator(":scope > .child").allTextContents(), [
    "first",
    "outside",
  ]);
  await page.close();
});

test("child reads honor indexed and filtered ancestors (#474)", async () => {
  const page = newPage();
  await page.setContent(
    '<div class="card" id="one"><button class="child">first</button></div><div class="card" id="two"><button class="child">second</button></div>',
  );
  const child = page.locator(".card").nth(-1).locator(".child");
  assert.equal(await child.count(), 1);
  assert.deepEqual(await child.allTextContents(), ["second"]);
  assert.equal(await child.getAttribute("class"), "child");
  assert.deepEqual(await page.locator(".card").last().getByRole("button").allTextContents(), [
    "second",
  ]);
  assert.deepEqual(
    await page.locator(".card").filter({ hasText: "second" }).locator(".child").allTextContents(),
    ["second"],
  );
  assert.equal(await page.locator(".card").nth(8).locator(".child").count(), 0);
  await page._ensureLive();
  await page.evaluate(
    "document.querySelector('#two button').onclick = () => document.body.setAttribute('data-clicked','two')",
  );
  await child.click();
  assert.equal(await page.locator("body").getAttribute("data-clicked"), "two");
  await page.close();
});
test("Frame.evaluate executes and awaits in the child realm and propagates errors (#444)", async () => {
  const server = createServer((req, res) => {
    res.setHeader("content-type", "text/html");
    res.end(
      '<title>Host</title><iframe name="child" srcdoc="&lt;title&gt;Child&lt;/title&gt;&lt;script&gt;globalThis.childOnly=42&lt;/script&gt;"></iframe>',
    );
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(base, { waitUntil: "networkidle" });
    const frame = p.frame("child");
    assert.deepEqual(
      await frame.evaluate(
        async (arg) => ({
          title: document.title,
          value: await Promise.resolve(globalThis.childOnly + arg),
        }),
        3,
      ),
      { title: "Child", value: 45 },
    );
    assert.equal(await frame.evaluate("document.title"), "Child");
    await assert.rejects(
      frame.evaluate(() => {
        throw new Error("child exploded");
      }),
      /child exploded/,
    );
    assert.equal(await p.evaluate(() => document.title), "Host");
  } finally {
    await p.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Frame locator interactions target the child and preserve nested frame scope (#443)", async () => {
  const server = createServer((req, res) => {
    res.setHeader("content-type", "text/html");
    res.end(
      "<button id=\"same\" onclick=\"document.body.setAttribute('data-clicked','host')\">Host</button><iframe name=\"child\" srcdoc=\"&lt;button id=same onclick=&quot;document.body.setAttribute('data-clicked','child')&quot;&gt;Child&lt;/button&gt;&lt;input id=entry&gt;\"></iframe>",
    );
  });
  const base = await listen(server);
  const p = newPage();
  try {
    await p.goto(base, { waitUntil: "networkidle" });
    const frame = p.frame("child");
    await frame.locator("#same").click();
    assert.equal(await p.evaluate(() => document.body.getAttribute("data-clicked")), null);
    assert.equal(await frame.evaluate(() => document.body.getAttribute("data-clicked")), "child");
    await frame.locator("#entry").fill("child value");
    assert.equal(await frame.evaluate(() => document.querySelector("#entry").value), "child value");
    await frame.getByRole("button", { name: "Child" }).click();
    await frame.evaluate(() => {
      const nested = document.createElement("iframe");
      nested.name = "nested";
      nested.srcdoc = `<button id=same onclick="document.body.setAttribute('data-clicked','nested')">Nested</button>`;
      document.body.appendChild(nested);
    });
    const nested = p.frame("nested");
    await nested.locator("#same").click();
    assert.equal(await nested.evaluate(() => document.body.getAttribute("data-clicked")), "nested");
    assert.equal(await frame.evaluate(() => document.body.getAttribute("data-clicked")), "child");
  } finally {
    await p.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("toHaveJSProperty reads typed properties in the live realm (#438)", async () => {
  const server = createServer((_req, res) =>
    res.end(
      '<button id="target">target</button><input id="field" value="0"><script>document.getElementById("target").custom={count:1};</script>',
    ),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await expect(page.locator("#target")).toHaveJSProperty("custom", { count: 1 }, { timeout: 20 });
    await expect(page.locator("#field")).toHaveJSProperty("value", "0", { timeout: 20 });
    await assert.rejects(() =>
      expect(page.locator("#field")).toHaveJSProperty("value", 0, { timeout: 20 }),
    );
    await expect(page.locator("#field")).not.toHaveJSProperty("value", 0, { timeout: 20 });
    await expect(page.locator("#target")).toHaveJSProperty("missing", undefined, { timeout: 20 });
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API, captured fetch and navigation responses expose raw body bytes (#400)", async () => {
  const bytes = Buffer.from([0, 255, 128, 10]);
  const html =
    '<script>fetch("/bytes").then(r => r.arrayBuffer()).then(() => globalThis.done = true)</script>';
  const server = createServer((req, res) => {
    res.setHeader("content-type", req.url === "/bytes" ? "application/octet-stream" : "text/html");
    res.end(req.url === "/bytes" ? bytes : html);
  });
  const base = await listen(server);
  const api = await request.newContext();
  const page = newPage();
  let captured;
  page.on("response", (response) => {
    if (response.url().endsWith("/bytes")) captured = response;
  });
  try {
    const response = await api.get(`${base}/bytes`);
    assert.deepEqual(await response.body(), bytes);
    const navigation = await page.goto(base, { waitUntil: "networkidle" });
    assert.deepEqual(await navigation.body(), Buffer.from(html));
    assert.ok(captured);
    assert.deepEqual(await captured.body(), bytes);
    assert.equal(await response.text(), bytes.toString());
  } finally {
    await api.dispose();
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("fill rejects invalid number text before mutating the value (#411)", async () => {
  const server = createServer((_request, response) =>
    response.end('<input id="field" type="number" value="12">'),
  );
  const base = await listen(server);
  const p = newPage();
  try {
    for (const live of [false, true]) {
      if (live) await p.goto(base, { waitUntil: "networkidle" });
      else await p.setContent('<input id="field" type="number" value="12">');
      await assert.rejects(
        () => p.locator("#field").fill("abc"),
        /Cannot type text into input\[type=number\]/,
      );
      assert.equal(await p.locator("#field").inputValue(), "12");
      if (live) assert.equal(await p.evaluate(() => document.querySelector("#field").value), "12");
      await p.locator("#field").fill("-2.5");
      const changed = live
        ? await p.evaluate(() => document.querySelector("#field").value)
        : await p.locator("#field").inputValue();
      assert.equal(changed, "-2.5");
    }
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("API pooled connections do not cache otherwise cacheable responses (#485)", async () => {
  let sequence = 0;
  const server = createServer((req, res) => {
    res.setHeader("cache-control", "public, max-age=3600");
    res.end(String(++sequence));
  });
  const base = await listen(server);
  const api = await request.newContext();
  try {
    assert.equal(await (await api.get(base)).text(), "1");
    assert.equal(await (await api.get(base)).text(), "2");
    assert.equal(sequence, 2);
  } finally {
    await api.dispose();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("CSS and id assertions accept regex and retain negation (#437)", async () => {
  const page = await fresh();
  await expect(page.locator("#go")).toHaveId("go", { timeout: 0 });
  await expect(page.locator("#go")).toHaveId(/^go$/, { timeout: 0 });
  await expect(page.locator("#go")).not.toHaveId(/^wrong$/, { timeout: 0 });
  await assert.rejects(() => expect(page.locator("#go")).toHaveId(/^wrong$/, { timeout: 0 }));
  await expect(page.locator("#hidden")).toHaveCSS("display", "none", { timeout: 0 });
  await expect(page.locator("#hidden")).toHaveCSS("display", /^none$/, { timeout: 0 });
  await expect(page.locator("#hidden")).not.toHaveCSS("display", /^block$/, { timeout: 0 });
  await assert.rejects(() =>
    expect(page.locator("#hidden")).toHaveCSS("display", /^block$/, { timeout: 0 }),
  );
  const globalPattern = /none/g;
  await expect(page.locator("#hidden")).toHaveCSS("display", globalPattern, { timeout: 0 });
  await expect(page.locator("#hidden")).toHaveCSS("display", globalPattern, { timeout: 0 });
  await page.close();
});

test("attribute expectations preserve absence and normalize existence options (#436)", async () => {
  const page = await fresh();
  await expect(page.locator("#go")).toHaveAttribute("id", { timeout: 0 });
  await expect(page.locator("#go")).toHaveAttribute("id", /^go$/, { timeout: 0 });
  await assert.rejects(() =>
    expect(page.locator("#go")).toHaveAttribute("data-absent", /^$/, { timeout: 0 }),
  );
  await expect(page.locator("#go")).not.toHaveAttribute("data-absent", /^$/, { timeout: 0 });
  await page.setContent('<button id="present" data-empty="">target</button>');
  await expect(page.locator("#present")).toHaveAttribute("data-empty", { timeout: 0 });
  await expect(page.locator("#present")).toHaveAttribute("data-empty", /^$/, { timeout: 0 });
  await expect(page.locator("#present")).not.toHaveAttribute("data-absent", { timeout: 0 });
  await page.close();
});

test("viewport assertions use intersection ratio rather than visibility (#430)", async () => {
  const server = createServer((_req, res) =>
    res.end(
      '<style>body{margin:0}#outside{position:absolute;top:2000px;width:100px;height:100px}#partial{position:absolute;top:180px;width:100px;height:100px}</style><div id="outside">outside</div><div id="partial">partial</div>',
    ),
  );
  const base = await listen(server);
  const context = new BrowserContext({ viewport: { width: 300, height: 200 } });
  const page = await context.newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await expect(page.locator("#outside")).not.toBeInViewport({ timeout: 0 });
    await assert.rejects(() => expect(page.locator("#outside")).toBeInViewport({ timeout: 0 }));
    await expect(page.locator("#partial")).toBeInViewport({ ratio: 0.1, timeout: 0 });
    await expect(page.locator("#partial")).toBeInViewport({ ratio: 0.2, timeout: 0 });
    await expect(page.locator("#partial")).not.toBeInViewport({ ratio: 0.5, timeout: 0 });
    await assert.rejects(() =>
      expect(page.locator("#partial")).toBeInViewport({ ratio: 0.5, timeout: 0 }),
    );
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("locator typing preserves caret text and emits cancelable character events (#410)", async () => {
  const server = createServer((_request, response) =>
    response.end(`<input id="field" value="old"><script>
    globalThis.events=[];
    const field=document.querySelector('#field');
    for(const type of ['keydown','keypress','beforeinput','input','keyup','change'])
      field.addEventListener(type,e=>events.push({type,data:e.data??null}));
    field.addEventListener('keydown',e=>{if(e.key==='x')e.preventDefault()});
    </script>`),
  );
  const base = await listen(server);
  const p = newPage();
  try {
    for (const method of ["type", "pressSequentially"]) {
      await p.goto(base, { waitUntil: "networkidle" });
      await p.evaluate(() => {
        const field = document.querySelector("#field");
        field.focus();
        field.setSelectionRange(0, 0);
      });
      await p.locator("#field")[method]("ab");
      assert.deepEqual(
        await p.evaluate(() => ({ value: document.querySelector("#field").value, events })),
        {
          value: "abold",
          events: ["a", "b"].flatMap((data) => [
            { type: "keydown", data: null },
            { type: "keypress", data: null },
            { type: "beforeinput", data },
            { type: "input", data },
            { type: "keyup", data: null },
          ]),
        },
      );
      await p.locator("#field")[method]("x");
      assert.equal(await p.evaluate(() => document.querySelector("#field").value), "abold");
    }
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("boolean locator assertions honor expected-state options and negation (#429)", async () => {
  const page = newPage();
  await page.setContent(
    '<input id="unchecked" type="checkbox"><input id="disabled" disabled><input id="readonly" readonly><div id="hidden" hidden>hidden</div><button id="shown">shown</button>',
  );
  const cases = [
    ["#unchecked", "toBeChecked", "checked"],
    ["#missing", "toBeAttached", "attached"],
    ["#disabled", "toBeEnabled", "enabled"],
    ["#readonly", "toBeEditable", "editable"],
    ["#hidden", "toBeVisible", "visible"],
  ];
  for (const [selector, matcher, option] of cases) {
    const assertion = expect(page.locator(selector));
    await assertion[matcher]({ [option]: false, timeout: 0 });
    await assertion.not[matcher]({ [option]: true, timeout: 0 });
    await assert.rejects(() => assertion.not[matcher]({ [option]: false, timeout: 0 }));
    await assert.rejects(() => assertion[matcher]({ [option]: true, timeout: 0 }));
  }
  await expect(page.locator("#shown")).toBeVisible({ visible: true, timeout: 0 });
  await expect(page.locator("#shown")).not.toBeVisible({ visible: false, timeout: 0 });
  await page.close();
});

test("fill and clear respect beforeinput cancellation and commit on blur (#409)", async () => {
  const server = createServer((_request, response) =>
    response.end(`<input id="field" value="old"><input id="blocked" value="old"><script>
    globalThis.events=[];
    for(const field of document.querySelectorAll('input')) for(const type of ['beforeinput','input','change'])
      field.addEventListener(type,e=>events.push({id:field.id,type,data:e.data??null,inputType:e.inputType??null}));
    document.querySelector('#blocked').addEventListener('beforeinput',e=>e.preventDefault());
    </script>`),
  );
  const base = await listen(server),
    p = newPage();
  try {
    for (const [id, value, inputType, data] of [
      ["field", "new", "insertText", "new"],
      ["field", "", "deleteContentForward", null],
      ["blocked", "new", "insertText", "new"],
    ]) {
      await p.goto(base, { waitUntil: "networkidle" });
      if (value) await p.locator("#" + id).fill(value);
      else await p.locator("#" + id).clear();
      const expected = [{ id, type: "beforeinput", data, inputType }];
      if (id !== "blocked") expected.push({ id, type: "input", data, inputType });
      assert.deepEqual(
        await p.evaluate((id) => ({ value: document.getElementById(id).value, events }), id),
        { value: id === "blocked" ? "old" : value, events: expected },
      );
      await p.evaluate((id) => document.getElementById(id).blur(), id);
      if (id !== "blocked") expected.push({ id, type: "change", data: null, inputType: null });
      assert.deepEqual(await p.evaluate(() => events), expected);
    }
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("fill edits contenteditable text with input events and a collapsed caret (#408)", async () => {
  const server = createServer((_request, response) =>
    response.end(`<div id="field" contenteditable="true"><b>old</b> text</div><script>
    globalThis.events=[];
    for(const type of ['beforeinput','input','change'])document.querySelector('#field').addEventListener(type,e=>events.push({type,data:e.data??null,inputType:e.inputType??null}));
    </script>`),
  );
  const base = await listen(server),
    p = newPage();
  try {
    await p.setContent('<div id="static" contenteditable="true">old</div>');
    await p.locator("#static").fill("static edit");
    assert.equal(
      await p.evaluate(() => document.querySelector("#static").textContent),
      "static edit",
    );
    await p.goto(base, { waitUntil: "networkidle" });
    await p.locator("#field").fill("new");
    assert.deepEqual(
      await p.evaluate(() => {
        const selection = getSelection();
        return {
          text: document.querySelector("#field").textContent,
          events,
          collapsed: selection.isCollapsed,
          offset: selection.anchorOffset,
        };
      }),
      {
        text: "new",
        events: [
          { type: "beforeinput", data: "new", inputType: "insertText" },
          { type: "input", data: "new", inputType: "insertText" },
        ],
        collapsed: true,
        offset: 3,
      },
    );
    await p.locator("#field").clear();
    assert.equal(await p.evaluate(() => document.querySelector("#field").textContent), "");
    await p.evaluate(() => document.querySelector("#field").blur());
    assert.equal(await p.evaluate(() => events.some((e) => e.type === "change")), false);
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Captured responses retain complete host-only header metadata (#398)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/data") {
      res.statusMessage = "Probe Status";
      res.setHeader("content-type", "application/json");
      res.setHeader("X-Probe", "present");
      res.setHeader("Set-Cookie", ["a=one; Path=/", "b=two; Path=/"]);
      res.end('{"ok":true}');
    } else res.end('<script>fetch("/data").then(r=>r.json())</script>');
  });
  const base = await listen(server);
  const page = newPage();
  let response;
  page.on("response", (candidate) => {
    if (candidate.url().endsWith("/data")) response = candidate;
  });
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    assert.equal(response.headers()["x-probe"], "present");
    assert.equal(response.headers()["set-cookie"], undefined);
    assert.equal(response.statusText(), "Probe Status");
    assert.equal((await response.allHeaders())["set-cookie"], "a=one; Path=/\nb=two; Path=/");
    assert.equal(await response.headerValue("X-PROBE"), "present");
    assert.equal(await response.headerValue("missing"), null);
    assert.deepEqual(
      (await response.headersArray())
        .filter((h) => h.name.toLowerCase() === "set-cookie")
        .map((h) => h.value),
      ["a=one; Path=/", "b=two; Path=/"],
    );
    assert.equal(
      await page.evaluate(() => fetch("/data").then((r) => r.headers.get("set-cookie"))),
      null,
    );
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("getBy parents retain their scope for CSS and getBy children (#472)", async () => {
  const page = newPage();
  await page.setContent(
    '<section role="group" aria-label="One"><button id="one">first</button></section><section role="group" aria-label="Two"><button id="two">second</button></section>',
  );
  const group = page.getByRole("group", { name: "One" });
  assert.deepEqual(await group.locator("button").allTextContents(), ["first"]);
  assert.deepEqual(await group.getByRole("button").allTextContents(), ["first"]);
  assert.equal(await page.getByRole("group", { name: "missing" }).locator("button").count(), 0);
  await page._ensureLive();
  await page.evaluate(
    "document.querySelector('#one').onclick = () => document.body.setAttribute('data-clicked','one')",
  );
  await group.getByRole("button").click();
  assert.equal(await page.locator("body").getAttribute("data-clicked"), "one");
  await page.close();
});
test("cleanup hooks all execute before their failures are reported", async () => {
  const { spawnSync } = await import("node:child_process");
  const { mkdtemp, writeFile } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const root = await mkdtemp(join(tmpdir(), "surf-cleanup-"));
  for (const kind of ["afterEach", "afterAll"]) {
    const file = join(root, `${kind}.mjs`);
    await writeFile(
      file,
      `import {test} from ${JSON.stringify(new URL("./index.mjs", import.meta.url).href)};
      test.${kind}(()=>{console.log('FIRST_CLEANUP');throw Error('first cleanup failure');});
      test.${kind}(()=>{console.log('SECOND_CLEANUP');throw Error('second cleanup failure');});
      test.${kind}(()=>console.log('THIRD_CLEANUP'));
      test('body',()=>console.log('BODY_COMPLETED'));`,
    );
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    const result = spawnSync(process.execPath, ["--test", "--test-force-exit", file], {
      env,
      encoding: "utf8",
      timeout: 10000,
    });
    assert.equal(result.status, 1, result.stdout + result.stderr);
    assert.match(result.stdout, /FIRST_CLEANUP/);
    assert.match(result.stdout, /SECOND_CLEANUP/);
    assert.match(result.stdout, /THIRD_CLEANUP/);
    assert.match(result.stdout, /first cleanup failure/);
    assert.match(result.stdout, /second cleanup failure/);
  }
});

test("test.fail distinguishes expected failures from unexpected passes", async () => {
  const { spawnSync } = await import("node:child_process");
  const { mkdtemp, writeFile } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const root = await mkdtemp(join(tmpdir(), "surf-expected-failure-"));
  for (const throws of [true, false]) {
    const file = join(root, `${throws}.mjs`);
    await writeFile(
      file,
      `import {test} from ${JSON.stringify(new URL("./index.mjs", import.meta.url).href)};
      test.fail('marked failure',()=>{${throws ? "throw Error('expected failure');" : "console.log('BODY_PASSED');"}});`,
    );
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    const result = spawnSync(process.execPath, ["--test", "--test-force-exit", file], {
      env,
      encoding: "utf8",
      timeout: 10000,
    });
    assert.equal(result.status, throws ? 0 : 1, result.stdout + result.stderr);
    if (!throws) assert.match(result.stdout, /Expected test to fail, but it passed/);
  }
});

test("navigation deadlines cancel slow goto and reload fetches (#427)", async () => {
  let slow = false;
  const server = createServer((_req, res) => {
    const send = () => res.end('<p id="state">loaded</p>');
    if (slow) setTimeout(send, 180);
    else send();
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "commit" });
    slow = true;
    for (const navigate of [
      () => page.goto(`${base}/slow`, { timeout: 30, waitUntil: "commit" }),
      () => page.reload({ timeout: 30, waitUntil: "commit" }),
    ]) {
      await assert.rejects(navigate(), (error) => error.name === "TimeoutError");
      await new Promise((done) => setTimeout(done, 220));
      assert.equal(page.url(), `${base}/`);
      assert.equal(await page.locator("#state").textContent(), "loaded");
    }
    await page.goto(`${base}/unlimited`, { timeout: 0, waitUntil: "commit" });
    assert.equal(page.url(), `${base}/unlimited`);
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("direct navigation referrers are explicit and reload retains them (#426)", async () => {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push(req.headers.referer ?? null);
    res.end("<title>referrer</title>");
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(`${base}/a`, { referer: `${base}/custom`, waitUntil: "networkidle" });
    assert.equal(seen.at(-1), `${base}/custom`);
    assert.equal(await page.evaluate(() => document.referrer), `${base}/custom`);
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(seen.at(-1), `${base}/custom`);
    assert.equal(await page.evaluate(() => document.referrer), `${base}/custom`);
    await page.goto(`${base}/b`, { waitUntil: "networkidle" });
    assert.equal(seen.at(-1), null);
    assert.equal(await page.evaluate(() => document.referrer), "");
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("history navigation honors networkidle hydration (#425)", async () => {
  const server = createServer((req, res) => {
    res.setHeader("cache-control", "no-store");
    res.end(
      `<p id="state">server ${req.url}</p><script>document.querySelector('#state').textContent='hydrated ${req.url}'</script>`,
    );
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(`${base}/a`, { waitUntil: "networkidle" });
    await page.goto(`${base}/b`, { waitUntil: "networkidle" });
    assert.equal((await page.goBack({ waitUntil: "networkidle" })).status(), 200);
    assert.equal(await page.locator("#state").textContent(), "hydrated /a");
    assert.equal((await page.goForward({ waitUntil: "networkidle" })).status(), 200);
    assert.equal(await page.locator("#state").textContent(), "hydrated /b");
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("clearing context cookies updates every active page jar (#424)", async () => {
  const server = createServer((_req, res) => {
    res.setHeader("set-cookie", ["session=old; Path=/", "scoped=yes; Path=/scoped"]);
    res.end("<title>cookies</title>");
  });
  const base = await listen(server);
  const context = new BrowserContext();
  try {
    const pages = [await context.newPage(), await context.newPage()];
    for (const page of pages) await page.goto(base, { waitUntil: "networkidle" });
    await context.clearCookies();
    assert.deepEqual(await context.cookies(), []);
    for (const page of pages) assert.equal(await page.evaluate(() => document.cookie), "");
    assert.deepEqual(await context.cookies(), []);
  } finally {
    await context.close();
    await new Promise((done) => server.close(done));
  }
});

test("context cookie URL filters honor host path and secure scope (#423)", async () => {
  const context = new BrowserContext();
  context.addCookies([
    { name: "root", value: "a", domain: "example.test", path: "/", host_only: true },
    { name: "scoped", value: "b", domain: "example.test", path: "/scoped", host_only: true },
    { name: "domain", value: "c", domain: ".example.test", path: "/" },
    { name: "secure", value: "d", domain: "example.test", path: "/", secure: true },
  ]);
  const names = async (urls) => (await context.cookies(urls)).map((cookie) => cookie.name).sort();
  assert.deepEqual(await names("http://example.test/elsewhere"), ["domain", "root"]);
  assert.deepEqual(await names("http://example.test/scoped/child"), ["domain", "root", "scoped"]);
  assert.deepEqual(await names("http://example.test/scopedness"), ["domain", "root"]);
  assert.deepEqual(await names("http://sub.example.test/"), ["domain"]);
  assert.deepEqual(await names("http://other.test/"), []);
  assert.deepEqual(await names(["https://example.test/", "http://other.test/"]), [
    "domain",
    "root",
    "secure",
  ]);
  assert.equal((await context.cookies()).length, 4);
  await context.close();
});

test("adding cookies replaces matching identities without losing other scopes (#422)", async () => {
  const context = new BrowserContext();
  const cookie = { name: "session", value: "old", domain: "example.test", path: "/" };
  context.addCookies([cookie, { ...cookie, path: "/scoped" }, { ...cookie, domain: "other.test" }]);
  context.addCookies([
    { ...cookie, value: "new" },
    { ...cookie, value: "latest" },
  ]);
  const cookies = await context.cookies();
  assert.equal(cookies.length, 3);
  assert.equal(
    cookies.find((entry) => entry.domain === "example.test" && entry.path === "/").value,
    "latest",
  );
  assert.equal(context.storageState().cookies.length, 3);
  assert.equal(cookies.filter((entry) => entry.value === "old").length, 2);
  await context.close();
});

test("textContent reads preserve raw descendant DOM text (#421)", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<div class="row">  A  <span hidden>B</span> C \n D  <!-- ignored --></div><div class="row"><script> x  y </script><style> a  b </style></div>',
    );
    const rows = page.locator(".row");
    assert.equal(await rows.first().textContent(), "  A  B C \n D  ");
    assert.deepEqual(await rows.allTextContents(), ["  A  B C \n D  ", " x  y  a  b "]);
    assert.equal(await page.locator(".missing").textContent(), null);
    assert.deepEqual(await page.locator(".missing").allTextContents(), []);
  } finally {
    await page.close();
  }
});

test("Live locator inputValue and value assertions read dirty DOM properties (#350)", async () => {
  const server = createServer((req, res) =>
    res.end("<input id=text value=old><textarea id=area>before</textarea>"),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.locator("#text").fill("new");
    assert.equal(await page.locator("#text").inputValue(), "new");
    assert.equal(await page.locator("#text").getAttribute("value"), "old");
    await expect(page.locator("#text")).toHaveValue("new", { timeout: 0 });
    await page.evaluate(() => {
      document.querySelector("#area").value = "after";
    });
    assert.equal(await page.locator("#area").inputValue(), "after");
    await expect(page.locator("#area")).toHaveValue("after", { timeout: 0 });
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Live checkbox actions execute one default and retain requested state (#351)", async () => {
  const server = createServer((req, res) =>
    res.end(`<input id=off type=checkbox><input id=on type=checkbox checked>
    <input id=canceled type=checkbox onclick="event.preventDefault()">
    <script>globalThis.changes=[]; for(const el of document.querySelectorAll('input')) el.addEventListener('change',()=>changes.push(el.id));</script>`),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.locator("#off").check();
    assert.equal(await page.evaluate(() => document.querySelector("#off").checked), true);
    assert.equal(await page.locator("#off").isChecked(), true);
    await expect(page.locator("#off")).toBeChecked({ timeout: 0 });
    await page.locator("#off").check();
    await page.locator("#on").uncheck();
    assert.equal(await page.locator("#on").isChecked(), false);
    await page.locator("#on").setChecked(true);
    await page.locator("#on").setChecked(false);
    assert.deepEqual(await page.evaluate(() => changes), ["off", "on", "on", "on"]);
    await assert.rejects(page.locator("#canceled").check(), /checked state|did not change/);
    assert.equal(await page.evaluate(() => document.querySelector("#canceled").checked), false);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("navigation deadlines also cancel slow hydration (#427)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/slow.js") {
      setTimeout(() => {
        res.setHeader("content-type", "application/javascript");
        res.end("document.body.dataset.loaded='yes'");
      }, 180);
      return;
    }
    res.end('<body><script src="/slow.js"></script></body>');
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await assert.rejects(
      page.goto(base, { timeout: 30, waitUntil: "networkidle" }),
      (error) => error.name === "TimeoutError",
    );
    assert.equal(page._session, null);
    await page.goto(base, { timeout: 0, waitUntil: "networkidle" });
    assert.equal(await page.evaluate(() => document.body.dataset.loaded), "yes");
    await assert.rejects(
      page.reload({ timeout: 30, waitUntil: "networkidle" }),
      (error) => error.name === "TimeoutError",
    );
    assert.equal(page._session, null);
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("socket failures emit requestfailed without response or requestfinished (#407)", async () => {
  const server = createServer((request, response) => {
    if (request.url === "/broken") {
      request.socket.destroy();
      return;
    }
    response.end(
      `<button id="run" onclick="fetch('/broken').catch(()=>globalThis.result='rejected')">fetch</button><script>globalThis.result='ready'</script>`,
    );
  });
  const base = await listen(server),
    p = newPage(),
    events = [];
  for (const type of ["requestfailed", "response", "requestfinished"])
    p.on(type, (entry) => {
      if (entry.url().endsWith("/broken"))
        events.push({ type, failure: type === "requestfailed" ? entry.failure() : null });
    });
  try {
    await p.goto(base, { waitUntil: "networkidle" });
    await p.locator("#run").click();
    let result;
    for (let i = 0; i < 30; i++) {
      result = await p.evaluate(() => globalThis.result);
      if (result === "rejected") break;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    assert.equal(result, "rejected");
    assert.deepEqual(
      events.map((entry) => entry.type),
      ["requestfailed"],
    );
    assert.equal(typeof events[0].failure.errorText, "string");
    assert.ok(events[0].failure.errorText.length > 0);
    assert.notEqual(
      events[0].failure.errorText,
      "net::ERR_FAILED",
      "retain the native transport error",
    );
    const failed = p.waitForEvent("requestfailed", { timeout: 1000 });
    await p.evaluate(() => {
      const xhr = new XMLHttpRequest();
      xhr.open("GET", "/broken");
      xhr.send();
    });
    const xhrFailure = (await failed).failure();
    assert.ok(xhrFailure.errorText.length > 0);
    assert.notEqual(xhrFailure.errorText, "net::ERR_FAILED");
  } finally {
    await p.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("addStyleTag inserts content URL and path sheets into the live document (#388)", async () => {
  const server = createServer((req, res) => {
    res.writeHead(req.url === "/missing.css" ? 404 : 200, { "content-type": "text/css" });
    res.end("#target { color: rgb(44, 55, 66) }");
  });
  const base = await listen(server);
  const directory = await mkdtemp(join(tmpdir(), "surf-style-"));
  const page = newPage();
  try {
    await page.setContent('<div id="target">target</div>');
    await page._ensureLive();
    const handle = await page.addStyleTag({ content: "#target { color: rgb(11, 22, 33) }" });
    assert.ok(handle);
    assert.equal(
      await page.evaluate(() => getComputedStyle(document.querySelector("#target")).color),
      "rgb(11, 22, 33)",
    );
    await page.addStyleTag({ url: `${base}/sheet.css` });
    assert.equal(
      await page.evaluate(() => getComputedStyle(document.querySelector("#target")).color),
      "rgb(44, 55, 66)",
    );
    const path = join(directory, "sheet.css");
    await writeFile(path, "#target { color: rgb(77, 88, 99) }");
    await page.addStyleTag({ path });
    assert.equal(
      await page.evaluate(() => getComputedStyle(document.querySelector("#target")).color),
      "rgb(77, 88, 99)",
    );
    await assert.rejects(page.addStyleTag({ url: `${base}/missing.css` }), /load|stylesheet/i);
    await assert.rejects(page.addStyleTag({}), /content|url|path/i);
  } finally {
    await page.close();
    await rm(directory, { recursive: true, force: true });
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("setExtraHTTPHeaders replaces and clears prior headers (#393)", async () => {
  const ctx = new BrowserContext();
  const p = await ctx.newPage();
  try {
    await ctx.setExtraHTTPHeaders({ "x-old": "old" });
    await ctx.setExtraHTTPHeaders({ "x-probe": "new" });
    assert.deepEqual(JSON.parse(ctx._headersJson()), { "x-probe": "new" });
    await ctx.setExtraHTTPHeaders({});
    assert.equal(ctx._headersJson(), null);
    await p.setExtraHTTPHeaders({ "x-old": "old" });
    await p.setExtraHTTPHeaders({ "x-probe": "new" });
    assert.deepEqual(JSON.parse(ctx._headersJson()), { "x-probe": "new" });
  } finally {
    await ctx.close();
  }
});

test("Numeric assertions reject coercion and compare matching infinities (#390)", () => {
  expect(Infinity).toBeCloseTo(Infinity);
  expect(-Infinity).toBeCloseTo(-Infinity);
  for (const name of [
    "toBeGreaterThan",
    "toBeGreaterThanOrEqual",
    "toBeLessThan",
    "toBeLessThanOrEqual",
    "toBeCloseTo",
  ]) {
    assert.throws(() => expect("1")[name](1), /number|numeric/i);
    assert.throws(() => expect(1)[name]("1"), /number|numeric/i);
    assert.throws(() => expect("1").not[name](1), /number|numeric/i);
  }
  expect(2n).toBeGreaterThan(1n);
  assert.throws(() => expect(NaN).toBeCloseTo(NaN));
  expect(NaN).toBe(NaN);
  assert.throws(() => expect(-0).toBe(0));
});

test("Asymmetric matchers compose recursively including negative forms (#389)", () => {
  expect({ id: 1, name: "Alice" }).toEqual(expect.objectContaining({ id: 1 }));
  expect([1, 2, 3]).toEqual(expect.arrayContaining([3, 1]));
  expect("payroll saved").toEqual(expect.stringContaining("saved"));
  expect("payroll saved").toEqual(expect.stringMatching(/saved$/));
  expect({ id: 1 }).toEqual({ id: expect.any(Number) });
  expect({ id: 1 }).toEqual({ id: expect.anything() });
  expect({ users: [{ id: 1, name: "Alice" }] }).toEqual(
    expect.objectContaining({
      users: expect.arrayContaining([
        expect.objectContaining({ name: expect.stringMatching(/^A/) }),
      ]),
    }),
  );
  expect({ id: 1 }).toEqual(expect.not.objectContaining({ id: 2 }));
  expect([1, 2]).toEqual(expect.not.arrayContaining([3]));
  assert.throws(() => expect({ id: "1" }).toEqual({ id: expect.any(Number) }));
  assert.throws(() => expect(null).toEqual(expect.anything()));
  assert.throws(() => expect("saved").toEqual(expect.not.stringContaining("saved")));
  assert.throws(() => expect({}).toEqual(expect.objectContaining({ missing: undefined })));
  expect(new Number(1)).toEqual(expect.any(Number));
});

test("Context media preferences reach native matching and stay isolated (#396)", async () => {
  const dark = new BrowserContext({ colorScheme: "dark", reducedMotion: "reduce" });
  const light = new BrowserContext({ colorScheme: "light", reducedMotion: "no-preference" });
  const p = await dark.newPage();
  const q = await light.newPage();
  try {
    await p.setContent("<p>media</p>");
    await q.setContent("<p>media</p>");
    await p._ensureLive();
    await q._ensureLive();
    assert.deepEqual(
      await p.evaluate(() => [
        matchMedia("(prefers-color-scheme: dark)").matches,
        matchMedia("(prefers-reduced-motion: reduce)").matches,
      ]),
      [true, true],
    );
    assert.deepEqual(
      await q.evaluate(() => [
        matchMedia("(prefers-color-scheme: dark)").matches,
        matchMedia("(prefers-reduced-motion: reduce)").matches,
      ]),
      [false, false],
    );
    await p.emulateMedia({ colorScheme: "light", reducedMotion: "no-preference" });
    assert.deepEqual(
      await p.evaluate(() => [
        matchMedia("(prefers-color-scheme: dark)").matches,
        matchMedia("(prefers-reduced-motion: reduce)").matches,
      ]),
      [false, false],
    );
  } finally {
    await dark.close();
    await light.close();
  }
});

test("toThrow supports error expectations and rejects invalid received values (#385)", () => {
  for (const expected of [/specific/, new Error("specific failure"), TypeError, "specific"]) {
    assert.doesNotThrow(() =>
      expect(() => {
        throw new TypeError("specific failure");
      }).toThrow(expected),
    );
    assert.throws(() =>
      expect(() => {
        throw new Error("different");
      }).toThrow(expected),
    );
  }
  assert.throws(() => expect(123).toThrow(), /function/);
  assert.throws(() => expect(123).not.toThrow(), /function/);
  assert.doesNotThrow(() =>
    expect(() => {
      throw undefined;
    }).toThrow(),
  );
  assert.doesNotThrow(() => expect(() => {}).not.toThrow());
  assert.throws(() => expect(() => {}).toThrow());
  assert.doesNotThrow(() =>
    expect(() => {
      throw new TypeError("specific failure");
    }).toThrowError(TypeError),
  );
});

test("toMatchObject preserves array length and typed value semantics (#384)", () => {
  assert.throws(() => expect([{ id: 1 }, { id: 2 }]).toMatchObject([{ id: 1 }]));
  assert.throws(() => expect({ at: new Date(1) }).toMatchObject({ at: new Date(2) }));
  assert.doesNotThrow(() => expect([{ id: 1, extra: true }]).toMatchObject([{ id: 1 }]));
  assert.doesNotThrow(() =>
    expect({ at: new Date(1), extra: true }).toMatchObject({ at: new Date(1) }),
  );
  assert.throws(() =>
    expect({ values: new Map([["a", 1]]) }).toMatchObject({ values: new Map([["a", 2]]) }),
  );
  assert.doesNotThrow(() => expect({ a: { b: 1, c: 2 } }).toMatchObject({ a: { b: 1 } }));
});

test("toHaveProperty distinguishes path existence and supports array paths (#383)", () => {
  assert.doesNotThrow(() => expect({ "a.b": { c: 1 } }).toHaveProperty(["a.b", "c"], 1));
  assert.doesNotThrow(() => expect({ a: undefined }).toHaveProperty("a"));
  assert.doesNotThrow(() => expect({ a: undefined }).toHaveProperty("a", undefined));
  assert.throws(() => expect({}).toHaveProperty("a", undefined));
  assert.throws(() => expect({ a: undefined }).not.toHaveProperty("a"));
  assert.doesNotThrow(() => expect({ a: [{ b: 2 }] }).toHaveProperty(["a", 0, "b"], 2));
  assert.doesNotThrow(() => expect({ a: [{ b: 2 }] }).toHaveProperty("a[0].b", 2));
  assert.doesNotThrow(() => expect({ a: null }).not.toHaveProperty("a.b"));
});

test("promise rejection assertions preserve not and error matchers (#382)", async () => {
  const rejected = (error) => {
    const promise = Promise.reject(error);
    promise.catch(() => {});
    return promise;
  };
  await assert.rejects(expect(rejected(new Error("same"))).rejects.not.toThrow("same"));
  await assert.doesNotReject(expect(rejected(new Error("other"))).rejects.not.toThrow("same"));
  await assert.doesNotReject(expect(rejected(new TypeError("same"))).rejects.toThrow(TypeError));
  await assert.doesNotReject(
    expect(rejected(new Error("same"))).rejects.toThrow(new Error("same")),
  );
  await assert.doesNotReject(expect(rejected(new Error("same"))).rejects.toThrow(/same/));
  await assert.doesNotReject(expect(Promise.resolve(2)).resolves.not.toBe(3));
  await assert.rejects(expect(Promise.resolve("same")).rejects.not.toThrow("other"));
});

test("deep equality handles typed values symbols cycles and strictness (#381)", () => {
  const token = Symbol("token");
  for (const [left, right] of [
    [new Date(0), new Date(1)],
    [new Map([["a", 1]]), new Map([["a", 2]])],
    [new Set([1]), new Set([2])],
    [/one/i, /two/g],
    [new Error("one"), new Error("two")],
    [{ [token]: 1 }, { [token]: 2 }],
    [new Uint8Array([1]), new Uint8Array([2])],
  ])
    assert.throws(() => expect(left).toEqual(right));
  assert.doesNotThrow(() => expect({ a: undefined }).toEqual({}));
  assert.throws(() => expect({ a: undefined }).toStrictEqual({}));
  class Employee {
    constructor() {
      this.id = 1;
    }
  }
  assert.doesNotThrow(() => expect(new Employee()).toEqual({ id: 1 }));
  assert.throws(() => expect(new Employee()).toStrictEqual({ id: 1 }));
  const left = { id: 1 };
  left.self = left;
  const right = { id: 1 };
  right.self = right;
  assert.doesNotThrow(() => expect(left).toEqual(right));
  assert.doesNotThrow(() => expect(left).toStrictEqual(right));
  assert.throws(() => expect(left).toEqual({ id: 1, self: { id: 2 } }));
  assert.throws(() => expect([new Map([["a", 1]])]).toContainEqual(new Map([["a", 2]])));
  assert.doesNotThrow(() =>
    expect(new Set([{ id: 1 }, { id: 2 }])).toEqual(new Set([{ id: 2 }, { id: 1 }])),
  );
  const a = new Map();
  a.set("self", a);
  const b = new Map();
  b.set("self", b);
  assert.doesNotThrow(() => expect(a).toEqual(b));
});

test("Context locale configures native language and Intl without site cookies (#395)", async () => {
  const requests = [];
  const server = createServer((req, res) => {
    requests.push(req.headers);
    res.setHeader("content-type", "text/html");
    res.end("<p>locale</p><script>globalThis.observedLocale=navigator.language</script>");
  });
  const base = await listen(server);
  const fr = new BrowserContext({ locale: "fr-FR" });
  const en = new BrowserContext({ locale: "en-US" });
  try {
    const p = await fr.newPage();
    const q = await en.newPage();
    await p.goto(base, { waitUntil: "networkidle" });
    await q.goto(base, { waitUntil: "networkidle" });
    assert.deepEqual(
      await p.evaluate(() => [
        navigator.language,
        ...navigator.languages,
        new Intl.NumberFormat().resolvedOptions().locale,
      ]),
      ["fr-FR", "fr-FR", "fr-FR"],
    );
    assert.equal(await p.evaluate(() => globalThis.observedLocale), "fr-FR");
    assert.equal(
      await p.evaluate(() => new Intl.NumberFormat("en-US").resolvedOptions().locale),
      "en-US",
    );
    assert.equal(await q.evaluate(() => navigator.language), "en-US");
    assert.equal(await p.evaluate(() => (1234.5).toLocaleString()), "1\u202f234,5");
    assert.equal(requests[0]["accept-language"], "fr-FR");
    assert.equal(requests[0].cookie, undefined);
    assert.deepEqual(
      (await fr.cookies()).filter((cookie) => cookie.name === "NEXT_LOCALE"),
      [],
    );
  } finally {
    await fr.close();
    await en.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("addScriptTag executes content URL and path scripts in the live realm (#387)", async () => {
  const server = createServer((req, res) => {
    res.writeHead(req.url === "/missing.js" ? 404 : 200, { "content-type": "text/javascript" });
    res.end("globalThis.externalMarker = 42");
  });
  const base = await listen(server);
  const directory = await mkdtemp(join(tmpdir(), "surf-script-"));
  const page = newPage();
  try {
    await page.setContent('<div id="target">old</div>');
    await page._ensureLive();
    assert.ok(await page.addScriptTag({ content: "globalThis.inlineMarker = 43" }));
    assert.equal(await page.evaluate(() => globalThis.inlineMarker), 43);
    assert.ok(await page.addScriptTag({ url: `${base}/script.js` }));
    assert.equal(await page.evaluate(() => globalThis.externalMarker), 42);
    await assert.rejects(
      page.addScriptTag({ type: "module", content: "globalThis.moduleMarker = 45" }),
      /module.*not supported/,
    );
    const path = join(directory, "script.js");
    await writeFile(path, 'document.querySelector("#target").textContent = "path"');
    await page.addScriptTag({ path });
    assert.equal(await page.locator("#target").textContent(), "path");
    await assert.rejects(page.addScriptTag({ url: `${base}/missing.js` }), /load|script/i);
    await assert.rejects(page.addScriptTag({}), /content|url|path/i);
  } finally {
    await page.close();
    await rm(directory, { recursive: true, force: true });
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("placeholder alt and title locators retain parent and indexed scope (#380)", async () => {
  const page = newPage();
  await page.setContent(
    '<div class="card" id="one"><input id="email-one" placeholder="Email address"><img alt="Company logo"><span title="Payroll export">one</span></div><div class="card" id="two"><input id="email-two" placeholder="Email address"><img alt="Company logo"><span title="Payroll export">two</span></div>',
  );
  for (const parent of [page.locator("#one"), page.locator(".card").last()]) {
    assert.equal(await parent.getByPlaceholder("Email").count(), 1);
    assert.equal(await parent.getByAltText("Company").count(), 1);
    assert.equal(await parent.getByTitle("Payroll").count(), 1);
  }
  await page._ensureLive();
  await page.locator(".card").last().getByPlaceholder("Email").fill("second");
  assert.equal(await page.evaluate(() => document.querySelector("#email-one").value), "");
  assert.equal(await page.evaluate(() => document.querySelector("#email-two").value), "second");
  await page.close();
});

test("role queries filter pressed state and accessibility-hidden ancestors (#379)", async () => {
  const page = newPage();
  await page.setContent(
    '<div id="scope"><button id="pressed" aria-pressed="true">Toggle</button><button id="unpressed" aria-pressed="false">Toggle</button></div><button hidden>Hidden action</button><div aria-hidden="true"><button>Aria hidden</button></div><div style="display:none"><button>CSS hidden</button></div><button style="opacity:0">Transparent action</button>',
  );
  assert.equal(await page.getByRole("button", { name: "Toggle", pressed: true }).count(), 1);
  assert.equal(await page.getByRole("button", { name: "Toggle", pressed: false }).count(), 1);
  assert.equal(await page.getByRole("button", { name: "Hidden action" }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "Aria hidden" }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "CSS hidden" }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "Transparent action" }).count(), 1);
  assert.equal(
    await page.getByRole("button", { name: "Hidden action", includeHidden: true }).count(),
    1,
  );
  await page._ensureLive();
  await page.evaluate(
    "document.querySelector('#unpressed').onclick = () => document.body.setAttribute('data-clicked','unpressed')",
  );
  await page.locator("#scope").getByRole("button", { name: "Toggle", pressed: false }).click();
  assert.equal(await page.evaluate(() => document.body.getAttribute("data-clicked")), "unpressed");
  await page.close();
});
test("innerText reads rendered text and excludes hidden descendants (#378)", async () => {
  const page = newPage();
  await page.setContent(
    '<div class="rendered" id="rendered">First<span style="display:none">SECRET</span><br>Second<div>Third</div></div><div class="rendered">Fourth<span hidden>HIDE</span></div>',
  );
  assert.equal(await page.locator("#rendered").innerText(), "First\nSecond\nThird");
  assert.deepEqual(await page.locator(".rendered").allInnerTexts(), [
    "First\nSecond\nThird",
    "Fourth",
  ]);
  assert.ok(
    (await page.evaluate(() => document.querySelector("#rendered").textContent)).includes("SECRET"),
  );
  await page._ensureLive();
  await page.evaluate(() => (document.querySelector("#rendered span").style.display = "inline"));
  assert.equal(await page.locator("#rendered").innerText(), "FirstSECRET\nSecond\nThird");
  await page.close();
});
test("tracing and enabled video options reject unsupported recording explicitly (#465)", async () => {
  const browser = await chromium.launch();
  await assert.rejects(
    () => browser.newContext({ recordVideo: { dir: "/tmp/surf-unused-video" } }),
    /recordVideo.*unavailable/,
  );
  for (const options of [
    { video: "on" },
    { video: { mode: "retain-on-failure" } },
    { trace: "on-first-retry" },
    { trace: { mode: "on" } },
  ])
    await assert.rejects(() => browser.newContext(options), /video|trace/);
  const context = await browser.newContext({ video: "off", trace: { mode: "off" } });
  try {
    for (const method of ["start", "startChunk", "stopChunk", "stop"])
      await assert.rejects(() => context.tracing[method](), /tracing.*unavailable/);
    const p = await context.newPage();
    assert.equal(p.video(), null);
  } finally {
    await context.close();
  }
});

test("fixture teardown waits in dependency order and reports asynchronous failures", async () => {
  const { runShimProbe } = await import("./runner-probes.mjs");
  const result = await runShimProbe(`
    const extended=base.extend({
      inner:async(_f,use)=>{await use(true);await new Promise(r=>setTimeout(r,10));console.log('INNER_COMPLETE');},
      outer:async({inner},use)=>{await use(inner);await new Promise(r=>setTimeout(r,20));console.log('OUTER_THROW');throw Error('intentional teardown failure');},
    });
    extended('passing body',({outer})=>assert.equal(outer,true));
  `);
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stdout, /OUTER_THROW[\s\S]*INNER_COMPLETE/);
  assert.match(result.stdout, /intentional teardown failure/);
  assert.equal((result.stdout.match(/OUTER_THROW/g) ?? []).length, 1);
  assert.equal((result.stdout.match(/INNER_COMPLETE/g) ?? []).length, 1);
});

test("failed attempts share accurate TestInfo with hooks and fixture teardown", async () => {
  const { runShimProbe } = await import("./runner-probes.mjs");
  const result = await runShimProbe(`
    const extended=base.extend({capture:async(_f,use,info)=>{
      await use(true);
      console.log('TEARDOWN_STATUS='+JSON.stringify({status:info.status,error:info.error?.message,errors:info.errors.length}));
    }});
    extended.afterEach((_f,info)=>console.log('HOOK_STATUS='+JSON.stringify({status:info?.status,current:extended.info().status,error:info?.error?.message,errors:info?.errors.length})));
    extended('intentional failure',({capture})=>{assert.equal(capture,true);throw Error('intentional body failure');});
  `);
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(
    result.stdout,
    /HOOK_STATUS={"status":"failed","current":"failed","error":"intentional body failure","errors":1}/,
  );
  assert.match(
    result.stdout,
    /TEARDOWN_STATUS={"status":"failed","error":"intentional body failure","errors":1}/,
  );
});

test("worker tuple fixtures are shared with suite hooks and torn down once", async () => {
  const { runShimProbe } = await import("./runner-probes.mjs");
  const result = await runShimProbe(`
    let workerValue;
    const extended=base.extend({
      worker:[async(_f,use)=>{console.log('WORKER_SETUP');await use({id:1});console.log('WORKER_TEARDOWN');},{scope:'worker'}],
    });
    extended.beforeAll(({worker})=>{workerValue=worker;});
    extended.afterAll(({worker})=>assert.equal(worker,workerValue));
    extended('first',({worker})=>assert.equal(worker,workerValue));
    extended('second',({worker})=>assert.equal(worker,workerValue));
  `);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal((result.stdout.match(/WORKER_SETUP/g) ?? []).length, 1);
  assert.equal((result.stdout.match(/WORKER_TEARDOWN/g) ?? []).length, 1);
});

test("file, hook, fixture predicate and TestInfo skips abort guarded bodies", async () => {
  const { runShimProbe } = await import("./runner-probes.mjs");
  const cases = [
    `test.skip(true,'file guard');test('guarded',()=>console.log('GUARDED_BODY_RAN'));`,
    `test.beforeAll(()=>test.skip(true,'all guard'));test('guarded',()=>console.log('GUARDED_BODY_RAN'));`,
    `test.beforeEach(()=>test.skip(true,'each guard'));test('guarded',()=>console.log('GUARDED_BODY_RAN'));`,
    `test('guarded',(_f,info)=>{info.skip(true,'info guard');console.log('GUARDED_BODY_RAN');});`,
    `const extended=base.extend({skipFlag:true});extended.skip(({skipFlag})=>skipFlag,'predicate guard');extended('guarded',()=>console.log('GUARDED_BODY_RAN'));`,
  ];
  for (const source of cases) {
    const result = await runShimProbe(source);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.doesNotMatch(result.stdout, /GUARDED_BODY_RAN/);
    assert.match(result.stdout, /skipped 1/);
  }
});

test("calling fixture use twice fails promptly during teardown", async () => {
  const { runShimProbe } = await import("./runner-probes.mjs");
  const result = await runShimProbe(
    `
    const extended=base.extend({value:async(_f,use)=>{await use(1);await use(2);}});
    extended('body',({value})=>assert.equal(value,1));
  `,
    { timeout: 2000 },
  );
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.equal(result.error, undefined);
  assert.match(result.stdout, /Fixture value called use more than once/);
});
test("off removes only the most recent duplicate listener (#374)", async () => {
  const page = newPage();
  const calls = [];
  const duplicate = () => calls.push("duplicate");
  const middle = () => calls.push("middle");
  page.on("load", duplicate).on("load", middle).on("load", duplicate);
  assert.equal(page.off("load", duplicate), page);
  page._emit("load");
  assert.deepEqual(calls, ["duplicate", "middle"]);
  calls.length = 0;
  page.removeListener("load", duplicate);
  page._emit("load");
  assert.deepEqual(calls, ["middle"]);
  await page.close();
});

test("once removes before invocation and supports original callback removal (#373)", async () => {
  const page = newPage();
  let calls = 0;
  page.once("load", () => {
    if (++calls === 1) page._emit("load");
  });
  page._emit("load");
  page._emit("load");
  assert.equal(calls, 1);
  const removed = () => {
    throw new Error("removed once handler ran");
  };
  page.once("load", removed);
  page.off("load", removed);
  page._emit("load");
  const order = [];
  const duplicate = () => order.push("duplicate");
  page.on("custom", duplicate).once("custom", duplicate);
  page.removeListener("custom", duplicate);
  page._emit("custom");
  page._emit("custom");
  assert.deepEqual(order, ["duplicate", "duplicate"]);
  await page.close();
});
test("Context headers reach authored and subsequent native page fetches (#392)", async () => {
  const requests = [];
  const server = createServer((req, res) => {
    requests.push({ url: req.url, headers: req.headers });
    res.setHeader("content-type", "text/html");
    res.end(req.url === "/" ? '<script>fetch("/authored")</script>' : "OK");
  });
  const base = await listen(server);
  const ctx = new BrowserContext({ extraHTTPHeaders: { "x-probe": "yes" } });
  try {
    const p = await ctx.newPage();
    await p.goto(base, { waitUntil: "networkidle" });
    await p.evaluate(() => fetch("/fetch").then((response) => response.text()));
    assert.equal(requests.find((request) => request.url === "/").headers["x-probe"], "yes");
    assert.equal(requests.find((request) => request.url === "/authored").headers["x-probe"], "yes");
    assert.equal(requests.find((request) => request.url === "/fetch").headers["x-probe"], "yes");
    await ctx.setExtraHTTPHeaders({ "x-next": "new" });
    await p.evaluate(() => fetch("/next").then((response) => response.text()));
    const next = requests.find((request) => request.url === "/next").headers;
    assert.equal(next["x-next"], "new");
    assert.equal(next["x-probe"], undefined);
    await p.setExtraHTTPHeaders({});
    await p.evaluate(() => fetch("/cleared").then((response) => response.text()));
    assert.equal(
      requests.find((request) => request.url === "/cleared").headers["x-next"],
      undefined,
    );
  } finally {
    await ctx.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("viewport resize updates live layout media and only its own page (#369)", async () => {
  const server = createServer((_req, res) =>
    res.end(
      '<style>#target{width:220px;height:20px}@media(max-width:600px){#target{width:80px}}</style><div id="target"></div><script>window.initialSize=[innerWidth,innerHeight];window.resizeCount=0;addEventListener("resize",()=>resizeCount++);</script>',
    ),
  );
  const base = await listen(server);
  const context = new BrowserContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const sibling = await context.newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await sibling.setViewportSize({ width: 375, height: 600 });
    await sibling.goto(base, { waitUntil: "networkidle" });
    assert.deepEqual(await sibling.evaluate(() => initialSize), [375, 600]);
    await sibling.setViewportSize({ width: 1280, height: 800 });
    await page.setViewportSize({ width: 375, height: 812 });
    assert.deepEqual(page.viewportSize(), { width: 375, height: 812 });
    assert.deepEqual(sibling.viewportSize(), { width: 1280, height: 800 });
    assert.deepEqual(
      await page.evaluate(() => [
        innerWidth,
        innerHeight,
        matchMedia("(max-width:600px)").matches,
        document.getElementById("target").getBoundingClientRect().width,
        resizeCount,
      ]),
      [375, 812, true, 80, 1],
    );
    await page.setViewportSize({ width: 375, height: 812 });
    assert.equal(await page.evaluate(() => resizeCount), 1);
    await page.setViewportSize({ width: 1280, height: 800 });
    assert.deepEqual(
      await page.evaluate(() => [
        innerWidth,
        matchMedia("(max-width:600px)").matches,
        document.getElementById("target").getBoundingClientRect().width,
        resizeCount,
      ]),
      [1280, false, 220, 2],
    );
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("soft assertions keep running but fail their owning test (#368)", async () => {
  const { spawnSync } = await import("node:child_process");
  const { mkdtemp, writeFile } = await import("node:fs/promises");
  const root = await mkdtemp(join(tmpdir(), "surf-soft-"));
  const file = join(root, "soft.mjs");
  await writeFile(
    file,
    `import {test,expect} from ${JSON.stringify(new URL("./index.mjs", import.meta.url).href)};
    test('soft failure continues',async(_args,info)=>{
      expect.soft(1).toBe(2);
      console.log('AFTER_SYNC_SOFT');
      await expect.soft(Promise.resolve(3)).resolves.toBe(4);
      console.log('AFTER_ASYNC_SOFT',info.errors.length);
      expect(5).toBe(5);
    });
    test('following test is clean',()=>{console.log('NEXT_TEST');expect(1).toBe(1);});`,
  );
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  const result = spawnSync(process.execPath, ["--test", "--test-force-exit", file], {
    env,
    encoding: "utf8",
    timeout: 10000,
  });
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stdout, /AFTER_SYNC_SOFT/);
  assert.match(result.stdout, /AFTER_ASYNC_SOFT 2/);
  assert.match(result.stdout, /NEXT_TEST/);
  assert.match(result.stdout, /soft assertion/i);
  await rm(root, { recursive: true, force: true });
});

test("toPass retries async callbacks and retains the final assertion error (#367)", async () => {
  let calls = 0;
  await expect(async () => {
    calls++;
    expect(calls).toBeGreaterThanOrEqual(3);
  }).toPass({ timeout: 100, intervals: [1] });
  assert.equal(calls, 3);
  const failure = new Error("last callback assertion");
  let failures = 0;
  await assert.rejects(
    () =>
      expect(async () => {
        failures++;
        throw failure;
      }).toPass({ timeout: 10, intervals: [1] }),
    (error) => error === failure,
  );
  assert.ok(failures > 1);
  await assert.rejects(
    () => expect(() => new Promise(() => {})).toPass({ timeout: 5, intervals: [1] }),
    /timed out/i,
  );
});

test("expect.poll retries values awaits async callbacks and preserves not (#366)", async () => {
  let calls = 0;
  await expect.poll(() => ++calls, { timeout: 100, intervals: [1] }).toBe(3);
  assert.equal(calls, 3);
  let asynchronousCalls = 0;
  await expect
    .poll(
      async () => {
        asynchronousCalls++;
        return 7;
      },
      { timeout: 100, intervals: [1] },
    )
    .toBe(7);
  assert.equal(asynchronousCalls, 1);
  let negativeCalls = 0;
  await expect.poll(() => ++negativeCalls, { timeout: 100, intervals: [1] }).not.toBeLessThan(3);
  assert.equal(negativeCalls, 3);
  await assert.rejects(
    () => expect.poll(() => 0, { timeout: 5, intervals: [1] }).toBe(1),
    /expected 0 to be 1/,
  );
  await assert.rejects(
    () => expect.poll(() => new Promise(() => {}), { timeout: 5, intervals: [1] }).toBe(1),
    /timed out/i,
  );
});

test("XPath engine prefixes and scoped parent locators retain their context (#365)", async () => {
  const page = newPage();
  try {
    await page.setContent(
      '<section><div data-testid="marker"></div><button data-testid="target">inside</button></section><button data-testid="target">outside</button><div class="MuiPickersInputBase-root"><div data-testid="field"><div class="MuiPickersSectionList-root"><span>01</span></div></div></div>',
    );
    assert.equal(await page.locator("xpath=//button").count(), 2);
    assert.equal(await page.locator("//button").count(), 2);
    const target = page.getByTestId("marker").locator("xpath=..").getByTestId("target");
    assert.equal(await target.count(), 1);
    assert.equal(await target.textContent(), "inside");
    const sections = page
      .getByTestId("field")
      .locator(
        'xpath=descendant-or-self::div[contains(@class,"MuiPickersSectionList-root")] | ancestor::*[contains(@class,"MuiPickersInputBase-root")][1]//div[contains(@class,"MuiPickersSectionList-root")]',
      );
    assert.equal(await sections.count(), 1);
    assert.equal(await sections.locator("xpath=.").count(), 1);
    assert.equal(await page.locator("css=section").locator("xpath=.//button").count(), 1);
  } finally {
    await page.close();
  }
});

test("Network waits reject unmet predicates instead of inventing events (#308)", async () => {
  const page = newPage();
  try {
    for (const method of ["waitForResponse", "waitForRequest"]) {
      await assert.rejects(
        page[method](() => false, { timeout: 10 }),
        { name: "TimeoutError" },
      );
      assert.equal(
        page._listeners.get(method === "waitForResponse" ? "response" : "request")?.length ?? 0,
        0,
      );
    }
  } finally {
    await page.close();
  }
});

test("scoped XPath parent actions dispatch only to the selected live target (#365)", async () => {
  const server = createServer((_req, res) =>
    res.end(
      '<section><div data-testid="marker"></div><button data-testid="target">inside</button></section><button data-testid="target">outside</button><p id="result">none</p><script>document.querySelectorAll("button").forEach(button=>button.addEventListener("click",()=>document.querySelector("#result").textContent=button.textContent))</script>',
    ),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.getByTestId("marker").locator("xpath=..").getByTestId("target").click();
    assert.equal(await page.locator("#result").textContent(), "inside");
    await page.locator("xpath=//button").last().click();
    assert.equal(await page.locator("#result").textContent(), "outside");
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("XPath parent chains execute in an iframe realm (#365)", async () => {
  const server = createServer((_req, res) =>
    res.end(
      `<iframe srcdoc="<section><span data-testid='marker'></span><button data-testid='target' onclick=&quot;this.textContent='changed'&quot;>inside</button></section><button data-testid='target'>outside</button>"></iframe>`,
    ),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    const target = page
      .frameLocator("iframe")
      .getByTestId("marker")
      .locator("xpath=..")
      .getByTestId("target");
    assert.equal(await target.count(), 1);
    await target.click();
    assert.equal(await target.textContent(), "changed");
  } finally {
    await page.close();
    await new Promise((done) => server.close(done));
  }
});

test("Network waits exclude events already delivered before subscribing (#355)", async () => {
  const page = newPage();
  const response = {
    url: () => "http://localhost/api",
    request: () => ({ url: () => "http://localhost/api" }),
  };
  response._seq = page._actionSeq;
  page._recentResponses = [response];
  try {
    for (const method of ["waitForResponse", "waitForRequest"]) {
      await assert.rejects(page[method](/\/api$/, { timeout: 10 }), { name: "TimeoutError" });
      const pending = page[method](/\/api$/, { timeout: 100 });
      await new Promise((resolve) => setImmediate(resolve));
      const item = method === "waitForResponse" ? response : response.request();
      page._emit(method === "waitForResponse" ? "response" : "request", item);
      assert.equal(await pending, item);
    }
  } finally {
    await page.close();
  }
});

test("Network waits support URL globs and await predicate results (#354)", async () => {
  const page = newPage();
  const item = { url: () => "http://localhost/api" };
  try {
    for (const [method, event] of [
      ["waitForResponse", "response"],
      ["waitForRequest", "request"],
    ]) {
      const glob = page[method]("**/api", { timeout: 100 });
      page._emit(event, item);
      assert.equal(await glob, item);
      const rejected = assert.rejects(
        page[method](
          async () => {
            await Promise.resolve();
            return false;
          },
          { timeout: 10 },
        ),
        { name: "TimeoutError" },
      );
      page._emit(event, item);
      await rejected;
      const accepted = page[method](
        async (candidate) => {
          await Promise.resolve();
          return candidate === item;
        },
        { timeout: 100 },
      );
      page._emit(event, item);
      assert.equal(await accepted, item);
      const exact = assert.rejects(page[method]("http://localhost/ap", { timeout: 10 }), {
        name: "TimeoutError",
      });
      page._emit(event, item);
      await exact;
    }
  } finally {
    await page.close();
  }
});

test("Live locator inputValue and value assertions read dirty DOM properties (#350)", async () => {
  const server = createServer((req, res) =>
    res.end("<input id=text value=old><textarea id=area>before</textarea>"),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.locator("#text").fill("new");
    assert.equal(await page.locator("#text").inputValue(), "new");
    assert.equal(await page.locator("#text").getAttribute("value"), "old");
    await expect(page.locator("#text")).toHaveValue("new", { timeout: 0 });
    await page.evaluate(() => {
      document.querySelector("#area").value = "after";
    });
    assert.equal(await page.locator("#area").inputValue(), "after");
    await expect(page.locator("#area")).toHaveValue("after", { timeout: 0 });
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Live checkbox actions execute one default and retain requested state (#351)", async () => {
  const server = createServer((req, res) =>
    res.end(`<input id=off type=checkbox><input id=on type=checkbox checked>
    <input id=canceled type=checkbox onclick="event.preventDefault()">
    <script>globalThis.changes=[]; for(const el of document.querySelectorAll('input')) el.addEventListener('change',()=>changes.push(el.id));</script>`),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.locator("#off").check();
    assert.equal(await page.evaluate(() => document.querySelector("#off").checked), true);
    assert.equal(await page.locator("#off").isChecked(), true);
    await expect(page.locator("#off")).toBeChecked({ timeout: 0 });
    await page.locator("#off").check();
    await page.locator("#on").uncheck();
    assert.equal(await page.locator("#on").isChecked(), false);
    await page.locator("#on").setChecked(true);
    await page.locator("#on").setChecked(false);
    assert.deepEqual(await page.evaluate(() => changes), ["off", "on", "on", "on"]);
    await assert.rejects(page.locator("#canceled").check(), /checked state|did not change/);
    assert.equal(await page.evaluate(() => document.querySelector("#canceled").checked), false);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Locator visibility follows rendered boxes while ignoring opacity (#353)", async () => {
  const server = createServer((req, res) =>
    res.end(`<!doctype html><style>
    .hidden {visibility:hidden} .override {visibility:visible}
    </style><div hidden><button id=ancestor>hidden</button></div>
    <button id=opacity style="opacity:0">transparent</button><span id=empty></span><div id=emptydiv></div>
    <div class=hidden><button class=override id=override>shown</button></div>
    <div style="display:contents" id=contents><button>child</button></div>
    <button id=offscreen style="position:absolute;left:-10000px">offscreen</button>`),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    for (const [id, visible] of [
      ["ancestor", false],
      ["opacity", true],
      ["empty", false],
      ["emptydiv", false],
      ["override", true],
      ["contents", true],
      ["offscreen", true],
    ]) {
      assert.equal(await page.locator(`#${id}`).isVisible(), visible, id);
      if (visible) await expect(page.locator(`#${id}`)).not.toBeHidden({ timeout: 0 });
      else await expect(page.locator(`#${id}`)).toBeHidden({ timeout: 0 });
    }
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Live locator CSS reads preserve rules inserted through CSSOM (#352)", async () => {
  const server = createServer((req, res) =>
    res.end("<style id=sheet></style><button id=target class=hide>target</button>"),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.evaluate(() =>
      document
        .querySelector("#sheet")
        .sheet.insertRule(".hide { display:none; color:rgb(1, 2, 3) }", 0),
    );
    assert.equal(await page.locator("#target").cssValue("display"), "none");
    await expect(page.locator("#target")).toHaveCSS("color", "rgb(1, 2, 3)", { timeout: 0 });
    assert.equal(await page.locator("#target").isVisible(), false);
    await expect(page.locator("#target")).toBeHidden({ timeout: 0 });
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Locator submit clicks perform one native form activation (#348)", async () => {
  let requests = 0;
  const server = createServer((req, res) => {
    requests++;
    return res.end(
      '<form><button id=submit>Submit</button><button id=canceled onclick="event.preventDefault()">Canceled</button></form><script>globalThis.submits=0;document.querySelector("form").addEventListener("submit",event=>{event.preventDefault();submits++})</script>',
    );
  });
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    await page.locator("#submit").click();
    assert.equal(await page.evaluate(() => globalThis.submits), 1);
    assert.equal(requests, 1, "prevented submit must not make a fallback network request");
    await page.locator("#canceled").click();
    assert.equal(await page.evaluate(() => globalThis.submits), 1);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Default locator actions wait for enabled editable visible controls (#346)", async () => {
  const server = createServer((req, res) =>
    res.end(
      '<button id=disabled disabled>disabled</button><button id=hidden hidden>hidden</button><input id=disabledinput disabled value=old><input id=readonly readonly value=old><input id=hiddeninput hidden value=old><script>globalThis.events=[];for(const element of document.querySelectorAll("input,button")) for(const type of ["click","input"]) element.addEventListener(type,()=>events.push(element.id+":"+type))</script>',
    ),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    for (const [id, method] of [
      ["disabled", "click"],
      ["hidden", "click"],
      ["disabledinput", "fill"],
      ["readonly", "fill"],
      ["hiddeninput", "fill"],
    ]) {
      const action =
        method === "fill"
          ? page.locator(`#${id}`).fill("new", { timeout: 30 })
          : page.locator(`#${id}`).click({ timeout: 30 });
      await assert.rejects(action, { name: "TimeoutError" }, id);
    }
    assert.deepEqual(await page.evaluate(() => events), []);
    assert.deepEqual(
      await page.evaluate(() => Array.from(document.querySelectorAll("input"), (el) => el.value)),
      ["old", "old", "old"],
    );
    await page.evaluate(() =>
      setTimeout(() => {
        document.querySelector("#disabled").disabled = false;
      }, 10),
    );
    await page.locator("#disabled").click({ timeout: 1000 });
    assert.deepEqual(await page.evaluate(() => events), ["disabled:click"]);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Completed emitted requests link back to their exact response (#344)", async () => {
  const server = createServer((req, res) => {
    if (req.url === "/api") {
      res.setHeader("content-type", "application/json");
      res.end('{"state":"owed"}');
    } else res.end('<script>fetch("/api")</script>');
  });
  const base = await listen(server);
  const page = newPage();
  const completed = [];
  page.on("requestfinished", (request) => {
    if (request.url().endsWith("/api")) completed.push(request);
  });
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    assert.equal(completed.length, 1);
    const response = await completed[0].response();
    assert.equal(response.request(), completed[0]);
    assert.equal(response.status(), 200);
    assert.deepEqual(await response.json(), { state: "owed" });
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("Opacity-only sibling animations do not reject unrelated geometry (#345)", async () => {
  const server = createServer((req, res) =>
    res.end(
      "<style>@keyframes fade{from{opacity:0}to{opacity:1}}#sibling{animation:fade 100s both}#target{width:120px;height:20px}</style><div id=sibling>sibling</div><div id=target>target</div>",
    ),
  );
  const base = await listen(server);
  const page = newPage();
  try {
    await page.goto(base, { waitUntil: "networkidle" });
    assert.equal(
      await page.evaluate(() => document.querySelector("#target").getBoundingClientRect().width),
      120,
    );
    await page.evaluate(() => {
      document.querySelector("#sibling").style.animation = "fade 1ms both";
    });
    await page.waitForTimeout(25);
    assert.equal(
      await page.evaluate(() => document.querySelector("#target").getBoundingClientRect().width),
      120,
    );
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("role locators resolve native headings table structure and image names (#377)", async () => {
  const page = newPage();
  await page.setContent(
    '<h1>Overview</h1><h2 id="second">Overview</h2><table><thead><tr><th>Employee</th></tr></thead><tbody><tr><th scope="row">Alice</th><td>Engineer</td></tr></tbody></table><img alt="Company logo"><img alt="Company logo">',
  );
  assert.equal(await page.getByRole("heading", { name: "Overview", level: 2 }).count(), 1);
  assert.equal(await page.getByRole("heading").count(), 2);
  assert.equal(await page.getByRole("columnheader", { name: "Employee" }).count(), 1);
  assert.equal(await page.getByRole("row").count(), 2);
  assert.equal(await page.getByRole("rowheader", { name: "Alice" }).count(), 1);
  assert.equal(await page.getByRole("cell", { name: "Engineer" }).count(), 1);
  assert.equal(await page.getByRole("img", { name: "Company logo" }).count(), 2);
  await page._ensureLive();
  await page.evaluate(
    "document.querySelector('#second').onclick = () => document.body.setAttribute('data-heading','two')",
  );
  await page.getByRole("heading", { name: "Overview", level: 2 }).click();
  assert.equal(await page.evaluate(() => document.body.getAttribute("data-heading")), "two");
  await page.close();
});
