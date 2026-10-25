// Pure wire parsers for the Chrome identity battery: TLS ClientHello details,
// JA3/JA4, HTTP/2 client frames and the Akamai H2 fingerprint. No I/O here, so
// every helper is covered offline by chrome-identity.test.mjs.
import { createHash } from "node:crypto";
import { isGrease, normalizeHello, parseClientHello } from "./transport-hello.mjs";

const md5 = (text) => createHash("md5").update(text).digest("hex");
const sha12 = (text) => createHash("sha256").update(text).digest("hex").slice(0, 12);
const hex4 = (value) => value.toString(16).padStart(4, "0");

export const TLS_EXTENSION_NAMES = {
  0: "server_name",
  5: "status_request",
  10: "supported_groups",
  11: "ec_point_formats",
  13: "signature_algorithms",
  16: "alpn",
  18: "signed_certificate_timestamp",
  21: "padding",
  23: "extended_master_secret",
  27: "compress_certificate",
  35: "session_ticket",
  41: "pre_shared_key",
  42: "early_data",
  43: "supported_versions",
  45: "psk_key_exchange_modes",
  51: "key_share",
  17513: "application_settings_old",
  17613: "application_settings",
  51764: "trust_anchors",
  65037: "encrypted_client_hello",
  65281: "renegotiation_info",
};

export const TLS_GROUP_NAMES = {
  29: "x25519",
  23: "secp256r1",
  24: "secp384r1",
  25: "secp521r1",
  4588: "X25519MLKEM768",
  25497: "X25519Kyber768Draft00",
};

// Raw extension bodies, keyed by order, so details beyond transport-hello's
// parser (SNI, point formats, ALPS, certificate compression) stay exact.
function extensionBodies(record) {
  let offset = 5 + 4 + 2 + 32;
  offset += 1 + record[offset];
  offset += 2 + record.readUInt16BE(offset);
  offset += 1 + record[offset];
  const end = offset + 2 + record.readUInt16BE(offset);
  offset += 2;
  const bodies = [];
  while (offset + 4 <= end) {
    const type = record.readUInt16BE(offset);
    const length = record.readUInt16BE(offset + 2);
    bodies.push({ type, body: record.subarray(offset + 4, offset + 4 + length) });
    offset += 4 + length;
  }
  return bodies;
}

const u8List = (body) => Array.from(body.subarray(1, 1 + body[0]));
const u16List = (body, start, length) =>
  Array.from({ length: length / 2 }, (_, i) => body.readUInt16BE(start + i * 2));

function protocolList(body) {
  const protocols = [];
  for (let at = 2; at < body.length; ) {
    const length = body[at++];
    protocols.push(body.subarray(at, at + length).toString("latin1"));
    at += length;
  }
  return protocols;
}

/** Every identity-relevant ClientHello field, plus JA3/JA4, from one raw record. */
export function describeClientHello(record) {
  const hello = parseClientHello(record);
  const details = {};
  for (const { type, body } of extensionBodies(record)) {
    if (type === 0) details.serverName = body.subarray(5).toString("latin1");
    if (type === 11) details.pointFormats = u8List(body);
    if (type === 27) details.certificateCompression = u16List(body, 1, body[0]);
    if (type === 45) details.pskModes = u8List(body);
    if (type === 17513 || type === 17613)
      details.applicationSettings = { codepoint: type, protocols: protocolList(body) };
    if (type === 65037) details.echOuterType = body[0];
  }
  const ext = (type) => hello.extensions.find((entry) => entry.type === type);
  const plain = (values = []) => values.filter((value) => !isGrease(value));
  const versions = plain(ext(43)?.values);
  const groups = plain(ext(10)?.values);
  const keyShares = (ext(51)?.keyShares || []).filter((share) => !isGrease(share.group));
  return {
    ...hello,
    ...details,
    extensionOrder: hello.extensions.map((entry) => entry.type),
    extensionNames: hello.extensions
      .filter((entry) => !isGrease(entry.type))
      .map((entry) => TLS_EXTENSION_NAMES[entry.type] || `unknown_${entry.type}`),
    supportedVersions: versions,
    groups,
    groupNames: groups.map((group) => TLS_GROUP_NAMES[group] || `unknown_${group}`),
    keyShareGroups: keyShares.map((share) => share.group),
    signatureAlgorithms: plain(ext(13)?.values),
    alpn: ext(16)?.protocols || [],
    trustAnchors: ext(51764)?.publicRootIds ?? null,
    normalized: normalizeHello(hello),
    ...fingerprints(hello, details),
  };
}

/**
 * JA3 (raw extension order), JA3N (sorted extensions; stable across Chrome's
 * per-connection extension permutation) and JA4 (FoxIO spec, TCP).
 */
export function fingerprints(hello, details = {}) {
  const plain = (values = []) => values.filter((value) => !isGrease(value));
  const ext = (type) => hello.extensions.find((entry) => entry.type === type);
  const ciphers = plain(hello.ciphers);
  const extensions = plain(hello.extensions.map((entry) => entry.type));
  const groups = plain(ext(10)?.values);
  const points = details.pointFormats || [];
  const ja3Of = (order) =>
    [
      hello.legacyVersion,
      ciphers.join("-"),
      order.join("-"),
      groups.join("-"),
      points.join("-"),
    ].join(",");
  const ja3 = ja3Of(extensions);
  const ja3n = ja3Of([...extensions].sort((a, b) => a - b));
  const versions = plain(ext(43)?.values);
  const top = versions.length ? Math.max(...versions) : hello.legacyVersion;
  const version = { 772: "13", 771: "12", 770: "11", 769: "10" }[top] || "00";
  const alpn = ext(16)?.protocols?.[0];
  const alpnCode = alpn ? `${alpn[0]}${alpn.at(-1)}` : "00";
  const count = (value) => String(Math.min(value, 99)).padStart(2, "0");
  const ja4a = `t${version}${extensions.includes(0) ? "d" : "i"}${count(ciphers.length)}${count(extensions.length)}${alpnCode}`;
  const sortedCiphers = [...ciphers].sort((a, b) => a - b).map(hex4);
  const sortedExtensions = extensions
    .filter((type) => type !== 0 && type !== 16)
    .sort((a, b) => a - b)
    .map(hex4);
  const signatures = plain(ext(13)?.values).map(hex4);
  const ja4c = sortedExtensions.join(",") + (signatures.length ? `_${signatures.join(",")}` : "");
  return {
    ja3,
    ja3Hash: md5(ja3),
    ja3n,
    ja3nHash: md5(ja3n),
    ja4: `${ja4a}_${sha12(sortedCiphers.join(","))}_${sha12(ja4c)}`,
    ja4Raw: `${ja4a}_${sortedCiphers.join(",")}_${ja4c}`,
  };
}

export const H2_PREFACE = Buffer.from("PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n", "latin1");
export const H2_FRAME_TYPES = [
  "DATA",
  "HEADERS",
  "PRIORITY",
  "RST_STREAM",
  "SETTINGS",
  "PUSH_PROMISE",
  "PING",
  "GOAWAY",
  "WINDOW_UPDATE",
  "CONTINUATION",
];
export const H2_SETTING_NAMES = {
  1: "HEADER_TABLE_SIZE",
  2: "ENABLE_PUSH",
  3: "MAX_CONCURRENT_STREAMS",
  4: "INITIAL_WINDOW_SIZE",
  5: "MAX_FRAME_SIZE",
  6: "MAX_HEADER_LIST_SIZE",
  8: "ENABLE_CONNECT_PROTOCOL",
  9: "NO_RFC7540_PRIORITIES",
};

function priorityOf(payload, at) {
  const word = payload.readUInt32BE(at);
  return {
    exclusive: Boolean(word & 0x80000000),
    dependency: word & 0x7fffffff,
    weight: payload[at + 4] + 1,
  };
}

function describeFrame(type, flags, streamId, payload) {
  const frame = { type: H2_FRAME_TYPES[type] || `UNKNOWN_${type}`, flags, streamId };
  frame.length = payload.length;
  if (type === 4) {
    if (flags & 1) frame.ack = true;
    frame.settings = [];
    for (let at = 0; at + 6 <= payload.length; at += 6)
      frame.settings.push({ id: payload.readUInt16BE(at), value: payload.readUInt32BE(at + 2) });
  } else if (type === 8 && payload.length >= 4) {
    frame.increment = payload.readUInt32BE(0) & 0x7fffffff;
  } else if (type === 2 && payload.length >= 5) {
    frame.priority = priorityOf(payload, 0);
  } else if (type === 1) {
    frame.endStream = Boolean(flags & 1);
    frame.endHeaders = Boolean(flags & 4);
    const padding = flags & 8 ? 1 : 0;
    if (flags & 0x20 && payload.length >= padding + 5)
      frame.priority = priorityOf(payload, padding);
  } else if (type === 3 && payload.length >= 4) {
    frame.errorCode = payload.readUInt32BE(0);
  } else if (type === 6) {
    if (flags & 1) frame.ack = true;
  }
  return frame;
}

/**
 * Incremental parser for the client→server direction of a cleartext H2 stream.
 * DATA payloads are dropped; every control frame and HEADERS flag is retained.
 */
export class H2FrameTap {
  constructor() {
    this.buffer = Buffer.alloc(0);
    this.prefaceSeen = false;
    this.frames = [];
    this.error = null;
  }
  push(chunk) {
    if (this.error) return;
    this.buffer = Buffer.concat([this.buffer, chunk]);
    if (!this.prefaceSeen) {
      if (this.buffer.length < H2_PREFACE.length) return;
      if (!this.buffer.subarray(0, H2_PREFACE.length).equals(H2_PREFACE)) {
        this.error = "missing HTTP/2 client preface";
        return;
      }
      this.buffer = this.buffer.subarray(H2_PREFACE.length);
      this.prefaceSeen = true;
    }
    while (this.buffer.length >= 9) {
      const length = this.buffer.readUIntBE(0, 3);
      if (this.buffer.length < 9 + length) return;
      const type = this.buffer[3];
      const flags = this.buffer[4];
      const streamId = this.buffer.readUInt32BE(5) & 0x7fffffff;
      const payload = this.buffer.subarray(9, 9 + length);
      // Bound fixture size: keep control frames, never body bytes.
      if (this.frames.length < 256) this.frames.push(describeFrame(type, flags, streamId, payload));
      this.buffer = this.buffer.subarray(9 + length);
    }
  }
}

/** Connection-level H2 identity: the frames a client sends before its first request. */
export function summarizeH2(frames, pseudoOrder = []) {
  const firstHeaders = frames.findIndex((frame) => frame.type === "HEADERS");
  const preamble = firstHeaders < 0 ? frames : frames.slice(0, firstHeaders);
  const settingsFrame = preamble.find((frame) => frame.type === "SETTINGS" && !frame.ack);
  const settings = settingsFrame?.settings || [];
  const windowUpdate = preamble.find(
    (frame) => frame.type === "WINDOW_UPDATE" && frame.streamId === 0,
  );
  const priorities = preamble.filter((frame) => frame.type === "PRIORITY");
  const headers = frames.find((frame) => frame.type === "HEADERS");
  const pseudo = pseudoOrder.map((name) => name.replace(/^:/, "")[0]).join(",");
  return {
    preamble: preamble.map((frame) => frame.type),
    settings: settings.map(({ id, value }) => ({
      id,
      name: H2_SETTING_NAMES[id] || `UNKNOWN_${id}`,
      value,
    })),
    connectionWindowIncrement: windowUpdate?.increment ?? null,
    priorityFrames: priorities.map((frame) => ({ streamId: frame.streamId, ...frame.priority })),
    firstHeadersPriority: headers?.priority || null,
    akamai: [
      settings.map(({ id, value }) => `${id}:${value}`).join(";"),
      windowUpdate ? String(windowUpdate.increment) : "00",
      priorities.length
        ? priorities
            .map(
              (frame) =>
                `${frame.streamId}:${frame.priority.exclusive ? 1 : 0}:${frame.priority.dependency}:${frame.priority.weight}`,
            )
            .join(",")
        : "0",
      pseudo,
    ].join("|"),
  };
}

/** Header list from Node's rawHeaders: order, original casing and values. */
export function headerList(rawHeaders) {
  const list = [];
  for (let index = 0; index + 1 < rawHeaders.length; index += 2)
    list.push([rawHeaders[index], rawHeaders[index + 1]]);
  return list;
}
