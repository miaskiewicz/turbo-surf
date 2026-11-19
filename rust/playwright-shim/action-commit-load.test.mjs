import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("action commit/load: native addon not built, skipping");
  process.exit(0);
}

function bounded(promise, milliseconds, message) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(
        () => reject(new Error(typeof message === "function" ? message() : message)),
        milliseconds,
      );
    }),
  ]).finally(() => clearTimeout(timer));
}

test(
  "Locator.click settles at main-document commit before a held script reaches EOF",
  {
    timeout: 15000,
  },
  async () => {
    let finishMain;
    const mainFinished = new Promise((resolve) => {
      finishMain = resolve;
    });
    let requestScript;
    const scriptRequested = new Promise((resolve) => {
      requestScript = resolve;
    });
    let scriptResponse;
    let scriptEnded = false;
    let documents = 0;
    const requests = [];
    const server = createServer((request, response) => {
      const url = new URL(request.url, "http://fixture.test");
      requests.push({
        method: request.method,
        rawURL: request.url,
        pathname: url.pathname,
        search: url.search,
      });
      if (url.pathname === "/held.js") {
        scriptResponse = response;
        response.setHeader("content-type", "application/javascript");
        response.write("/* physical external script EOF is held */");
        requestScript();
        return;
      }
      documents++;
      response.setHeader("content-type", "text/html");
      if (url.pathname === "/next") {
        response.once("finish", finishMain);
        response.end(
          '<p id="committed" data-nonce="commit-load-fixture">new document</p><script src="/held.js"></script>',
        );
      } else {
        response.end(
          '<script>globalThis.oldRealmMarker = true</script><form action="/next"><button>Go</button></form>',
        );
      }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const page = shim.newPage();
    let action;
    let actionSettled = false;
    let actionError;
    const failureContext = () => JSON.stringify({ requests, actionSettled, actionError });
    try {
      await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
      assert.equal(page._readEngine("return globalThis.oldRealmMarker === true;"), true);
      action = page.locator("button").click();
      action.then(
        () => {
          actionSettled = true;
        },
        (error) => {
          actionSettled = true;
          actionError = String(error?.message ?? error);
        },
      );
      // Start the commit bound at the server's physical main-response completion,
      // rather than counting setup or transport time against the action.
      await bounded(
        mainFinished,
        3000,
        () => `replacement main response never finished: ${failureContext()}`,
      );
      await bounded(
        action,
        1000,
        () => `Locator.click waited for LOAD instead of main-document COMMIT: ${failureContext()}`,
      );
      assert.equal(new URL(page.url()).pathname, "/next");
      assert.deepEqual(
        page._readEngine(`return ({
      nonce: document.querySelector('#committed')?.getAttribute('data-nonce'),
      oldRealmGone: typeof globalThis.oldRealmMarker === 'undefined',
      scriptFinished: globalThis.scriptFinished === true
    });`),
        { nonce: "commit-load-fixture", oldRealmGone: true, scriptFinished: false },
      );
      await bounded(scriptRequested, 1000, "replacement did not request its external script");
      assert.equal(scriptEnded, false);
      assert.equal(documents, 2, "one action must fetch exactly one replacement document");
      scriptEnded = true;
      scriptResponse.end("globalThis.scriptFinished = true;");
      await page.waitForLoadState("load", { timeout: 3000 });
      assert.equal(page._readEngine("return globalThis.scriptFinished === true;"), true);
    } finally {
      scriptResponse?.end();
      await page.close();
      await action?.catch(() => {});
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    }
  },
);
