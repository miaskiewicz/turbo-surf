import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("press signals: native addon not built, skipping");
  process.exit(0);
}

function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function bounded(promise, message) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(message)), 1500);
    }),
  ]).finally(() => clearTimeout(timer));
}
async function fixture(run) {
  const htmlRequested = deferred();
  const scriptRequested = deferred();
  const keyupRequested = deferred();
  const rejectedRequested = deferred();
  const held = new Map();
  const requests = [];
  const server = createServer((request, response) => {
    requests.push(`${request.method} ${request.url}`);
    response.setHeader("content-type", "text/html");
    if (request.url.startsWith("/reject")) {
      rejectedRequested.resolve();
      request.socket.destroy();
    } else if (request.url === "/keyup") {
      keyupRequested.resolve();
      response.end("ok");
    } else if (request.url === "/held.js") {
      response.setHeader("content-type", "application/javascript");
      held.set("script", response);
      response.write("/* script is held */");
      scriptRequested.resolve();
    } else if (request.url.startsWith("/held-html")) {
      held.set("html", response);
      response.write('<p id="replacement">replacement');
      htmlRequested.resolve();
    } else if (request.url.startsWith("/script-document")) {
      response.end('<p id="replacement">committed</p><script src="/held.js"></script>');
    } else if (request.url.startsWith("/result")) {
      response.end(`<p id="replacement">${request.url}</p>`);
    } else {
      response.end(
        `<form action="/held-html"><input id="q" name="q" value="signal"><button style="position:absolute;left:20px;top:80px;width:100px;height:40px">Go</button></form>`,
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const context = new shim.BrowserContext();
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    const page = await context.newPage();
    await page.goto(origin, { waitUntil: "networkidle" });
    await run({
      page,
      context,
      origin,
      requests,
      held,
      htmlRequested,
      scriptRequested,
      keyupRequested,
      rejectedRequested,
    });
  } finally {
    for (const response of held.values()) response.end();
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test(
  "Locator.press dispatches keyup before awaiting held replacement HTML",
  { timeout: 10000 },
  async () => {
    await fixture(async ({ page, held, htmlRequested, keyupRequested }) => {
      await page.evaluate(() => {
        document.querySelector("#q").addEventListener("keyup", () => fetch("/keyup"));
      });
      let settled = false;
      const action = page
        .locator("#q")
        .press("Enter")
        .finally(() => {
          settled = true;
        });
      try {
        await bounded(htmlRequested.promise, "form request was not admitted");
        await bounded(
          keyupRequested.promise,
          "keypress waited for navigation before dispatching keyup",
        );
        assert.equal(settled, false, "press must await replacement HTML commit");
        let observation;
        try {
          const outcome = await Promise.race([
            action.then(() => "settled"),
            new Promise((resolve) => {
              observation = setTimeout(() => resolve("held"), 100);
            }),
          ]);
          assert.equal(outcome, "held", "press settled while its main HTML was still held");
        } finally {
          clearTimeout(observation);
        }
        held.get("html").end("</p>");
        await bounded(action, "press did not finish after commit");
        assert.match(page.url(), /\/held-html\?q=signal$/);
        assert.match(await page.content(), /id="replacement"/);
      } finally {
        held.get("html")?.end();
        await action.catch(() => {});
      }
    });
  },
);

test(
  "Locator.press settles at commit before an external script reaches EOF",
  { timeout: 10000 },
  async () => {
    await fixture(async ({ page, held, scriptRequested }) => {
      await page.evaluate(() => {
        document.querySelector("form").action = "/script-document";
      });
      await bounded(page.locator("#q").press("Enter"), "press waited for external script EOF");
      assert.equal(new URL(page.url()).pathname, "/script-document");
      await bounded(scriptRequested.promise, "committed document never requested its real script");
      assert.equal(page._readEngine("return globalThis.scriptFinished === true;"), false);
      held.get("script").end("globalThis.scriptFinished=true;");
      await page.waitForLoadState("load", { timeout: 1500 });
    });
  },
);

for (const kind of ["keyboard", "mouse", "touch"]) {
  test(`raw ${kind} returns before replacement HTML commit`, { timeout: 10000 }, async () => {
    await fixture(async ({ page, held, htmlRequested }) => {
      await page.locator("#q").focus();
      const box = await page.locator("button").boundingBox();
      const action =
        kind === "keyboard"
          ? page.keyboard.press("Enter")
          : kind === "mouse"
            ? page.mouse.click(box.x + box.width / 2, box.y + box.height / 2)
            : page.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
      await bounded(action, `raw ${kind} incorrectly waited for replacement EOF`);
      await bounded(htmlRequested.promise, `raw ${kind} navigation did not progress`);
      held.get("html").end("</p>");
      await page.waitForURL(/\/held-html/, { timeout: 1500 });
    });
  });
}

test(
  "press without a new signal does not await preexisting held own navigation",
  { timeout: 10000 },
  async () => {
    await fixture(async ({ page, origin, held, htmlRequested }) => {
      await page.evaluate(() => {
        location.assign("/held-html");
      });
      await bounded(htmlRequested.promise, "preexisting navigation was not admitted");
      await bounded(page.locator("#q").press("ArrowLeft"), "press awaited an older own navigation");
      assert.equal(page.url(), `${origin}/`);
      held.get("html").end("</p>");
      await page.waitForURL(/\/held-html/, { timeout: 1500 });
    });
  },
);

for (const peer of ["held", "rejected"]) {
  test(
    `press own commit is isolated from ${peer} sibling navigation`,
    { timeout: 10000 },
    async () => {
      await fixture(async ({ page, context, origin, held, htmlRequested, rejectedRequested }) => {
        const sibling = await context.newPage();
        await sibling.goto(origin, { waitUntil: "networkidle" });
        const destination = peer === "held" ? "/held-html" : "/reject";
        await sibling.evaluate((url) => {
          location.assign(url);
        }, destination);
        if (peer === "held") await bounded(htmlRequested.promise, "sibling request never started");
        else await bounded(rejectedRequested.promise, "sibling failed request never started");
        await page.evaluate(() => {
          document.querySelector("form").action = "/result";
        });
        await bounded(
          page.locator("#q").press("Enter"),
          "own press consumed sibling completion/error",
        );
        assert.match(page.url(), /\/result\?q=signal$/);
        assert.match(await page.content(), /id="replacement"/);
        held.get("html")?.end("</p>");
      });
    },
  );
}

for (const last of ["form", "location"]) {
  test(
    `mixed native admissions keep the latest ${last} destination`,
    { timeout: 10000 },
    async () => {
      await fixture(async ({ page, requests }) => {
        await page.evaluate((kind) => {
          const form = document.querySelector("form");
          form.action = "/result-form";
          document.querySelector("#q").addEventListener("keydown", (event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            if (kind === "form") {
              location.assign("/result-location");
              form.submit();
            } else {
              form.submit();
              location.assign("/result-location");
            }
          });
        }, last);
        await bounded(page.locator("#q").press("Enter"), "mixed navigation did not commit");
        assert.equal(new URL(page.url()).pathname, `/result-${last}`);
        assert.equal(
          requests.filter((entry) => entry.includes("/result-")).length,
          1,
          "a superseded queued navigation must not issue a real request",
        );
      });
    },
  );
}

test(
  "Locator.press releases its signal barrier when the admitted navigation fails",
  { timeout: 10000 },
  async () => {
    await fixture(async ({ page, rejectedRequested, origin }) => {
      await page.evaluate(() => {
        document.querySelector("form").action = "/reject";
      });
      await bounded(
        page.locator("#q").press("Enter"),
        "failed navigation did not release press barrier",
      );
      await bounded(rejectedRequested.promise, "failed destination was never requested");
      assert.equal(page.url(), `${origin}/`);
    });
  },
);

test(
  "closing an action owner rejects instead of releasing a successful press barrier",
  { timeout: 10000 },
  async () => {
    await fixture(async ({ page, htmlRequested }) => {
      const action = page.locator("#q").press("Enter");
      const outcome = action.then(
        () => "resolved",
        () => "rejected",
      );
      await bounded(htmlRequested.promise, "owned request was not admitted");
      await page.close();
      assert.equal(await bounded(outcome, "closed owner did not cancel the action"), "rejected");
    });
  },
);
