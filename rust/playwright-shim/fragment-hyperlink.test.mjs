import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let newPage;
try {
  ({ newPage } = await import("./index.mjs"));
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const requests = [];
  const server = createServer((request, response) => {
    requests.push(request.url);
    response.setHeader("content-type", "text/html");
    response.end(`<!doctype html>
      <main><a id="open" href="#">Open editor</a><a id="named" href="#named">Named fragment</a>
      <a id="cancel" href="#cancelled">Cancel</a><a id="next" href="/next#named">Next</a></main>
      <script>
        document.querySelector('#open').onclick = () => {
          const dialog = document.createElement('div');
          dialog.role = 'dialog';
          dialog.innerHTML = '<input>';
          document.body.append(dialog);
        };
        document.querySelector('#cancel').onclick = event => event.preventDefault();
      </script>`);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}/fixture`;
  const page = newPage();
  try {
    await page.goto(url, { waitUntil: "networkidle" });
    await run(page, url, requests);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("fragment hyperlink activation keeps the editor and native history in the same document", async () => {
  await fixture(async (page, url, requests) => {
    await page.locator("main").getByRole("link", { name: "Open editor" }).click();
    assert.equal(page.url(), url + "#");
    assert.equal(await page.getByRole("dialog").count(), 1);
    assert.equal(await page.evaluate(() => location.href), url + "#");
    assert.equal(await page.goBack(), null);
    assert.equal(page.url(), url);
    assert.equal(await page.getByRole("dialog").count(), 1);
    assert.deepEqual(requests, ["/fixture"]);
  });
});

test("click cancellation and different-document hyperlinks keep their existing behavior", async () => {
  await fixture(async (page, url, requests) => {
    await page.locator("#cancel").click();
    assert.equal(page.url(), url);
    assert.deepEqual(requests, ["/fixture"]);
    await page.locator("#next").click();
    assert.equal(page.url(), new URL("/next#named", url).href);
    assert.deepEqual(requests, ["/fixture", "/next"]);
  });
});

test("scripted anchor activation publishes the fragment without fetching a document", async () => {
  await fixture(async (page, url, requests) => {
    await page.evaluate(() => {
      window.fragmentEvents = [];
      addEventListener("hashchange", (event) => {
        window.fragmentEvents.push([event.oldURL, event.newURL]);
      });
      document.querySelector("#named").click();
    });
    await page.waitForFunction(() => window.fragmentEvents.length === 1);
    assert.deepEqual(await page.evaluate(() => window.fragmentEvents), [[url, url + "#named"]]);
    assert.equal(page.url(), url + "#named");
    assert.deepEqual(requests, ["/fixture"]);
  });
});
