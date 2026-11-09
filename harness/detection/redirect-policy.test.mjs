import assert from "node:assert/strict";
import { test } from "node:test";
import { redirectErrors } from "./redirect-policy.mjs";

const valid = {
  same: true,
  cross: false,
  back: false,
  manual: { type: "opaqueredirect", status: 0, ok: false, url: "/manual", headers: [], text: "" },
  errorRejected: true,
  unsafeAllowed: true,
  unsafeDenied: true,
};
const requests = ["/unsafe-step", "/unsafe-final", "/unsafe-blocked"].map((path) => ({
  path,
  method: "OPTIONS",
}));

test("redirect oracle accepts the Chrome authorization/redirect outcome", () => {
  assert.deepEqual(redirectErrors(valid, requests), []);
});

test("redirect oracle checks the filtered manual response and request log", () => {
  assert.deepEqual(
    redirectErrors(
      { ...valid, manual: { ...valid.manual, headers: [["x", "y"]] }, unsafeDenied: false },
      [
        ...requests.filter((row) => row.path !== "/unsafe-final"),
        { path: "/unsafe-blocked", method: "PUT" },
        { path: "/error-final", method: "GET" },
      ],
    ),
    [
      "manual filtered response",
      "redirected preflight authorization",
      "denied unsafe request was sent",
      "/unsafe-final preflight absent",
      "/error-final was followed",
    ],
  );
  assert.deepEqual(redirectErrors(undefined, []), [
    "authorization origin transition",
    "manual filtered response",
    "error mode rejection",
    "redirected preflight authorization",
    "/unsafe-step preflight absent",
    "/unsafe-final preflight absent",
    "/unsafe-blocked preflight absent",
  ]);
});
