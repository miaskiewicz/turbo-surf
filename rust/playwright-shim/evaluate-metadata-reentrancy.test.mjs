import assert from "node:assert/strict";
import test from "node:test";
let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM ?? "./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("evaluation content includes real DOM mutations caused by iframe metadata reads", async () => {
  const page = shim.newPage();
  try {
    await page.setContent('<h1>before</h1><iframe srcdoc="&lt;p&gt;child&lt;/p&gt;"></iframe>');
    await page.evaluate(() => {
      globalThis.metadataReadCount = 0;
      const original = Element.prototype.getAttribute;
      Element.prototype.getAttribute = function (name) {
        const value = Reflect.apply(original, this, [name]);
        if (this.localName === "iframe" && name === "name" && metadataReadCount === 0) {
          metadataReadCount++;
          document.querySelector("h1").textContent = "metadata read mutation";
          Element.prototype.getAttribute = original;
        }
        return value;
      };
      document.querySelector("h1").textContent = "collector mutation";
    });
    const contentAtEvaluationCompletion = await page.content();
    const actual = await page.evaluate(() => ({
      reads: metadataReadCount,
      heading: document.querySelector("h1").textContent,
    }));
    assert.deepEqual(actual, { reads: 1, heading: "metadata read mutation" });
    assert.match(contentAtEvaluationCompletion, /metadata read mutation/);
    assert.doesNotMatch(contentAtEvaluationCompletion, /collector mutation/);
  } finally {
    await page.close();
  }
});
