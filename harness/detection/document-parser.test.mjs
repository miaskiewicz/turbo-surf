import assert from "node:assert/strict";
import { test } from "node:test";
import { parserErrors } from "./document-parser.mjs";

const doc = (url) => ({
  view: true,
  location: true,
  inert: true,
  rootOwned: true,
  bodyOwned: true,
  headOwned: true,
  documentCtor: true,
  documentProto: true,
  url,
  cookie: "",
  ready: "complete",
});
const valid = {
  distinct: true,
  parent: true,
  created: true,
  dom: doc("http://127.0.0.1:4000/"),
  unsafe: doc("about:blank"),
  safe: {
    ...doc("about:blank"),
    head: "<title>parsed</title>",
    body: "<p><a>link</a></p><svg><a>vector</a></svg><math><mi>x</mi></math>",
  },
  import: { owned: true, original: true, copied: true, text: "link" },
  invalid: ["TypeError"],
};

test("parsed-document oracle accepts independent inert documents", () => {
  assert.deepEqual(parserErrors(valid), []);
});

test("parsed-document oracle checks each document's flags and URL rule", () => {
  assert.deepEqual(
    parserErrors({
      ...valid,
      parent: false,
      dom: doc("about:blank"),
      unsafe: { ...doc("about:blank"), headOwned: false },
      safe: { ...valid.safe, url: "http://127.0.0.1:4000/", body: "" },
      import: { ...valid.import, copied: false },
      invalid: ["TypeError", "SyntaxError"],
    }),
    [
      "parent",
      "dom independent document",
      "unsafe independent document",
      "safe independent document",
      "native default sanitizer",
      "native import",
      "MIME validation",
    ],
  );
  assert.deepEqual(parserErrors(undefined), [
    "distinct",
    "parent",
    "created",
    "dom independent document",
    "unsafe independent document",
    "safe independent document",
    "native default sanitizer",
    "native import",
  ]);
});
