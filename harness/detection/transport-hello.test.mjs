import assert from "node:assert/strict";
import test from "node:test";
import { isGrease, normalizeHello, parseClientHello } from "./transport-hello.mjs";

test("TLS normalization preserves capabilities while ignoring protocol randomness", () => {
  const hello = {
    ciphers: [0x2a2a, 4865],
    extensions: [
      { type: 0xaaaa, length: 0 },
      { type: 13, length: 6, values: [0xbaba, 2308] },
      { type: 65037, length: 282 },
      { type: 51764, length: 4, publicRootIds: "00020102" },
    ],
  };
  const other = {
    ciphers: [0x4a4a, 4865],
    extensions: [
      { type: 51764, length: 4, publicRootIds: "00020102" },
      { type: 65037, length: 218 },
      { type: 13, length: 6, values: [0xcaca, 2308] },
      { type: 0xdada, length: 0 },
    ],
  };
  assert.deepEqual(normalizeHello(hello), normalizeHello(other));
  assert.equal(isGrease(0x0a1a), false);
  other.extensions[0].publicRootIds = "00020304";
  assert.notDeepEqual(normalizeHello(hello), normalizeHello(other));
});

test("TLS parser rejects truncated input before accessing random or secret bytes", () => {
  assert.throws(
    () => parseClientHello(Buffer.from([22, 3, 1, 0, 2, 1])),
    /invalid TLS record length/,
  );
  assert.throws(() => parseClientHello(Buffer.from([22])), /truncated ClientHello/);
});

function helloRecord() {
  const u16 = (value) => Buffer.from([value >> 8, value & 255]);
  const vector = (bytes) => Buffer.concat([u16(bytes.length), bytes]);
  const alpn = vector(Buffer.from([2, 104, 50, 8, ...Buffer.from("http/1.1")]));
  const shares = vector(Buffer.concat([u16(29), vector(Buffer.alloc(32, 9))]));
  const extensions = Buffer.concat([u16(16), vector(alpn), u16(51), vector(shares)]);
  const body = Buffer.concat([
    u16(0x0303),
    Buffer.alloc(32, 7),
    Buffer.from([0]),
    vector(Buffer.from([0x13, 1])),
    Buffer.from([1, 0]),
    vector(extensions),
  ]);
  const handshake = Buffer.concat([
    Buffer.from([1, body.length >> 16, (body.length >> 8) & 255, body.length & 255]),
    body,
  ]);
  return Buffer.concat([Buffer.from([22, 3, 1]), vector(handshake)]);
}

test("ClientHello parser retains TLS versions, ALPN and key share capabilities", () => {
  const hello = parseClientHello(helloRecord());
  assert.equal(hello.recordVersion, 0x0301);
  assert.equal(hello.legacyVersion, 0x0303);
  assert.deepEqual(hello.compressionMethods, [0]);
  assert.deepEqual(hello.extensions.find((entry) => entry.type === 16).protocols, [
    "h2",
    "http/1.1",
  ]);
  assert.deepEqual(hello.extensions.find((entry) => entry.type === 51).keyShares, [
    { group: 29, length: 32 },
  ]);
  const modified = structuredClone(hello);
  modified.extensions.find((entry) => entry.type === 16).protocols.reverse();
  assert.notDeepEqual(normalizeHello(hello), normalizeHello(modified));
});

test("ClientHello collector retains byte-exact raw evidence after segmented delivery", async () => {
  const { captureClientHello } = await import("./transport-hello.mjs");
  const { connect } = await import("node:net");
  const { mkdtempSync, readFileSync, rmSync } = await import("node:fs");
  const { join } = await import("node:path");
  const { tmpdir } = await import("node:os");
  const { createHash } = await import("node:crypto");
  const out = mkdtempSync(join(tmpdir(), "turbo-hello-test-"));
  const record = helloRecord();
  try {
    const result = await captureClientHello(
      async (url) => {
        const address = new URL(url);
        const socket = connect({ host: address.hostname, port: Number(address.port) });
        socket.on("error", () => {});
        await new Promise((resolve) => socket.once("connect", resolve));
        socket.write(record.subarray(0, 3));
        await new Promise((resolve) => setImmediate(resolve));
        socket.write(record.subarray(3));
        return () => socket.destroy();
      },
      { out, label: "native" },
    );
    assert.deepEqual(readFileSync(join(out, result.raw.file)), record);
    assert.equal(result.raw.bytes, record.length);
    assert.equal(result.raw.sha256, createHash("sha256").update(record).digest("hex"));
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("fresh paired transport evidence retains every raw ClientHello and bounded admission profile", async () => {
  const { readFileSync } = await import("node:fs");
  const { createHash } = await import("node:crypto");
  const evidence = JSON.parse(readFileSync(new URL("TRANSPORT-155.json", import.meta.url)));
  assert.equal(evidence.chromeVersion, "Chrome/155.0.8059.80");
  assert.equal(evidence.v8, "15.5.35.23");
  assert.equal(evidence.dependency.revision, "4ed3cf0cd4c14c58c557fdd3f3ff6afa4ae24762");
  assert.equal(evidence.dependency.registryCompatible, false);
  const normalized = {};
  for (const [name, entry] of Object.entries(evidence.rawRecords)) {
    const bytes = readFileSync(new URL(entry.fixture, import.meta.url));
    assert.equal(createHash("sha256").update(bytes).digest("hex"), entry.sha256);
    assert.equal(bytes.length, entry.bytes);
    normalized[name] = normalizeHello(parseClientHello(bytes));
  }
  assert.deepEqual(normalized.native, evidence.tls.normalized.native);
  assert.deepEqual(normalized.chrome, evidence.tls.normalized.chrome);
  assert.deepEqual(normalized.native, normalized.chrome);
  assert.deepEqual(normalized["native-normal"], normalized["native-upload"]);
  assert.deepEqual(evidence.tls.differences, []);
  assert.deepEqual(evidence.http2.differences, []);
});
