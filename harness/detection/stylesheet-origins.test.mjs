import assert from "node:assert/strict";
import { test } from "node:test";
import { stylesheetErrors } from "./stylesheet-origins.mjs";

const allowed = { sheet: true, rules: 1, insert: "allowed" };
const denied = { sheet: true, rules: "SecurityError", insert: "SecurityError" };
const valid = {
  same: allowed,
  cors: allowed,
  include: allowed,
  opaque: denied,
  redirect: denied,
  denied,
  mime: denied,
  "same-mime": denied,
  imported: { sheet: true, rules: "SecurityError" },
  events: [
    ["cors", "load", true],
    ["denied", "error", true],
    ["include", "load", true],
    ["mime", "error", true],
    ["opaque", "load", true],
    ["redirect", "load", true],
    ["same", "load", true],
    ["same-mime", "error", true],
  ],
  opaqueApplies: true,
  corsApplies: true,
  importApplies: true,
};

test("stylesheet origin oracle accepts the Chrome CSSOM access outcome", () => {
  assert.deepEqual(stylesheetErrors(valid), []);
});

test("stylesheet origin oracle checks readable and opaque sheets separately", () => {
  assert.deepEqual(
    stylesheetErrors({
      ...valid,
      cors: denied,
      opaque: allowed,
      mime: { ...denied, sheet: false },
      imported: { sheet: true, rules: 1 },
      events: [],
      corsApplies: false,
    }),
    ["cors", "opaque", "mime", "import origin", "parser resource events", "corsApplies"],
  );
});
