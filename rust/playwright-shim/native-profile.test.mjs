import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";

let native, BrowserContext, newPage;
try {
  ({ BrowserContext, newPage } = await import("./index.mjs"));
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
const detection = new URL("../../harness/detection/", import.meta.url);
const probeSource = readFileSync(new URL("native-profile-probe.js", detection), "utf8");
const probe = Function(`return (${probeSource.trim().replace(/;$/, "")})`)();
const control = JSON.parse(
  readFileSync(new URL("fixtures/native-profile-chrome155.json", detection)),
);

test("the addon reports its linked renderer engine without exposing diagnostics to guests", async () => {
  assert.equal(typeof native.engineVersion, "function");
  const actual = native.engineVersion();
  assert.match(actual, /^\d+\.\d+\.\d+\.\d+(?:-[\w.-]+)?$/);
  const expected = process.env.TURBO_SURF_EXPECTED_V8_VERSION;
  if (expected !== undefined) assert.equal(actual, expected);
  const page = newPage();
  try {
    await page.setContent("<!doctype html><body>Engine diagnostic isolation</body>");
    assert.equal(await page.evaluate("typeof globalThis.engineVersion"), "undefined");
    assert.equal(native.engineVersion(), actual);
  } finally {
    await page.close();
  }
});

async function fixture(run) {
  const counts = new Map();
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://fixture").pathname;
    counts.set(path, (counts.get(path) ?? 0) + 1);
    response.setHeader("Connection", "close");
    response.setHeader("Content-Type", "text/html");
    if (path === "/cached") {
      response.setHeader("Cache-Control", "max-age=600");
      return response.end("cached");
    }
    if (path === "/clear" || path === "/clear-image")
      response.setHeader("Clear-Site-Data", '"storage"');
    response.end("<!doctype html><body>Profile control</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    await run(`http://127.0.0.1:${server.address().port}`, counts);
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("native tabs and navigation match the source-pinned Chrome profile control", async () => {
  assert.equal(control.browser, "Chrome/155.0.8059.79");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  for (const [key, path] of [
    ["probeSha256", "native-profile-probe.js"],
    ["collectorSha256", "native-profile-chrome.mjs"],
  ])
    assert.equal(
      control[key],
      createHash("sha256")
        .update(readFileSync(new URL(path, detection)))
        .digest("hex"),
    );
  await fixture(async (base, counts) => {
    const context = new BrowserContext();
    try {
      const a = await context.newPage(),
        b = await context.newPage();
      await a.goto(base, { waitUntil: "networkidle" });
      await b.goto(base, { waitUntil: "networkidle" });
      const run = async (page, operation) =>
        JSON.parse(
          await page.evaluate(
            `(async()=>JSON.stringify(await (${probe})(${JSON.stringify(operation)})))()`,
          ),
        );
      const actual = {};
      actual.first = await run(a, { local: "first", session: "alpha", cache: true });
      actual.otherBefore = await run(b, { cache: true });
      actual.otherWrite = await run(b, { local: "second", session: "beta" });
      actual.firstAfter = await run(a, {});
      await a.goto(`${base}/next`, { waitUntil: "networkidle" });
      actual.navigation = await run(a, { cache: true });
      await a.close();
      const c = await context.newPage();
      await c.goto(base, { waitUntil: "networkidle" });
      actual.freshTab = await run(c, { cache: true });
      await run(b, { clear: true });
      actual.clearedPeer = await run(c, {});
      actual.clearedSource = await run(b, {});
      actual.cachedRequests = counts.get("/cached");
      assert.deepEqual(actual, control.result);
    } finally {
      await context.close();
    }
  });
});

test("hydrate, navigation response headers and binary CSD use the retained native profile", async () => {
  await fixture(async (base) => {
    const id = native.requestStateNew(),
      other = native.requestStateNew();
    const hydrate = (html, state = id, metadata = undefined) =>
      native.hydrate(html, base, "[]", "", state, metadata);
    try {
      await hydrate(
        "<body><script>localStorage.setItem('key','local');sessionStorage.setItem('key','session')</script>",
      );
      const read =
        "<body><script>document.body.setAttribute('data-values',JSON.stringify([localStorage.getItem('key'),sessionStorage.getItem('key')]))</script>";
      assert.match(await hydrate(read), /data-values="\[&quot;local&quot;,&quot;session&quot;\]"/);
      assert.match(await hydrate(read, other), /data-values="\[null,null\]"/);
      const response = JSON.parse(
        await native.fetchWithCookies(
          `${base}/clear`,
          "[]",
          undefined,
          undefined,
          undefined,
          false,
          id,
        ),
      );
      assert.equal(response.status, 200);
      assert.match(
        await hydrate(read, id, JSON.stringify(response)),
        /data-values="\[null,null\]"/,
      );
      await hydrate(
        "<body><script>localStorage.setItem('key','again');sessionStorage.setItem('key','again')</script>",
      );
      await native.fetchBytes(`${base}/clear-image`, id, base, "[]");
      assert.match(await hydrate(read), /data-values="\[null,null\]"/);
      const policy = JSON.stringify({
        status: 418,
        referrer: `${base}/source`,
        headers: [["content-security-policy", "script-src 'none'"]],
      });
      const html = await hydrate("<body><script>document.body.id='blocked'</script>", id, policy);
      assert.doesNotMatch(html, /id="blocked"/);
    } finally {
      native.requestStateClose(id);
      native.requestStateClose(other);
    }
  });
});

test("closed native profile handles reject admission and existing tabs keep their ownership", async () => {
  const parent = native.requestStateNew(),
    tab = native.requestStateNew(parent);
  const base = "https://profile-close.test/";
  const live = await native.liveOpen("<body>", base, "[]", "", tab);
  try {
    native.requestStateClose(parent);
    assert.throws(() => native.requestStateNew(parent), /no such request state/);
    native.requestStateClose(tab);
    assert.throws(() => native.hydrate("<body>", base, "[]", "", tab), /no such request state/);
    await assert.rejects(
      native.fetchWithCookies(base, "[]", undefined, undefined, undefined, false, tab),
      /no such request state/,
    );
    await assert.rejects(native.fetchBytes(base, tab), /no such request state/);
    assert.equal(
      await native.liveEval(
        live,
        "sessionStorage.setItem('owned','yes');globalThis.__RESULT=sessionStorage.getItem('owned')",
      ),
      "yes",
    );
  } finally {
    native.liveClose(live);
    native.requestStateClose(parent);
    native.requestStateClose(tab);
  }
  await assert.rejects(
    native.liveEval(live, "globalThis.__RESULT='closed'"),
    /no such live session/,
  );
});

test("shim closes native tab IDs before context IDs and releases an implicitly owned context", async () => {
  const context = new BrowserContext();
  const page = await context.newPage();
  const parent = context._requestState,
    tab = page._requestState;
  await page.close();
  assert.throws(() => native.requestStateNew(tab), /no such request state/);
  const sibling = native.requestStateNew(parent);
  native.requestStateClose(sibling);
  await context.close();
  assert.throws(() => native.requestStateNew(parent), /no such request state/);
  await assert.rejects(context.newPage(), /context is closed/);
  await assert.rejects(page.goto("about:blank"), /page is closed/);
  const implicit = newPage({ userAgent: "Profile test" });
  const implicitParent = implicit._context._requestState;
  assert.equal(implicit._context._userAgent, "Profile test");
  await implicit.close();
  assert.throws(() => native.requestStateNew(implicitParent), /no such request state/);
});
