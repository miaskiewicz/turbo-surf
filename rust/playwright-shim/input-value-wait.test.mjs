import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { newPage } from "./index.mjs";

test("inputValue waits for a delayed attached input instead of fabricating an empty value", async () => {
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end('<div id="field"></div>');
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.evaluate(() => {
      setTimeout(() => {
        const input = document.createElement("input");
        input.value = "served company";
        document.querySelector("#field").append(input);
      }, 100);
    });
    assert.equal(
      await page.locator("#field input").inputValue({ timeout: 2000 }),
      "served company",
    );
    await page.evaluate(() => {
      document.querySelector("input").value = "";
    });
    assert.equal(await page.locator("input").inputValue(), "");
    await assert.rejects(page.locator("#missing").inputValue({ timeout: 25 }), /timed out/);
    await assert.rejects(page.locator("#field").inputValue(), /not an <input>/);
    await page.evaluate(() => {
      document.querySelector("#field").append(document.createElement("input"));
    });
    await assert.rejects(page.locator("input").inputValue(), /strict mode violation/);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});
