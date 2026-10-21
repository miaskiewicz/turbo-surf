import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);
let native;
try {
  native = require("../../rust/crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
}
const probe = readFileSync(new URL("./css-diagnostics.js", import.meta.url), "utf8");
const tracer = readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8");
const html = "<!doctype html><head><style>p { color: blue; }</style></head><body></body>";

for (const instrumented of [false, true]) {
  test(
    `CSS diagnostics preserve stylesheet identity and cleanup (${instrumented ? "traced" : "baseline"})`,
    {
      skip: !native && "native addon is not built",
    },
    () => {
      const result = JSON.parse(native.evaluate(html, `${instrumented ? tracer : ""}\n${probe}`));
      assert.equal(result.rows.length, 4);
      for (const row of result.rows) {
        assert.equal(row.before, 1);
        for (const state of [row.first, row.changed]) {
          for (const key of [
            "connected",
            "ownerDocument",
            "inHead",
            "directSheet",
            "registered",
            "sameSheet",
            "ownerNode",
          ]) {
            assert.equal(state[key], true, `${key}: ${JSON.stringify(row)}`);
          }
          assert.equal(state.listedCount, 2);
        }
        assert.equal(row.first.directRules, 2);
        assert.equal(row.first.css[1].selector, "local-css-sentinel");
        assert.equal(row.changed.directRules, 1);
        assert.equal(row.changed.css[0].selector, "local-css-changed");
      }
      assert.equal(result.document.sheetCount, 1);
      assert.equal(result.document.sheetOwners.length, 1);
    },
  );
}

test(
  "CSS diagnostics expose list/direct-sheet disagreement and preserve the page",
  {
    skip: !native && "native addon is not built",
  },
  () => {
    const result = JSON.parse(
      native.evaluate(
        html,
        `
    Object.defineProperty(document, 'styleSheets', {get() {return [];}, configurable: true});
    const report = ${probe.trim().replace(/;$/, "")};
    JSON.stringify({report: JSON.parse(report), styles: document.querySelectorAll('style').length});
  `,
      ),
    );
    for (const row of result.report.rows) {
      assert.equal(row.first.directSheet, true);
      assert.equal(row.first.directRules, 2);
      assert.equal(row.first.registered, false);
      assert.equal(row.first.sameSheet, false);
    }
    assert.equal(result.styles, 1);
  },
);

test(
  "CSS diagnostics retain the parent stylesheet list after asynchronous frame DOM work",
  { skip: !native && "native addon is not built" },
  async () => {
    const id = await native.liveOpen(
      `${html}<script>setTimeout(() => {
        const frame=document.createElement('iframe');document.body.appendChild(frame);
        const childStyle=frame.contentDocument.createElement('STYLE');
        frame.contentDocument.head.appendChild(childStyle);
        childStyle.appendChild(frame.contentDocument.createTextNode('child-sentinel {}'));
        const style=document.createElement('STYLE');document.head.appendChild(style);
        style.appendChild(document.createTextNode('late-parent-sentinel {}'));
      }, 1);</script>`,
      "https://css-diagnostic.test/",
    );
    try {
      const result = JSON.parse(
        await native.liveEval(id, `globalThis.__RESULT=(${probe.trim().replace(/;$/, "")});`),
      );
      assert.equal(result.document.sheetCount, 2);
      for (const row of result.rows) {
        assert.equal(row.before, 2);
        assert.equal(row.first.registered, true);
        assert.equal(row.first.directRules, 2);
        assert.equal(row.changed.registered, true);
        assert.equal(row.changed.directRules, 1);
      }
    } finally {
      native.liveClose(id);
    }
  },
);
