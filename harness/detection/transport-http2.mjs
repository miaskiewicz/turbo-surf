// Real localhost TLS/H2 request control. Certificates and keys stay in owned scratch storage.
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createSecureServer } from "node:http2";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, copyFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const run = (command, args) =>
  new Promise((done, fail) => {
    const child = spawn(command, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "",
      stderr = "";
    child.stdout.on("data", (data) => {
      stdout += data;
    });
    child.stderr.on("data", (data) => {
      stderr += data;
    });
    const timer = setTimeout(() => child.kill("SIGTERM"), 60000);
    child.on("error", fail);
    child.on("close", (code) => {
      clearTimeout(timer);
      code === 0 ? done(stdout) : fail(new Error(`${command} exited ${code}: ${stderr}`));
    });
  });
export function normalizeHttp2(row) {
  return {
    alpn: row.alpn,
    settings: row.settings,
    connectionWindow: row.connectionWindow,
    headerOrder: row.rawHeaders.filter((_, index) => index % 2 === 0),
    headers: Object.fromEntries(
      row.rawHeaders.reduce((pairs, value, index, array) => {
        if (index % 2 === 0 && value !== ":authority") pairs.push([value, array[index + 1]]);
        return pairs;
      }, []),
    ),
  };
}
export async function compareHttp2({ binary, out }) {
  mkdirSync(out, { recursive: true, mode: 0o700 });
  const scratch = mkdtempSync(join(tmpdir(), "turbo-http2-fixture-"));
  const key = join(scratch, "key.pem"),
    cert = join(scratch, "cert.pem");
  execFileSync(
    "openssl",
    [
      "req",
      "-x509",
      "-newkey",
      "rsa:2048",
      "-nodes",
      "-days",
      "1",
      "-subj",
      "/CN=127.0.0.1",
      "-addext",
      "subjectAltName=IP:127.0.0.1",
      "-keyout",
      key,
      "-out",
      cert,
    ],
    { stdio: "ignore" },
  );
  const sessions = new Set(),
    rows = [];
  const server = createSecureServer({
    key: readFileSync(key),
    cert: readFileSync(cert),
    remoteCustomSettings: [8, 9],
  });
  server.on("session", (session) => {
    sessions.add(session);
    session.on("close", () => sessions.delete(session));
  });
  server.on("stream", (stream, _headers, _flags, rawHeaders) => {
    const session = stream.session;
    rows.push({
      alpn: session.socket.alpnProtocol,
      settings: session.remoteSettings,
      connectionWindow: session.state.remoteWindowSize,
      rawHeaders,
    });
    stream.respond({ ":status": 200, "content-type": "text/html" });
    stream.end("<!doctype html><body>transport fixture</body>");
  });
  const frozen = join(out, "native-executable");
  try {
    copyFileSync(binary, frozen);
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const url = `https://127.0.0.1:${server.address().port}/`;
    const nativeOutput = await run(frozen, [url, cert]);
    const native = rows.splice(0).at(-1);
    const config = {
      url,
      localCertificate: true,
      waitMs: 0,
      steps: [],
      successSelector: "body",
      languages: ["en-US", "en"],
      locale: "en-US",
    };
    writeFileSync(join(out, "config.json"), JSON.stringify(config), { mode: 0o600 });
    const chromeOut = join(out, "chrome");
    await run(process.execPath, [
      join(here, "chrome-cdp-trace.mjs"),
      join(out, "config.json"),
      chromeOut,
    ]);
    const chrome = rows.find((row) => row.rawHeaders[row.rawHeaders.indexOf(":path") + 1] === "/");
    if (!native || !chrome) throw new Error("Both native and Chrome must complete the H2 request");
    const trace = JSON.parse(readFileSync(join(chromeOut, "trace.json"), "utf8"));
    const normalized = { chrome: normalizeHttp2(chrome), native: normalizeHttp2(native) };
    const result = {
      schemaVersion: 1,
      collectedAt: new Date().toISOString(),
      chromeVersion: trace.browserVersion,
      nativeBuild: JSON.parse(nativeOutput.trim()),
      binaryHash: createHash("sha256").update(readFileSync(frozen)).digest("hex"),
      note: "Localhost HTTPS/H2 only. Chrome certificate exception is confined to the controlled fixture; native trusts only its ephemeral fixture certificate. No Google traffic, cookies or private certificate keys retained.",
      chrome,
      native,
      normalized,
      differences: differences(normalized.chrome, normalized.native),
    };
    writeFileSync(join(out, "comparison.json"), JSON.stringify(result, null, 2), { mode: 0o600 });
    return result;
  } finally {
    for (const session of sessions) session.destroy();
    await new Promise((done) => server.close(done));
    rmSync(scratch, { recursive: true, force: true });
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  if (!args.includes("--run")) console.log("Use --run for the localhost HTTP/2 comparison");
  else {
    const out = resolve(flag("--out", join(here, "artifacts", `http2-${Date.now()}`)));
    const result = await compareHttp2({
      binary: resolve(flag("--binary", "rust/target/debug/examples/transport_http2")),
      out,
    });
    console.log(
      JSON.stringify({
        out,
        chromeVersion: result.chromeVersion,
        differences: result.differences.length,
      }),
    );
  }
}
