// Offline test of the napi-backed Playwright shim (node --test). Uses
// setContent for the read/locator/expect surface; a localhost server exercises
// goto. Skips cleanly if the native addon isn't built.

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
const { chromium, expect, newPage, test: pwTest } = shim;

const PAGE = `<html><head><title>Shop</title></head><body>
  <main><h1>Widget</h1><p class="d">A nice widget</p></main>
  <button>Add to cart</button><button>Wishlist</button>
  <label for="q">Search</label><input id="q">
  <a href="/p">product</a>
</body></html>`;

test("locator count + text", async () => {
  const page = newPage();
  await page.setContent(PAGE);
  assert.equal(await page.locator("button").count(), 2);
  assert.equal(await page.locator(".d").textContent(), "A nice widget");
  assert.equal(await page.locator("button").first().textContent(), "Add to cart");
  assert.equal(await page.locator("button").nth(1).textContent(), "Wishlist");
});

test("getByRole / getByText / getByLabel", async () => {
  const page = newPage();
  await page.setContent(PAGE);
  assert.equal(await page.getByRole("button").count(), 2);
  assert.equal(await page.getByRole("button", { name: "Wishlist" }).count(), 1);
  assert.equal(await page.getByText("nice widget").count(), 1);
  assert.equal(await page.getByLabel("Search").count(), 1); // the labelled <input>
});

test("expect matchers + .not", async () => {
  const page = newPage();
  await page.setContent(PAGE);
  await expect(page.locator("button")).toHaveCount(2);
  await expect(page.locator("h1")).toHaveText("Widget");
  await expect(page.locator(".d")).toContainText("widget");
  await expect(page.locator("button")).not.toHaveCount(5);
  await assert.rejects(() => expect(page.locator("h1")).toHaveText("Nope"));
});

test("page views", async () => {
  const page = newPage();
  await page.setContent(PAGE);
  assert.equal(await page.title(), "Shop");
  assert.match(page.markdown(), /# Widget/);
  assert.ok((await page.innerText()).includes("Widget"));
});

test("evaluate + render (JS execution, no browser)", async () => {
  const page = newPage();
  await page.setContent("<body><h1 id='t'>Hi</h1><div id='app'></div></body>");
  // evaluate: read the DOM via page JS
  assert.equal(await page.evaluate("document.querySelector('#t').textContent"), "Hi");
  // render: the page's own script hydrates the DOM (incl. a virtual timer)
  await page.render(
    "document.getElementById('app').innerHTML='<p>hi</p>'; setTimeout(()=>{document.querySelector('p').textContent='late'},5)",
  );
  assert.match(await page.content(), /<p>late<\/p>/);
  // the locator surface now sees the hydrated DOM
  assert.equal(await page.locator("p").textContent(), "late");
});

test("actions: fill / check / selectOption mutate the DOM", async () => {
  const page = newPage();
  await page.setContent(
    "<input id='t'><input id='c' type='checkbox'><select id='s'><option value='a'>A</option><option value='b'>B</option></select>",
  );
  await page.fill("#t", "hello");
  await page.check("#c");
  await page.selectOption("#s", "b");
  assert.match(await page.content(), /value="hello"/);
  assert.match(await page.content(), /id="c"[^>]*checked|checked[^>]*id="c"/);
  // the select now has b selected (serialized)
  assert.match(await page.content(), /value="b"[^>]*selected|selected[^>]*value="b"/);
});

test("click follows a link to a new page", async () => {
  const linked = "<html><head><title>Dest</title></head><body><p>arrived</p></body></html>";
  const server = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(linked);
  });
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const page = newPage();
  // base URL is the server so the relative href resolves there
  await page.goto(`http://127.0.0.1:${port}/`);
  await page.setContent(`<a href="http://127.0.0.1:${port}/next">go</a>`);
  await page.click("a");
  assert.equal(await page.title(), "Dest");
  server.close();
});

test("click submits a GET form", async () => {
  let seen = "";
  const server = createServer((req, res) => {
    seen = req.url;
    res.writeHead(200, { "content-type": "text/html" });
    res.end("<html><head><title>Results</title></head><body>ok</body></html>");
  });
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const page = newPage();
  await page.goto(`http://127.0.0.1:${port}/`);
  await page.setContent(
    `<form action="http://127.0.0.1:${port}/search" method="get"><input name="q" value="rust"><button>Go</button></form>`,
  );
  await page.click("button");
  assert.equal(await page.title(), "Results");
  assert.match(seen, /\/search\?q=rust/);
  server.close();
});

test("locator accessors (G1)", async () => {
  const page = newPage();
  await page.setContent(
    "<a id='l' href='/p' title='go'>link</a><input id='i' value='hi' disabled><div id='d' style='display:none'>x</div>",
  );
  assert.equal(await page.locator("#l").getAttribute("href"), "/p");
  assert.equal(await page.locator("#l").ariaRole(), "link");
  assert.equal(await page.locator("#i").inputValue(), "hi");
  assert.equal(await page.locator("#i").isEnabled(), false);
  assert.equal(await page.locator("#d").isVisible(), false);
  assert.equal(await page.locator("#l").isVisible(), true);
});

test("expect matcher surface (G2)", async () => {
  const page = newPage();
  await page.setContent(
    "<button class='primary'>Go</button><input value='v'><div hidden>h</div><nav><a href='/'>Home</a></nav>",
  );
  await expect(page.locator("button")).toBeVisible();
  await expect(page.locator("div")).toBeHidden();
  await expect(page.locator("button")).toHaveClass("primary");
  await expect(page.locator("button")).toHaveAttribute("class", "primary");
  await expect(page.locator("input")).toHaveValue("v");
  await expect(page.locator("h1")).not.toBeVisible();
  await expect(page.locator("nav")).toMatchAriaSnapshot('- navigation\n- link "Home"');
});

test("locator-scoped actions (G3)", async () => {
  const page = newPage();
  await page.setContent("<input id='t'><input id='c' type='checkbox'>");
  await page.locator("#t").fill("typed");
  await page.locator("#c").check();
  assert.equal(await page.locator("#t").inputValue(), "typed");
  assert.equal(await page.locator("#c").isChecked(), true);
  await page.locator("#c").uncheck();
  assert.equal(await page.locator("#c").isChecked(), false);
});

test("page.screenshot renders a synthetic PNG (no browser)", async () => {
  const page = newPage();
  await page.setContent("<button>x</button>");
  const png = await page.screenshot();
  assert.ok(Buffer.isBuffer(png) && png.length > 0);
  // PNG magic.
  assert.deepEqual([...png.subarray(0, 4)], [0x89, 0x50, 0x4e, 0x47]);
  const svg = await page.screenshot({ type: "svg" });
  assert.match(svg.toString("utf8"), /^<svg/);
});

test("honest-throws for pixel APIs (G5)", async () => {
  const page = newPage();
  await page.setContent("<button>x</button>");
  await assert.rejects(() => page.pdf(), /unavailable/);
  await assert.rejects(() => page.locator("button").screenshot(), /unavailable/);
  // boundingBox returns null (no layout engine → unmeasurable), matching Playwright's
  // null for a non-laid-out element — not a throw.
  assert.equal(await page.locator("button").boundingBox(), null);
});

test("mock SPA hydrates through the shim (G15)", async () => {
  const page = newPage();
  await page.setContent("<html><body><div id='root'></div></body></html>");
  await page.render(`
    const root = document.getElementById('root');
    let state = { n: 0 };
    function render() {
      root.innerHTML = '';
      const h = document.createElement('h1');
      h.setAttribute('class', 'title');
      h.textContent = 'N' + state.n;
      root.appendChild(h);
    }
    render();
    setTimeout(() => { state.n = 7; render(); }, 5);
  `);
  // the hydrated DOM is visible to the locator surface
  assert.equal(await page.locator(".title").textContent(), "N7");
  await expect(page.locator("h1.title")).toHaveText("N7");
});

test("cookies persist across navigations (G10)", async () => {
  // /login sets a session cookie; /me echoes the Cookie header it received.
  const server = createServer((req, res) => {
    if (req.url === "/login") {
      res.writeHead(200, { "set-cookie": "sid=secret; Path=/", "content-type": "text/html" });
      res.end("<title>in</title>");
    } else {
      const got = req.headers.cookie ?? "";
      res.writeHead(200, { "content-type": "text/html" });
      res.end(`<body><p>cookie:${got}</p></body>`);
    }
  });
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const page = newPage();
  await page.goto(`http://127.0.0.1:${port}/login`); // receives Set-Cookie
  await page.goto(`http://127.0.0.1:${port}/me`); // must send it back
  assert.match(await page.innerText(), /cookie:sid=secret/);
  assert.equal(page.storageState().cookies.length, 1);
  server.close();
});

test("getByTestId resolves the configured attribute", async () => {
  const page = newPage();
  await page.setContent("<button data-testid='submit'>Go</button><span data-testid='x'>1</span>");
  assert.equal(await page.getByTestId("submit").count(), 1);
  assert.equal(await page.getByTestId("submit").textContent(), "Go");
  await expect(page.getByTestId("x")).toHaveText("1");
});

test("locator composition (last/filter/and/or) is pure JS", async () => {
  const page = newPage();
  await page.setContent("<ul><li>apple</li><li>banana</li><li>cherry</li></ul><li>loose</li>");
  assert.equal(await page.locator("ul li").count(), 3);
  assert.equal(await page.locator("ul li").last().textContent(), "cherry");
  assert.equal(await page.locator("li").filter({ hasText: "ban" }).count(), 1);
  assert.equal(await page.locator("li").filter({ hasNotText: "loose" }).count(), 3);
});

test("nested locator scopes via CSS concat", async () => {
  const page = newPage();
  await page.setContent("<div class='card'><button>buy</button></div><button>other</button>");
  assert.equal(await page.locator(".card").locator("button").count(), 1);
  assert.equal(await page.locator(".card").locator("button").textContent(), "buy");
});

test("function-form page.evaluate with an argument", async () => {
  const page = newPage();
  await page.setContent("<div id='n'>40</div>");
  const r = await page.evaluate((sel) => Number(document.querySelector(sel).textContent) + 2, "#n");
  assert.equal(Number(r), 42);
});

test("node_snapshot batches expect(locator) chain in one crossing", async () => {
  const page = newPage();
  await page.setContent("<button>Go</button><input disabled>");
  const snap = page.locator("button")._snapshot();
  assert.equal(snap.visible, true);
  assert.equal(snap.enabled, true);
  assert.equal(snap.text, "Go");
  await expect(page.locator("button")).toBeVisible();
  await expect(page.locator("button")).toBeEnabled();
  await expect(page.locator("input")).toBeDisabled();
});

test("generic-value expect matchers (no DOM, pure JS)", async () => {
  expect(2 + 2).toBe(4);
  expect([1, 2, 3]).toContain(2);
  expect([1, 2, 3]).toHaveLength(3);
  expect("hello world").toMatch(/world/);
  expect(null).toBeNull();
  expect(0).toBeFalsy();
  expect(5).toBeGreaterThan(3);
  expect({ a: 1, b: 2 }).toMatchObject({ a: 1 });
  expect({ a: 1 }).toEqual({ a: 1 });
  expect(() => {
    throw new Error("boom");
  }).toThrow("boom");
  expect(3).not.toBe(4);
});

test("page expect: toHaveURL / toHaveTitle", async () => {
  const body = "<html><head><title>Home</title></head><body>hi</body></html>";
  const server = createServer((_q, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const page = newPage();
  await page.goto(`http://127.0.0.1:${port}/`);
  await expect(page).toHaveURL(new RegExp(`127.0.0.1:${port}`));
  await expect(page).toHaveTitle("Home");
  server.close();
});

test("waiters resolve on the static DOM", async () => {
  const page = newPage();
  await page.setContent("<main><h1>x</h1></main>");
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(1);
  assert.ok(await page.waitForSelector("h1"));
  assert.equal(await page.waitForSelector("nope", { state: "hidden" }), null);
});

test("baseURL + history (goBack)", async () => {
  const server = createServer((req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(`<title>${req.url}</title>`);
  });
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const { newContext } = {
    newContext: () => new shim.BrowserContext({ baseURL: `http://127.0.0.1:${port}` }),
  };
  const ctx = newContext();
  const page = await ctx.newPage();
  await page.goto("/a"); // relative → resolved against baseURL
  await page.goto("/b");
  assert.equal(await page.title(), "/b");
  await page.goBack();
  assert.equal(await page.title(), "/a");
  server.close();
});

test("event registry: on('load') fires", async () => {
  const page = newPage();
  let fired = 0;
  page.on("load", () => fired++);
  await page.setContent("<p>x</p>");
  assert.equal(fired, 1);
});

test("honest throws for network/input/pixel", async () => {
  const page = newPage();
  await page.setContent("<a>x</a>");
  // page.route(fulfill) is now supported (#220); routeFromHAR still throws.
  await assert.rejects(() => page.routeFromHAR("x.har"), /interception/);
  // No synthetic pointer device → raw mouse coords stay an honest throw. (hover() and
  // keyboard.press are real features now — key-event dispatch via the live session —
  // covered in surface.test.mjs, so they're NOT asserted to throw here.)
  await assert.rejects(() => page.mouse.click(0, 0), /input/);
});

pwTest("@playwright/test fixture: { page } is injected", async ({ page }) => {
  await page.setContent("<h1>fixture</h1>");
  await expect(page.locator("h1")).toHaveText("fixture");
});

pwTest.describe("describe groups", () => {
  pwTest("nested test runs", async ({ page }) => {
    assert.ok(page);
  });
});

test("goto over localhost via chromium.launch", async () => {
  const body = "<html><head><title>Live</title></head><body><p>hello</p></body></html>";
  const server = createServer((_req, res) => {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const browser = await chromium.launch();
  const page = await browser.newPage();
  const resp = await page.goto(`http://127.0.0.1:${port}/`);
  assert.equal(resp.status(), 200);
  assert.equal(await page.title(), "Live");
  assert.ok((await page.innerText()).includes("hello"));
  await browser.close();
  server.close();
});
