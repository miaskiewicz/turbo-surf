import assert from "node:assert/strict";
import { test } from "node:test";
import { bodyFor, contentTypeFor, credentialErrors } from "./script-credentials.mjs";

const valid = {
  module: { value: "credentialed dependency", current: true, url: "/entry.mjs" },
  anonymous: true,
  classic: true,
  legacy: true,
  nosniff: true,
  json: true,
  cookie: "",
  redirect: { value: "redirect dependency", url: "/final/entry.mjs" },
  events: [
    ...["module", "anonymous", "classic", "redirect", "legacy"].map((id) => [id, "load", true]),
    ["denied", "error", true],
    ["nosniff", "error", true],
    ["json", "error", true],
  ],
};
const requests = [
  ...["/entry.mjs", "/dep.mjs", "/classic.js"].map((path) => ({
    path,
    cookie: true,
    origin: "source",
  })),
  { path: "/anonymous.mjs", cookie: false, origin: "source" },
  { path: "/final/entry.mjs", cookie: true, origin: "null" },
];

test("credential oracle accepts the Chrome credential/redirect shape", () => {
  assert.deepEqual(credentialErrors(valid, requests), []);
});

test("credential oracle reports value, event and request failures in order", () => {
  const value = {
    ...valid,
    module: { ...valid.module, current: false },
    json: false,
    cookie: "secret=session",
    events: valid.events.map(([id, type, trusted]) =>
      id === "classic"
        ? [id, "error", trusted]
        : id === "denied"
          ? [id, "load", trusted]
          : [id, type, id !== "json"],
    ),
  };
  const rows = [
    ...requests.filter((row) => row.path !== "/dep.mjs"),
    { path: "/anonymous.mjs", cookie: true },
    { path: "/must-not-request.mjs" },
  ];
  assert.deepEqual(credentialErrors(value, rows), [
    "credentialed module graph",
    "json MIME policy",
    "HttpOnly visibility",
    "classic event",
    "denied redirect event",
    "json MIME event",
    "/dep.mjs credentials/origin",
    "anonymous module credentials",
    "denied redirect was followed",
  ]);
  assert.deepEqual(credentialErrors(undefined, []), [
    "credentialed module graph",
    "script completion",
    "legacy MIME policy",
    "nosniff MIME policy",
    "json MIME policy",
    "HttpOnly visibility",
    "redirect source/import identity",
    "module event",
    "anonymous event",
    "classic event",
    "redirect event",
    "legacy event",
    "denied redirect event",
    "nosniff MIME event",
    "json MIME event",
    "/entry.mjs credentials/origin",
    "/dep.mjs credentials/origin",
    "/classic.js credentials/origin",
    "redirect-tainted origin",
  ]);
});

test("fixture server routes MIME types and bodies per path", () => {
  assert.equal(contentTypeFor("/"), "text/html");
  assert.equal(contentTypeFor("/nosniff.js"), "text/plain");
  assert.equal(contentTypeFor("/legacy.js"), "text/plain");
  assert.equal(contentTypeFor("/json.mjs"), "application/json");
  assert.equal(contentTypeFor("/entry.mjs"), "text/javascript");
  assert.equal(bodyFor("/classic.js"), "globalThis.credentialClassic=true;");
  assert.match(bodyFor("/entry.mjs"), /dep\.mjs/);
  assert.match(bodyFor("/final/entry.mjs"), /redirectModule/);
  assert.equal(bodyFor("/missing.mjs"), null);
  assert.equal(bodyFor("/"), null);
});
