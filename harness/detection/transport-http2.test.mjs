import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeHttp2 } from "./transport-http2.mjs";

test("H2 comparison retains wire header order while omitting the ephemeral authority value", () => {
  const raw = {
    alpn: "h2",
    settings: { headerTableSize: 65536 },
    connectionWindow: 15728640,
    rawHeaders: [
      ":method",
      "GET",
      ":authority",
      "127.0.0.1:4321",
      ":scheme",
      "https",
      ":path",
      "/",
      "accept",
      "text/html",
      "accept-language",
      "en-US,en;q=0.9",
    ],
  };
  const result = normalizeHttp2(raw);
  assert.deepEqual(result.headerOrder, [
    ":method",
    ":authority",
    ":scheme",
    ":path",
    "accept",
    "accept-language",
  ]);
  assert.equal(result.headers[":authority"], undefined);
  assert.equal(result.headers["accept-language"], "en-US,en;q=0.9");
  assert.deepEqual(result.settings, raw.settings);
  assert.equal(result.connectionWindow, 15728640);
});
