// Transport-identity control (turbo-surf#113): the slice of a Chrome identity
// battery capture that pins the HTTP/1.1 Connection/Priority behaviour, the
// HTTP/2 per-stream priorities, Sec-CH-DPR and the Critical-CH restart.
//
//   node harness/detection/chrome-identity-battery.mjs --chrome <oracle> --out <battery.json>
//   node harness/detection/transport-identity-control.mjs --battery <battery.json> \
//     --out rust/crates/turbo-surf-core/tests/fixtures/transport_identity_chrome156.json
//
// The battery runs only loopback servers (identity-server.mjs); labels already
// hide ports and hosts. rust/crates/turbo-surf-core/tests/transport_identity.rs
// replays the same requests against turbo-surf and compares them to this file.
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const KINDS = new Set(["h1-nav", "http-nav", "ch-accept", "ch-sub", "ch-next", "critical"]);

/** The control derived from a `--chrome` battery fixture. */
export function transportControl(battery) {
  if (battery.subject !== "chrome") throw new Error("the control must come from a Chrome capture");
  const requests = battery.requests.filter((request) => request.tier === "chrome");
  const header = (request, name) =>
    request.headers.find(([key]) => key.toLowerCase() === name)?.[1] ?? null;
  return {
    schemaVersion: 1,
    browser: battery.browser.product,
    collectedAt: battery.collectedAt,
    note: "Loopback-only capture by chrome-identity-battery.mjs; regenerate with transport-identity-control.mjs.",
    requests: requests
      .filter((request) => KINDS.has(request.kind))
      .map(({ kind, origin, method, protocol, headers }) => ({
        kind,
        origin,
        method,
        protocol,
        headers,
      })),
    streams: requests
      .filter((request) => request.protocol === "h2" && request.kind !== "favicon")
      .map((request) => ({
        kind: request.kind,
        origin: request.origin,
        priority: header(request, "priority"),
        streamPriority: request.streamPriority,
      })),
    connections: battery.connections
      .filter((connection) => connection.h2)
      .map((connection) => ({ origin: connection.origin, h2: connection.h2 })),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const battery = flag("--battery");
  const out = flag("--out");
  if (!battery || !out) {
    console.error("Usage: node transport-identity-control.mjs --battery <file> --out <file>");
    process.exit(2);
  }
  const control = transportControl(JSON.parse(readFileSync(resolve(battery), "utf8")));
  writeFileSync(resolve(out), `${JSON.stringify(control, null, 2)}\n`);
  console.log(JSON.stringify({ out: resolve(out), requests: control.requests.length }));
}
