import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("clearCookies intersects name/domain/path filters and preserves unrelated identities", async () => {
  const context = new shim.BrowserContext();
  try {
    await context.addCookies([
      { name: "drop", value: "1", domain: "one.test", path: "/app" },
      { name: "drop", value: "2", domain: "two.test", path: "/app" },
      { name: "drop", value: "3", domain: "one.test", path: "/other" },
      { name: "keep", value: "4", domain: "one.test", path: "/app" },
    ]);
    await context.clearCookies({ name: /^drop$/g, domain: "one.test", path: /^\/app$/g });
    assert.deepEqual(
      (await context.cookies()).map((cookie) => cookie.value),
      ["2", "3", "4"],
    );
    await context.clearCookies({ name: "drop" });
    assert.deepEqual(
      (await context.cookies()).map((cookie) => cookie.name),
      ["keep"],
    );
    await context.clearCookies();
    assert.deepEqual(await context.cookies(), []);
  } finally {
    await context.close();
  }
});
