import assert from "node:assert/strict";
import { pngPixels as decodePng } from "../../harness/hotpath/png-pixels.mjs";

export const RECTANGLES = `<!doctype html><style>
  html,body{margin:0}#secret{position:absolute;left:10px;top:10px;width:20px;height:20px;background:red}
  #tall{position:absolute;left:0;top:230px;width:10px;height:10px;background:blue}
</style><div id="secret"></div><div id="tall"></div>`;

export function pngPixels(bytes) {
  assert.equal(bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  const decoded = decodePng(bytes);
  return {
    width: decoded.width,
    height: decoded.height,
    pixel(x, y) {
      assert.ok(x >= 0 && x < decoded.width && y >= 0 && y < decoded.height);
      const rgba = decoded.pixel(x, y);
      return rgba.length === 3 ? [...rgba, 255] : rgba;
    },
  };
}

export function jpegDimensions(bytes) {
  assert.equal(bytes.subarray(0, 2).toString("hex"), "ffd8");
  for (let offset = 2; offset < bytes.length; ) {
    assert.equal(bytes[offset], 255);
    const marker = bytes[offset + 1],
      size = bytes.readUInt16BE(offset + 2);
    if ([0xc0, 0xc1, 0xc2].includes(marker))
      return { width: bytes.readUInt16BE(offset + 7), height: bytes.readUInt16BE(offset + 5) };
    offset += size + 2;
  }
  throw new Error("JPEG dimensions absent");
}
