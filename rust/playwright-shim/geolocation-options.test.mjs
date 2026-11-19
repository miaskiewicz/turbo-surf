import assert from "node:assert/strict";
import test from "node:test";
import {
  contextGeolocation,
  contextPermissionGrant,
  mergePermissionGrant,
} from "./geolocation-options.mjs";

test("geolocation rejects invalid coordinates before replacing a context option", () => {
  assert.deepEqual(contextGeolocation({ latitude: 40, longitude: -74 }), {
    latitude: 40,
    longitude: -74,
    accuracy: 0,
  });
  assert.equal(contextGeolocation(null), null);
  for (const value of [
    { latitude: 91, longitude: 0 },
    { latitude: 0, longitude: -181 },
    { latitude: NaN, longitude: 0 },
    { latitude: 0, longitude: 0, accuracy: -1 },
  ])
    assert.throws(() => contextGeolocation(value), /Invalid geolocation/);
});

test("permissions accumulate within one canonical origin and preserve other scopes", () => {
  const first = contextPermissionGrant(["geolocation"], { origin: "https://example.test/path" });
  let grants = mergePermissionGrant([], first);
  grants = mergePermissionGrant(
    grants,
    contextPermissionGrant(["notifications", "geolocation"], {
      origin: "https://example.test/other",
    }),
  );
  grants = mergePermissionGrant(grants, contextPermissionGrant(["camera"]));
  assert.deepEqual(grants, [
    { origin: "https://example.test", permissions: ["geolocation", "notifications"] },
    { origin: null, permissions: ["camera"] },
  ]);
  assert.throws(() => contextPermissionGrant(["not-a-permission"]), /Unsupported/);
  assert.throws(
    () => contextPermissionGrant(["geolocation"], { origin: "data:text/plain,test" }),
    /Invalid permission origin/,
  );
});
