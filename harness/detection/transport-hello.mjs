import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
export const isGrease = (value) => (value & 0x0f0f) === 0x0a0a && value >> 8 === (value & 0xff);

// Parsed public capabilities accompany the exact raw record saved by the collector.
export function parseClientHello(record) {
  let offset = 0;
  const take = (count) => {
    if (offset + count > record.length) throw new Error("truncated ClientHello");
    const value = record.subarray(offset, offset + count);
    offset += count;
    return value;
  };
  const u16 = () => take(2).readUInt16BE();
  const vector = () => take(u16());
  if (take(1)[0] !== 22) throw new Error("expected TLS handshake record");
  const recordVersion = u16();
  const recordLength = u16();
  if (recordLength + 5 !== record.length) throw new Error("invalid TLS record length");
  if (take(1)[0] !== 1) throw new Error("expected ClientHello");
  const handshakeBytes = take(3);
  const handshakeLength = handshakeBytes.readUIntBE(0, 3);
  if (handshakeLength + 4 !== recordLength) throw new Error("invalid ClientHello length");
  const legacyVersion = u16();
  take(32);
  take(take(1)[0]);
  const words = (bytes) => {
    if (bytes.length % 2) throw new Error("invalid uint16 list");
    return Array.from({ length: bytes.length / 2 }, (_, i) => bytes.readUInt16BE(i * 2));
  };
  const ciphers = words(vector());
  const compressionMethods = Array.from(take(take(1)[0]));
  const extensionBytes = vector();
  const extensions = [];
  for (let i = 0; i < extensionBytes.length; ) {
    if (i + 4 > extensionBytes.length) throw new Error("truncated TLS extension");
    const type = extensionBytes.readUInt16BE(i);
    const length = extensionBytes.readUInt16BE(i + 2);
    i += 4;
    if (i + length > extensionBytes.length) throw new Error("truncated TLS extension body");
    const bytes = extensionBytes.subarray(i, i + length);
    i += length;
    const entry = { type, length };
    if ([10, 13].includes(type)) entry.values = words(bytes.subarray(2));
    if (type === 43) entry.values = words(bytes.subarray(1));
    if (type === 16) {
      if (bytes.length < 2 || bytes.readUInt16BE() + 2 !== bytes.length)
        throw new Error("invalid ALPN protocol list");
      entry.protocols = [];
      for (let at = 2; at < bytes.length; ) {
        const length = bytes[at++];
        if (!length || at + length > bytes.length) throw new Error("invalid ALPN protocol");
        entry.protocols.push(bytes.subarray(at, at + length).toString("latin1"));
        at += length;
      }
    }
    if (type === 51) {
      if (bytes.length < 2 || bytes.readUInt16BE() + 2 !== bytes.length)
        throw new Error("invalid key share list");
      entry.keyShares = [];
      for (let at = 2; at < bytes.length; ) {
        if (at + 4 > bytes.length) throw new Error("truncated key share");
        const group = bytes.readUInt16BE(at),
          length = bytes.readUInt16BE(at + 2);
        at += 4;
        if (at + length > bytes.length) throw new Error("truncated key share body");
        entry.keyShares.push({ group, length });
        at += length;
      }
    }
    if (type === 51764) entry.publicRootIds = bytes.toString("hex");
    extensions.push(entry);
  }
  return { recordVersion, legacyVersion, compressionMethods, ciphers, extensions };
}

export function normalizeHello(hello) {
  return {
    ...(hello.recordVersion === undefined
      ? {}
      : {
          recordVersion: hello.recordVersion,
          legacyVersion: hello.legacyVersion,
          compressionMethods: hello.compressionMethods,
        }),
    ciphers: hello.ciphers.filter((value) => !isGrease(value)),
    cipherGrease: hello.ciphers.some(isGrease),
    extensions: hello.extensions
      .filter((entry) => !isGrease(entry.type))
      .map((entry) => {
        const result = { ...entry };
        // ECH GREASE intentionally varies its opaque payload length per connection.
        if (result.type === 65037) delete result.length;
        if (result.keyShares) {
          result.keyShareGreaseCount = result.keyShares.filter((value) =>
            isGrease(value.group),
          ).length;
          result.keyShares = result.keyShares.filter((value) => !isGrease(value.group));
        }
        if (result.values) {
          result.grease = result.values.some(isGrease);
          result.values = result.values.filter((value) => !isGrease(value));
        }
        return result;
      })
      .sort((a, b) => a.type - b.type),
    extensionGreaseCount: hello.extensions.filter((entry) => isGrease(entry.type)).length,
  };
}

export async function captureClientHello(start, { out, label } = {}) {
  let resolveCapture;
  let rejectCapture;
  const captured = new Promise((done, fail) => {
    resolveCapture = done;
    rejectCapture = fail;
  });
  const sockets = new Set();
  const server = createServer((socket) => {
    sockets.add(socket);
    let bytes = Buffer.alloc(0);
    socket.on("data", (chunk) => {
      bytes = Buffer.concat([bytes, chunk]);
      if (bytes.length > 65540) {
        rejectCapture(new Error("oversized TLS record"));
        socket.destroy();
      }
      if (bytes.length >= 5 && bytes.length >= 5 + bytes.readUInt16BE(3)) {
        try {
          const record = bytes.subarray(0, 5 + bytes.readUInt16BE(3));
          const raw = {
            sha256: createHash("sha256").update(record).digest("hex"),
            bytes: record.length,
          };
          if (out !== undefined) {
            if (!["native", "chrome"].includes(label)) throw new Error("invalid capture label");
            mkdirSync(out, { recursive: true, mode: 0o700 });
            raw.file = `${label}-client-hello.bin`;
            writeFileSync(join(out, raw.file), record, { mode: 0o600 });
          }
          resolveCapture({ ...parseClientHello(record), raw });
        } catch (error) {
          rejectCapture(error);
        }
        socket.destroy();
      }
    });
    socket.on("error", () => {});
    socket.on("close", () => sockets.delete(socket));
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  let finish;
  const timer = setTimeout(() => rejectCapture(new Error("ClientHello capture timed out")), 20000);
  try {
    finish = await start(`https://127.0.0.1:${server.address().port}/`);
    return await captured;
  } finally {
    clearTimeout(timer);
    for (const socket of sockets) socket.destroy();
    await new Promise((done) => server.close(done));
    await finish?.();
  }
}

export async function compareTransport({ binary, out }) {
  mkdirSync(out, { recursive: true, mode: 0o700 });
  const frozen = join(out, "native-executable");
  copyFileSync(binary, frozen);
  const native = await captureClientHello(
    async (url) => {
      const child = spawn(frozen, [url], { stdio: "ignore" });
      return () =>
        new Promise((done) => {
          if (child.exitCode !== null) done();
          else {
            child.kill();
            child.once("close", done);
          }
        });
    },
    { out, label: "native" },
  );
  const { chromium } = createRequire(join(root, "scripts/browser-sidecar/package.json"))(
    "patchright",
  );
  const profile = mkdtempSync(join(out, "chrome-profile-"));
  let chromeVersion;
  let chrome;
  try {
    chrome = await captureClientHello(
      async (url) => {
        const context = await chromium.launchPersistentContext(profile, {
          channel: "chrome",
          headless: false,
        });
        chromeVersion = context.browser().version();
        context
          .pages()[0]
          .goto(url)
          .catch(() => {});
        return () => context.close();
      },
      { out, label: "chrome" },
    );
  } finally {
    rmSync(profile, { recursive: true, force: true });
  }
  const normalized = { native: normalizeHello(native), chrome: normalizeHello(chrome) };
  const result = {
    schemaVersion: 1,
    collectedAt: new Date().toISOString(),
    chromeVersion,
    binaryHash: createHash("sha256").update(readFileSync(frozen)).digest("hex"),
    note: "Localhost ClientHello only. Exact raw ClientHello records are retained with hashes. GREASE values, extension permutation and opaque ECH payload lengths vary intentionally in the normalized comparison. No HTTP requests or browser cookies reused.",
    native,
    chrome,
    normalized,
    differences: differences(normalized.chrome, normalized.native),
  };
  writeFileSync(join(out, "comparison.json"), JSON.stringify(result, null, 2), { mode: 0o600 });
  return result;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const flag = (name, fallback) => {
    const index = args.indexOf(name);
    return index < 0 ? fallback : args[index + 1];
  };
  if (!args.includes("--run"))
    console.log("Use --run for the localhost Chrome/native ClientHello comparison.");
  else {
    const out = resolve(flag("--out", join(here, "artifacts", `transport-${Date.now()}`)));
    const result = await compareTransport({
      binary: resolve(flag("--binary", join(root, "rust/target/debug/examples/transport_hello"))),
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
