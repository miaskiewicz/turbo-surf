import test from "node:test";
import { checkXhrBodies } from "./xhr-body-fixture.mjs";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test(
  "routed XHR ignores GET/HEAD bodies while Fetch stays strict and POST retains them",
  { timeout: 15000 },
  async () => {
    const page = shim.newPage();
    try {
      await checkXhrBodies(page);
    } finally {
      await page.close();
    }
  },
);
