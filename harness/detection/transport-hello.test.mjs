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
