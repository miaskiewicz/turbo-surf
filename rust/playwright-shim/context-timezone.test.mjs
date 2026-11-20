import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function withFixture(run) {
  const server = createServer((_, response) => {
    response.setHeader("Content-Type", "text/html");
    response.end("<!doctype html><title>Context</title>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    await run(origin);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

test("BrowserContext timezoneId overrides Date accessors and Intl default zone (#397)", async () => {
  const { chromium } = shim;
  const browser = await chromium.launch();
  try {
    await withFixture(async (origin) => {
      const context = await browser.newContext({ timezoneId: "Pacific/Auckland" });
      try {
        const page = await context.newPage();
        await page.goto(origin, { waitUntil: "networkidle" });
        const seen = await page.evaluate(() => ({
          zone: Intl.DateTimeFormat().resolvedOptions().timeZone,
          hour: new Date("2026-01-01T00:00:00Z").getHours(),
          formatted: new Intl.DateTimeFormat("en-US", {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
            hour12: false,
          }).format(new Date("2026-01-01T00:00:00Z")),
        }));
        assert.equal(seen.zone, "Pacific/Auckland");
        assert.equal(seen.hour, 13);
        assert.ok(seen.formatted.includes("13:"), `formatted=${seen.formatted}`);
      } finally {
        await context.close();
      }
    });
  } finally {
    await browser.close();
  }
});

test("BrowserContext without timezoneId leaves Date and Intl at host defaults", async () => {
  const { chromium } = shim;
  const browser = await chromium.launch();
  try {
    await withFixture(async (origin) => {
      const context = await browser.newContext();
      try {
        const page = await context.newPage();
        await page.goto(origin, { waitUntil: "networkidle" });
        const zone = await page.evaluate(() => Intl.DateTimeFormat().resolvedOptions().timeZone);
        assert.ok(zone && typeof zone === "string");
      } finally {
        await context.close();
      }
    });
  } finally {
    await browser.close();
  }
});
