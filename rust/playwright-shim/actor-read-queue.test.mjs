import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { createRequire } from "node:module";
let newPage;
let native;
try {
  ({ newPage } = await import("./index.mjs"));
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function whileActorWaits(read) {
  let releaseResponse;
  let requested;
  const arrived = new Promise((resolve) => {
    requested = resolve;
  });
  const server = createServer((req, res) => {
    if (req.url === "/held") {
      releaseResponse = () => {
        if (!res.writableEnded) res.end("ready");
      };
      requested();
    } else {
      res.setHeader("content-type", "text/html");
      res.end(
        '<div data-testid="field"><input value="ready"></div><div role="row"><span>expense-1</span><input type="checkbox"></div><iframe srcdoc="&lt;input value=child&gt;"></iframe>',
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  let work;
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page._ensureLive();
    work = native.liveEvalEngineExpression(
      page._session,
      "fetch('/held').then(response => response.text())",
    );
    await arrived;
    const release = setTimeout(() => releaseResponse(), 25);
    try {
      await read(page);
    } finally {
      clearTimeout(release);
      releaseResponse();
    }
    await work;
  } finally {
    releaseResponse?.();
    await work?.catch(() => {});
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

for (const [name, read] of [
  [
    "simple-selector control",
    async (page) =>
      assert.equal(await page.getByTestId("field").locator("input").inputValue(), "ready"),
  ],
  [
    "value",
    async (page) => assert.equal(await page.locator("input:visible").first().inputValue(), "ready"),
  ],
  [
    "visibility",
    async (page) =>
      assert.equal(
        await page
          .getByRole("row")
          .filter({ hasText: "expense-1" })
          .getByRole("checkbox")
          .isVisible(),
        true,
      ),
  ],
  ["count", async (page) => assert.equal(await page.locator("input:visible").count(), 2)],
  [
    "frame",
    async (page) =>
      assert.equal(
        await page.frameLocator("iframe").locator("input:visible").inputValue(),
        "child",
      ),
  ],
]) {
  test(
    `${name} reads yield Node while an earlier actor command awaits localhost IO`,
    { timeout: 15000 },
    () => whileActorWaits(read),
  );
}
