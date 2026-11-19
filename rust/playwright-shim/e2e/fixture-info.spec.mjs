import assert from "node:assert/strict";
import { test } from "@playwright/test";
let attempt;
test.beforeEach((_fixtures, info) => {
  attempt = info;
});
test.afterEach((_fixtures, info) => {
  assert.equal(info, attempt);
  assert.equal(test.info(), info);
  assert.equal(info.status, "passed");
  assert.equal(info.attachments.length, 1);
  assert.equal(info.attachments[0].name, "diagnostic");
  assert.equal(info.attachments[0].body.toString(), "evidence");
});
test("attachments stay on the current attempt", async (_fixtures, info) => {
  assert.equal(info, attempt);
  assert.equal(test.info(), info);
  await info.attach("diagnostic", { body: "evidence", contentType: "text/plain" });
});
