import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { startResourceFixture } from "./resource-policy.mjs";

test("reusable resource fixture serves exact binary hashes and distinct origin policies", async () => {
  const fixture = await startResourceFixture();
  try {
    assert.notEqual(new URL(fixture.origin).origin, new URL(fixture.cross).origin);
    const html = await (await fetch(fixture.origin)).text();
    const bytes = new Uint8Array(await (await fetch(`${fixture.origin}/data`)).arrayBuffer());
    assert.deepEqual(Array.from(bytes), [114, 97, 119, 255, 98, 121, 116, 101, 115]);
    const digest = createHash("sha256").update(bytes).digest("base64");
    assert.ok(html.includes(`sha256-${digest}`));
    assert.ok(html.includes(fixture.cross));
    assert.equal(
      (await fetch(`${fixture.cross}/corp-origin.js`)).headers.get("cross-origin-resource-policy"),
      "same-origin",
    );
    assert.equal(
      (await fetch(`${fixture.cross}/corp-duplicate.js`)).headers.get(
        "cross-origin-resource-policy",
      ),
      "same-origin, same-site",
    );
    assert.equal(
      (await fetch(`${fixture.origin}/blocked.css`)).headers.get("content-type"),
      "text/css",
    );
  } finally {
    await fixture.close();
  }
});
