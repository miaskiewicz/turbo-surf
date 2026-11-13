import assert from "node:assert/strict";
import { createServer } from "node:http";
import test, { after } from "node:test";
import { setImmediate as nextTurn } from "node:timers/promises";

const server = createServer((_request, response) => response.end("<!doctype html><body></body>"));
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([origin]);
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  await new Promise((resolve) => server.close(resolve));
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("geolocation: native addon not built, skipping");
  process.exit(0);
}
const { chromium } = shim;
after(() => new Promise((resolve) => server.close(resolve)));

async function withContext(options, run) {
  const browser = await chromium.launch();
  const context = await browser.newContext(options);
  try {
    const page = await context.newPage();
    await page.goto(origin);
    await run(context, page, origin);
  } finally {
    await browser.close();
  }
}

const permission = () =>
  navigator.permissions.query({ name: "geolocation" }).then((status) => status.state);
const position = () =>
  new Promise((resolve) => {
    navigator.geolocation.getCurrentPosition(
      ({ coords }) => resolve([coords.latitude, coords.longitude, coords.accuracy]),
      ({ code }) => resolve({ code }),
      { timeout: 100 },
    );
  });

test("context geolocation configuration and native permission updates (#391)", async () => {
  await withContext(
    { permissions: ["geolocation"], geolocation: { latitude: 40, longitude: -74 } },
    async (context, page) => {
      assert.equal(await page.evaluate(permission), "granted");
      assert.deepEqual(await page.evaluate(position), [40, -74, 0]);
      await context.setGeolocation({ latitude: 12, longitude: 34, accuracy: 5 });
      assert.deepEqual(await page.evaluate(position), [12, 34, 5]);
      await context.clearPermissions();
      assert.equal(await page.evaluate(permission), "prompt");
      assert.deepEqual(await page.evaluate(position), { code: 1 });
    },
  );
});

test("origin grants are distinct and validate before replacing native configuration (#391)", async () => {
  await withContext({}, async (context, page, origin) => {
    await context.grantPermissions(["geolocation"], { origin });
    await context.setGeolocation({ latitude: 1, longitude: 2 });
    assert.equal(await page.evaluate(permission), "granted");
    await assert.rejects(context.setGeolocation({ latitude: 91, longitude: 0 }), /latitude/i);
    assert.deepEqual(await page.evaluate(position), [1, 2, 0]);
    await context.grantPermissions(["geolocation"], { origin: "https://other.test" });
    assert.equal(await page.evaluate(permission), "granted");
  });
});

test(
  "pending public evaluation receives a concurrent host location update without guest polling (#391)",
  { timeout: 10000 },
  async () => {
    await withContext(
      { permissions: ["geolocation"], geolocation: { latitude: 1, longitude: 2 } },
      async (context, page) => {
        assert.equal(await page.evaluate(permission), "granted");
        const pending = page.evaluate(
          () =>
            new Promise((resolve, reject) => {
              const watch = navigator.geolocation.watchPosition(
                ({ coords }) => {
                  if (coords.latitude === 1) {
                    localStorage.setItem("geo-native-evaluation-ready", "1");
                    return;
                  }
                  navigator.geolocation.clearWatch(watch);
                  resolve([coords.latitude, coords.longitude]);
                },
                ({ code }) => reject(new Error(`Geolocation failed: ${code}`)),
              );
            }),
        );
        pending.catch(() => {});
        // Observe the page's real public storage effect through the Send-only host
        // storage API. No second evaluation, guest timer, request or binding drives it.
        const deadline = Date.now() + 3000;
        let ready = false;
        while (!ready && Date.now() < deadline) {
          const state = await context.storageState();
          ready = state.origins.some((entry) =>
            entry.localStorage.some(
              (item) => item.name === "geo-native-evaluation-ready" && item.value === "1",
            ),
          );
          if (!ready) await nextTurn();
        }
        assert.ok(ready, "the pending evaluation must have delivered its initial position");
        await context.setGeolocation({ latitude: 3, longitude: 2 });
        assert.deepEqual(await pending, [3, 2]);
      },
    );
  },
);
