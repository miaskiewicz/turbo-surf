import assert from "node:assert/strict";
import { test } from "node:test";
import { cssSecurityErrors } from "./css-security.mjs";

const valid = {
  ...Object.fromEntries(
    [
      "allowedSheet",
      "blockedSheet",
      "sheetApplies",
      "initialBlocked",
      "removed",
      "dynamicBlocked",
      "dynamicNonce",
      "privateHooks",
    ].map((key) => [key, true]),
  ),
  authored: "opacity:0.25",
  initialDeclaration: "",
  cssom: { applies: true },
  cssText: { applies: true },
  deniedAttribute: { applies: true, attribute: "opacity:0.75", declaration: "0.5" },
  deniedAttrNode: { applies: true, attribute: "opacity:0.8", declaration: "0.5" },
};

test("CSS authorization oracle accepts the Chrome policy outcome", () => {
  assert.deepEqual(cssSecurityErrors(valid), []);
});

test("CSS authorization oracle separates authored and effective attributes", () => {
  assert.deepEqual(
    cssSecurityErrors({
      ...valid,
      removed: false,
      deniedAttrNode: { ...valid.deniedAttrNode, declaration: "0.8" },
    }),
    ["removed", "authored and effective attribute separation"],
  );
  assert.deepEqual(
    cssSecurityErrors({ ...valid, deniedAttribute: { applies: true, attribute: "opacity:0.75" } }),
    ["authored and effective attribute separation"],
  );
});
