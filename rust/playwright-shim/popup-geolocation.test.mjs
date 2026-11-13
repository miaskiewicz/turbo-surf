import assert from "node:assert/strict";
import { createServer } from "node:http";
import { setImmediate as nextTurn } from "node:timers/promises";
import test, { after } from "node:test";
import { pathToFileURL } from "node:url";

const shimSource = process.env.TURBO_SURF_TEST_SHIM;
if (!shimSource) throw new Error("Select the actual combined popup shim with TURBO_SURF_TEST_SHIM");
const server = createServer((request, response) => {
  response.setHeader("content-type", "text/html");
  response.end(
    request.url === "/child"
      ? "<!doctype html><body><p id='child-document'>Actual child document</p></body>"
      : "<!doctype html><body><p>Actual opener</p></body>",
  );
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([origin]);
const { chromium } = await import(pathToFileURL(shimSource).href);
after(() => new Promise((resolve) => server.close(resolve)));

const permission = () =>
  navigator.permissions.query({ name: "geolocation" }).then((status) => status.state);
const position = () =>
  new Promise((resolve, reject) =>
    navigator.geolocation.getCurrentPosition(
      ({ coords }) => resolve([coords.latitude, coords.longitude, coords.accuracy]),
      ({ code }) => reject(new Error(`Geolocation error ${code}`)),
    ),
  );

async function evaluationReady(context, marker) {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const state = await context.storageState();
    if (
      state.origins.some((entry) =>
        entry.localStorage.some((item) => item.name === marker && item.value === "1"),
      )
    )
      return;
    await nextTurn();
  }
  assert.fail("The real child evaluation did not publish its public storage readiness effect");
}

test(
  "actual popup geolocation follows context updates after its opener closes (#326 integration)",
  { timeout: 15000 },
  async () => {
    const browser = await chromium.launch();
    const context = await browser.newContext({
      permissions: ["geolocation"],
      geolocation: { latitude: 40, longitude: -74 },
    });
    try {
      const opener = await context.newPage();
      await opener.goto(origin, { waitUntil: "load" });
      const created = opener.waitForEvent("popup", { timeout: 2000 });
      await opener.evaluate((url) => {
        window.open(url, "_blank");
        return true;
      }, `${origin}/child`);
      const popup = await created;
      await popup.waitForURL(`${origin}/child`, { timeout: 2000 });
      await popup.waitForLoadState("load", { timeout: 2000 });
      await popup.locator("#child-document").waitFor({ state: "attached", timeout: 2000 });
      assert.equal(await popup.evaluate(() => document.URL), `${origin}/child`);
      assert.equal(
        await popup.evaluate(permission),
        "granted",
        "the real child inherits the context grant",
      );
      assert.deepEqual(await popup.evaluate(position), [40, -74, 0]);
      assert.deepEqual(
        await popup.evaluate(async () => {
          globalThis.geoLog = [];
          globalThis.geoErrors = [];
          globalThis.permissionChanges = [];
          globalThis.geoStatus = await navigator.permissions.query({ name: "geolocation" });
          geoStatus.onchange = () => permissionChanges.push(geoStatus.state);
          return new Promise((resolve, reject) => {
            globalThis.geoWatch = navigator.geolocation.watchPosition(
              ({ coords }) => {
                const value = [coords.latitude, coords.longitude, coords.accuracy];
                geoLog.push(value);
                if (geoLog.length === 1) resolve(value);
                if (globalThis.nextPosition) {
                  const done = nextPosition;
                  nextPosition = null;
                  done(value);
                }
              },
              ({ code }) => {
                geoErrors.push(code);
                if (globalThis.nextError) {
                  const done = nextError;
                  nextError = null;
                  done(code);
                } else reject(new Error(`Initial geolocation error ${code}`));
              },
            );
          });
        }),
        [40, -74, 0],
      );
      await opener.close();
      assert.equal(popup.isClosed(), false, "closing the opener must preserve the real child");

      // This first post-close update gives a direct semantic failure rather than
      // masking lost actor configuration behind a later evaluation timeout.
      await context.setGeolocation({ latitude: 41, longitude: -73, accuracy: 7 });
      assert.deepEqual(
        await popup.evaluate(position),
        [41, -73, 7],
        "the surviving actor receives context coordinates",
      );

      const update = popup.evaluate(
        () =>
          new Promise((resolve) => {
            globalThis.nextPosition = resolve;
            localStorage.setItem("popup-geo-update-waiting", "1");
          }),
      );
      update.catch(() => {});
      await evaluationReady(context, "popup-geo-update-waiting");
      await context.setGeolocation({ latitude: 42, longitude: -72, accuracy: 8 });
      assert.deepEqual(
        await update,
        [42, -72, 8],
        "the host updates settings while the child evaluation is pending",
      );

      const denied = popup.evaluate(
        () =>
          new Promise((resolve) => {
            globalThis.nextError = resolve;
            localStorage.setItem("popup-geo-permission-waiting", "1");
          }),
      );
      denied.catch(() => {});
      await evaluationReady(context, "popup-geo-permission-waiting");
      await context.clearPermissions();
      assert.equal(await denied, 1);
      assert.equal(await popup.evaluate(permission), "prompt");
      assert.deepEqual(
        await popup.evaluate(() => ({ changes: permissionChanges, errors: geoErrors })),
        { changes: ["prompt"], errors: [1] },
      );
      assert.deepEqual(await popup.evaluate(() => geoLog), [
        [40, -74, 0],
        [41, -73, 7],
        [42, -72, 8],
      ]);
    } finally {
      await context.close();
      await browser.close();
    }
  },
);
