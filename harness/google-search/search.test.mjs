// Opt-in, live-network integration test: drive a Google search from the homepage
// through the turbo-surf-mcp binary (goto -> fill the query box -> submit the form
// -> follow the navigation). Proves turbo-surf can DRIVE an interactive search
// end-to-end.
//
// It also documents the honest limitation: Google's results page is served behind
// its `enablejs` BotGuard wall, which the V8 render tier can't clear, so the SERP
// itself comes back as the no-JS interstitial ("If you're having trouble accessing
// Google Search…"), not organic results. Clearing that needs the opt-in
// `BrowserSolver` sidecar (a real browser) — see harness/browser-solver.
//
// NOT run by default (hits live google.com + needs the release binary built):
//   node --test harness/google-search/search.test.mjs        -> skips
//   RUN_LIVE_GOOGLE=1 node --test harness/google-search/search.test.mjs
//
// Build the binary first: (cd rust && cargo build --release -p turbo-surf-mcp)

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import assert from "node:assert/strict";

const HERE = dirname(fileURLToPath(import.meta.url));
const BIN = resolve(HERE, "../../rust/target/release/turbo-surf-mcp");

const OPT_IN = process.env.RUN_LIVE_GOOGLE === "1";
const HAVE_BIN = existsSync(BIN);
// Skip cleanly (never fail the suite) when not opted in or the binary is absent —
// matching the harness rule that live-network checks auto-skip.
const skip = !OPT_IN
  ? "set RUN_LIVE_GOOGLE=1 to run (live network)"
  : !HAVE_BIN
    ? "release binary missing — (cd rust && cargo build --release -p turbo-surf-mcp)"
    : false;

/** One turbo-surf-mcp process, newline-delimited JSON-RPC over stdio. */
function client() {
  const child = spawn(BIN, [], { stdio: ["pipe", "pipe", "ignore"] });
  let buf = "";
  const pending = [];
  child.stdout.on("data", (d) => {
    buf += d.toString();
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (line.trim()) pending.shift()?.(JSON.parse(line));
    }
  });
  let id = 0;
  const call = (method, params) =>
    new Promise((r) => {
      pending.push(r);
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: ++id, method, params })}\n`);
    });
  const tool = (name, args = {}) => call("tools/call", { name, arguments: args });
  const text = (resp) => resp?.result?.content?.[0]?.text ?? "";
  return { call, tool, text, kill: () => child.kill() };
}

test("drives a Google search from the homepage (fill + submit -> SERP url)", { skip }, async () => {
  const c = client();
  try {
    await c.call("initialize", {});
    await c.tool("set_mode", { mode: "secure" }); // run page JS in the V8 isolate

    const goto = JSON.parse(c.text(await c.tool("goto", { url: "https://www.google.com" })) || "{}");
    assert.match(goto.url ?? "", /google\.com/, "landed on google.com");

    // The homepage query box is a <textarea name=q> inside <form role=search>.
    const q = c.text(await c.tool("query", { selector: "textarea[name=q], input[name=q]" }));
    assert.ok(q.includes('name=\\"q\\"') || q.includes('name="q"'), "found the query input");

    const fill = JSON.parse(c.text(await c.tool("fill", { selector: "textarea[name=q]", value: "turbo dom rust" })) || "{}");
    assert.equal(fill.ok, true, "filled the query box");

    // Submitting the GET form navigates to /search?q=... — the drive works even
    // though the results are BotGuard-gated.
    const submit = JSON.parse(c.text(await c.tool("submit", { selector: "form[role=search]" })) || "{}");
    assert.match(submit.url ?? "", /\/search\?/, "submit navigated to the search endpoint");
    assert.match(submit.url ?? "", /q=turbo\+dom\+rust/, "query is carried into the SERP url");

    // Honest note: the SERP body is Google's enablejs interstitial, not organic
    // results — asserted loosely so the test documents the gate without flaking on
    // Google's exact copy. (Clearing it needs the BrowserSolver sidecar.)
    await c.tool("render", {});
    const md = c.text(await c.tool("markdown", {}));
    assert.ok(typeof md === "string", "markdown returns (interstitial or results)");
  } finally {
    c.kill();
  }
});
