// Capture the Observable, EventTarget#when and Blob#textStream control (or the
// --probes list, e.g. fetch-form-data-probe.js) in an independently installed Chrome.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const { values } = parseArgs({
  options: {
    runner: { type: "string" },
    out: { type: "string" },
    chrome: { type: "string" },
    probes: { type: "string" },
    // Probes that assert failed loads (revoked blob: URLs, bad data: URLs)
    // record each one as a request failure; those are part of the control.
    "allow-request-failures": { type: "boolean" },
  },
});
if (!values.runner || !values.out)
  throw Error("Pass --runner /absolute/chrome-cdp-trace.mjs --out /absolute/oracle.json");
const probeNames = values.probes
  ? values.probes.split(",")
  : ["observable-shape-probe.js", "observable-semantics-probe.js", "blob-text-stream-probe.js"];
const directory = mkdtempSync(join(tmpdir(), "turbo-web-apis-oracle-"));
// `/echo…` answers with the request it received (form-navigation probes read it
// back from the navigated frame); `/csp-…` pages carry a `form-action 'none'` policy,
// `/self-csp…` pages `form-action 'self'`; `/redirectNNN?to=` answers with that
// redirect status; `/xo-submit?targets=` submits a form to each [name, action] pair.
const echoHeaders = [
  "content-type",
  "content-length",
  "origin",
  "referer",
  "cache-control",
  "upgrade-insecure-requests",
  "sec-fetch-dest",
  "sec-fetch-mode",
  "sec-fetch-site",
  "sec-fetch-user",
];
const server = createServer((request, response) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    response.setHeader("Content-Type", "text/html");
    const origin = `http://${request.headers.host}`;
    const query = new URL(request.url, origin).searchParams;
    const redirect = /^\/redirect(\d{3})\?/.exec(request.url);
    if (redirect) {
      response.writeHead(Number(redirect[1]), { Location: query.get("to") });
      response.end();
      return;
    }
    if (request.url.startsWith("/xo-submit?")) {
      const submit = `for (const [target, action] of ${query.get("targets")}) {
        const form = document.createElement("form");
        form.method = "post"; form.action = action; form.target = target;
        document.body.append(form);
        try { form.submit(); } catch (_) {}
      }`;
      response.end(
        `<!doctype html><html><head></head><body><script>${submit}</script></body></html>`,
      );
      return;
    }
    if (request.url.startsWith("/echo")) {
      const boundary = /boundary=(\S+)/.exec(request.headers["content-type"] ?? "")?.[1];
      const scrub = (text) => {
        let value = text.replaceAll(origin, "<origin>");
        if (boundary) value = value.replaceAll(boundary, "<boundary>");
        return value;
      };
      const echo = {
        method: request.method,
        url: request.url,
        headers: Object.fromEntries(
          echoHeaders
            .filter((name) => name in request.headers)
            .map((name) => [name, scrub(request.headers[name])]),
        ),
        body: scrub(Buffer.concat(chunks).toString("latin1")),
      };
      const text = JSON.stringify(echo).replaceAll("&", "&amp;").replaceAll("<", "&lt;");
      response.end(
        `<!doctype html><html><head></head><body><pre id="echo">${text}</pre></body></html>`,
      );
      return;
    }
    const directive = request.url.startsWith("/csp-")
      ? "'none'"
      : request.url.startsWith("/self-csp")
        ? "'self'"
        : "";
    const policy = directive
      ? `<meta http-equiv="Content-Security-Policy" content="form-action ${directive}">`
      : "";
    response.end(`<!doctype html><html><head>${policy}</head><body></body></html>`);
  });
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
let complete = false;
try {
  const config = join(directory, "config.json");
  writeFileSync(
    config,
    JSON.stringify({
      url: `http://127.0.0.1:${server.address().port}/`,
      waitMs: 25,
      disableExtensions: true,
      foregroundFrames: true,
      steps: [],
      probes: probeNames.map((name) => join(root, "harness/detection", name)),
      ...(values.chrome ? { chromePath: resolve(values.chrome) } : {}),
    }),
  );
  const output = join(directory, "capture");
  const child = spawn(process.execPath, [resolve(values.runner), config, output], {
    stdio: "inherit",
  });
  const status = await new Promise((done) => child.once("exit", done));
  if (status !== 0) throw Error(`Chrome capture exited ${status}`);
  const trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
  if (
    trace.errors.some((error) => !(values["allow-request-failures"] && error.failure)) ||
    trace.probes.length !== probeNames.length ||
    trace.probes.some((probe) => probe.error || !probe.result)
  )
    throw Error("Chrome control failed; inspect preserved diagnostics");
  const oracle = {
    browser: trace.browserVersion,
    v8: trace.browserV8Version,
    recordedAt: new Date().toISOString(),
    fixtures: Object.fromEntries(
      probeNames.map((name) => [
        name,
        createHash("sha256")
          .update(readFileSync(join(root, "harness/detection", name)))
          .digest("hex"),
      ]),
    ),
    effectiveExtensionFlags: trace.effectiveExtensionFlags,
    result: Object.fromEntries(probeNames.map((name, index) => [name, trace.probes[index].result])),
  };
  writeFileSync(resolve(values.out), `${JSON.stringify(oracle, null, 2)}\n`);
  console.log(JSON.stringify({ browser: oracle.browser, out: resolve(values.out) }));
  complete = true;
} finally {
  await new Promise((done) => server.close(done));
  if (complete) rmSync(directory, { recursive: true, force: true });
  else console.error(`Failed capture preserved at ${directory}`);
}
