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

test("clearCookies intersects name/domain/path filters and preserves unrelated identities", async () => {
  const context = new shim.BrowserContext();
  try {
    await context.addCookies([
      { name: "drop", value: "1", domain: "one.test", path: "/app" },
      { name: "drop", value: "2", domain: "two.test", path: "/app" },
      { name: "drop", value: "3", domain: "one.test", path: "/other" },
      { name: "keep", value: "4", domain: "one.test", path: "/app" },
    ]);
    await context.clearCookies({ name: /^drop$/g, domain: "one.test", path: /^\/app$/g });
    assert.deepEqual(
      (await context.cookies()).map((cookie) => cookie.value),
      ["2", "3", "4"],
    );
    await context.clearCookies({ name: "drop" });
    assert.deepEqual(
      (await context.cookies()).map((cookie) => cookie.name),
      ["keep"],
    );
    await context.clearCookies();
    assert.deepEqual(await context.cookies(), []);
  } finally {
    await context.close();
  }
});

test("URL-form cookies reach HTTP requests and the live document", async () => {
  let sent;
  const server = createServer((request, response) => {
    sent = request.headers.cookie ?? "";
    response.end("<title>Cookie fixture</title>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const context = new shim.BrowserContext();
  try {
    await context.addCookies([{ name: "locale", value: "en-US", url: origin + "/account/page" }]);
    const page = await context.newPage();
    await page.goto(origin + "/account/home", { waitUntil: "networkidle" });
    assert.equal(sent, "locale=en-US");
    assert.equal(await page.evaluate(() => document.cookie), "locale=en-US");
    assert.equal((await context.cookies(origin + "/outside")).length, 0);
    const cookie = (await context.cookies())[0];
    assert.equal(cookie.path, "/account/");
    assert.equal(cookie.host_only, true);
    assert.equal(cookie.http_only, false);
  } finally {
    await context.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
