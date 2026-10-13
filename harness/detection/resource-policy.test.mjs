import assert from "node:assert/strict";
import { test } from "node:test";
import { cspErrors, policy } from "./csp.mjs";
import { redirectErrors } from "./redirect-policy.mjs";
import { credentialErrors } from "./script-credentials.mjs";

test("credential oracle rejects cookie leakage and denied redirects even when scripts finish", () => {
  const value = {
    module: { value: "credentialed dependency", current: true, url: "/entry.mjs" },
    anonymous: true,
    classic: true,
    redirect: { value: "redirect dependency", url: "/final/entry.mjs" },
    cookie: "",
    legacy: true,
    nosniff: true,
    json: true,
    events: ["module", "anonymous", "classic", "redirect", "legacy"]
      .map((id) => [id, "load", true])
      .concat(["denied", "nosniff", "json"].map((id) => [id, "error", true])),
  };
  const requests = ["/entry.mjs", "/dep.mjs", "/classic.js"]
    .map((path) => ({ path, cookie: true, origin: "source" }))
    .concat([
      { path: "/anonymous.mjs", cookie: false, origin: "source" },
      { path: "/final/entry.mjs", cookie: true, origin: "null" },
    ]);
  assert.deepEqual(credentialErrors(value, requests), []);
  assert.ok(credentialErrors(null, []).includes("credentialed module graph"));
  assert.ok(
    credentialErrors(value, [
      ...requests,
      { path: "/anonymous.mjs", cookie: true, origin: "source" },
    ]).includes("anonymous module credentials"),
  );
  assert.ok(
    credentialErrors(value, [...requests, { path: "/must-not-request.mjs" }]).includes(
      "denied redirect was followed",
    ),
  );
  assert.ok(
    credentialErrors({ ...value, cookie: "secret=session" }, requests).includes(
      "HttpOnly visibility",
    ),
  );
});
test("CSP oracle checks both actual execution and absent blocked requests", () => {
  const value = Object.fromEntries(
    [
      "inlineBlocked",
      "hashAllowed",
      "nonceAllowed",
      "evalBlocked",
      "connectBlocked",
      "classic",
      "module",
      "externalBlocked",
    ].map((key) => [key, true]),
  );
  const requests = ["/allowed.js", "/allowed.mjs", "/dep.mjs"];
  assert.deepEqual(cspErrors(value, requests), []);
  assert.ok(
    cspErrors(value, [...requests, "/blocked-frame"]).includes("/blocked-frame blocked request"),
  );
  assert.ok(cspErrors({ ...value, inlineBlocked: false }, requests).includes("inlineBlocked"));
  assert.ok(cspErrors(null, []).length > 0);
  assert.match(policy, /'nonce-good'/);
  assert.match(policy, /'sha256-/);
});

test("redirect oracle rejects origin leaks and contact after manual/error modes", () => {
  const value = {
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
  assert.deepEqual(redirectErrors(value, requests), []);
  assert.ok(
    redirectErrors({ ...value, cross: true }, []).includes("authorization origin transition"),
  );
  assert.ok(
    redirectErrors({ ...value, back: true }, []).includes("authorization origin transition"),
  );
  assert.ok(
    redirectErrors({ ...value, manual: { ...value.manual, status: 200 } }, []).includes(
      "manual filtered response",
    ),
  );
  assert.ok(
    redirectErrors(value, [{ path: "/manual-final" }]).includes("/manual-final was followed"),
  );
  assert.ok(
    redirectErrors(value, [{ path: "/error-final" }]).includes("/error-final was followed"),
  );
  assert.ok(
    redirectErrors({ ...value, errorRejected: false }, []).includes("error mode rejection"),
  );
  assert.ok(
    redirectErrors({ ...value, unsafeAllowed: false }, requests).includes(
      "redirected preflight authorization",
    ),
  );
  assert.ok(
    redirectErrors(value, [...requests, { path: "/unsafe-blocked", method: "PUT" }]).includes(
      "denied unsafe request was sent",
    ),
  );
  assert.ok(redirectErrors(null, []).length > 0);
});
