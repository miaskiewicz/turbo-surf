import assert from "node:assert/strict";
import { test } from "@playwright/test";

for (const scope of ["first", "second"]) {
  test.describe(scope, () => {
    let hookPage;
    test.beforeEach(({ page }) => {
      assert.ok(page, `missing page in ${scope}`);
      hookPage = page;
    });
    test.afterEach(({ page }) => assert.equal(page, hookPage));
    test(`shares fixtures in ${scope}`, ({ page }) => assert.equal(page, hookPage));
  });
}
