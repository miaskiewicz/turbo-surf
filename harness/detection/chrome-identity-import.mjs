// Import a pre-battery Chrome capture (TRANSPORT-<major>.json + its raw
// ClientHello + browser-release-chrome<major>.json) into the identity-fixture
// shape, marked `partial`, so the first battery run has a baseline to diff.
//
//   node harness/detection/chrome-identity-import.mjs 155
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describeClientHello } from "./identity-wire.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const sha256 = (data) => createHash("sha256").update(data).digest("hex");

/** Build the partial identity fixture for `major` from the legacy captures. */
export function importLegacy(major, read = (name) => readFileSync(join(here, name))) {
  const transportName = `TRANSPORT-${major}.json`;
  const releaseName = `fixtures/browser-release-chrome${major}.json`;
  const transport = JSON.parse(read(transportName));
  const release = JSON.parse(read(releaseName));
  const helloName = transport.rawRecords.chrome.fixture;
  const hello = read(helloName);
  if (sha256(hello) !== transport.rawRecords.chrome.sha256)
    throw new Error(`${helloName} does not match its recorded SHA-256`);
  const raw = transport.http2.chrome.rawHeaders;
  const headers = [];
  for (let i = 0; i + 1 < raw.length; i += 2)
    headers.push([raw[i], raw[i] === ":authority" ? "<A-host>" : raw[i + 1]]);
  return {
    schemaVersion: 1,
    subject: "chrome",
    partial: true,
    measured: ["requests", "transport", "js"],
    importedFrom: Object.fromEntries(
      [transportName, releaseName, helloName].map((name) => [name, sha256(read(name))]),
    ),
    collectedAt: transport.collectedAt,
    browser: { product: release.browser, v8: release.v8 },
    note: "Imported from pre-battery captures: one cold H2 navigation, one fresh ClientHello and the UA-CH probe. Everything else is unmeasured for this release.",
    missing: [],
    errors: [],
    pageErrors: [],
    requests: [
      {
        tier: "chrome",
        kind: "nav",
        method: "GET",
        path: "/",
        origin: "A",
        protocol: transport.http2.chrome.alpn,
        headers,
        bodyBytes: 0,
        connection: 0,
        streamPriority: null,
      },
    ],
    connections: [
      {
        id: 0,
        origin: "A",
        alpn: transport.http2.chrome.alpn,
        hello: { ...describeClientHello(hello), rawBase64: hello.toString("base64") },
        h2: null,
      },
    ],
    js: {
      navigator: { userAgent: release.result.userAgent },
      userAgentData: { brands: release.result.brands, high: release.result.high },
    },
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const major = Number(process.argv[2]);
  if (!major) throw new Error("Usage: node chrome-identity-import.mjs <major>");
  const out = join(here, `fixtures/chrome-identity-${major}.json`);
  writeFileSync(out, `${JSON.stringify(importLegacy(major), null, 2)}\n`);
  console.log(out);
}
