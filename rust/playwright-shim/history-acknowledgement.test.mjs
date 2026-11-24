import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
let shim, native;
try {
  shim = await import("./index.mjs");
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(html, run) {
  const requests = [];
  const server = createServer((request, response) => {
    requests.push(request.url);
    response.setHeader("content-type", "text/html");
    response.end(html);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  try {
    await run(page, origin, requests);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test(
  "navigation observes acknowledged history during real authored timer mutations",
  { timeout: 15000 },
  async () => {
    await fixture(
      `<body>ready<script>
    globalThis.mutations=0;
    setInterval(()=>{for(let i=0;i<20;i++)history.replaceState({n:++mutations},'',location.pathname+'?n='+mutations)},1);
  </script></body>`,
      async (page, origin, requests) => {
        await page.goto(origin + "/start", { waitUntil: "load", timeout: 3000 });
        await page.evaluate(() => true);
        for (let i = 0; i < 30; i++) {
          const response = await page.goto(origin + "/changing", {
            waitUntil: "load",
            timeout: 3000,
          });
          assert.equal(response.status(), 200);
          assert.equal(new URL(page.url()).pathname, "/changing");
        }
        assert.equal(requests.filter((url) => url === "/changing").length, 30);
        assert.ok((await page.evaluate(() => mutations)) > 0);
      },
    );
  },
);

test(
  "history publication read leaves host route replies available during an active command",
  { timeout: 10000 },
  async () => {
    await fixture(
      `<body><button id="fetch">Fetch</button><div id="result">pending</div><script>
    document.querySelector('#fetch').onclick=async()=>{document.querySelector('#result').textContent=await(await fetch('/flight')).text()};
  </script></body>`,
      async (page, origin) => {
        let reached;
        const routed = new Promise((resolve) => {
          reached = resolve;
        });
        let release;
        const held = new Promise((resolve) => {
          release = resolve;
        });
        await page.route("**/flight", async (route) => {
          reached();
          await held;
          await route.fulfill({ status: 200, body: "actual-route-reply" });
        });
        await page.goto(origin + "/start", { waitUntil: "load", timeout: 3000 });
        const clicking = page.locator("#fetch").click({ timeout: 3000 });
        clicking.catch(() => {});
        try {
          await routed;
          const encoded = await native.liveTargetHistoryStateAsync(page._session, 1000);
          const snapshot = JSON.parse(encoded);
          assert.equal(snapshot.entries[snapshot.index].url, origin + "/start");
          assert.equal(snapshot.pendingTransaction, undefined);
        } finally {
          release();
        }
        await clicking;
        assert.equal(await page.locator("#result").textContent(), "actual-route-reply");
      },
    );
  },
);
