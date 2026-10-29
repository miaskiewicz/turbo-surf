#!/usr/bin/env python3
"""Writes tests/fixtures/image_formats_cases.json for tests/image_formats.rs (turbo-surf#30).

Every image is built here from first principles (PNG/BMP/ICO/CUR by hand, AVIF/JPEG/GIF/
WebP/TIFF by ImageMagick 7 with the HEIC/AVIF delegate, JPEG XL pinned below), so the cases
need no copied fixtures. `data` cases are `<img>` sources (base64, percent-encoded and raw `data:` URLs,
correct and wrong MIME types, truncated bodies); `http` cases are served by the test's
local server (Playwright routes in the Chrome collector) with the given status and
Content-Type. Regenerate, then recapture the control (html-image-formats-chrome.mjs):
  python3 rust/crates/turbo-surf-render/tests/fixture-gen/make-image-format-cases.py
ImageMagick output is not byte-stable across versions; the committed JSON is the pin.
"""
import base64
import html
import json
import os
import struct
import subprocess
import tempfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "fixtures", "image_formats_cases.json")


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def png(width, height, rgba=(255, 0, 0, 255)):
    rows = b"".join(b"\x00" + bytes(rgba) * width for _ in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows))
            + chunk(b"IEND", b""))


def bmp24(width, height, bgr=(0, 0, 255)):
    row = bytes(bgr) * width
    row += b"\0" * ((4 - len(row) % 4) % 4)
    pixels = row * height
    header = b"BM" + struct.pack("<IHHI", 54 + len(pixels), 0, 0, 54)
    info = struct.pack("<IiiHHIIiiII", 40, width, height, 1, 24, 0, len(pixels), 2835, 2835, 0, 0)
    return header + info + pixels


def icon_bmp(width, height):
    """An ICO/CUR BMP entry: 32 bpp XOR image (doubled height) plus the AND mask."""
    xor = bytes((0, 255, 0, 255)) * width * height
    mask_row = b"\0" * (((width + 31) // 32) * 4)
    info = struct.pack("<IiiHHIIiiII", 40, width, height * 2, 1, 32, 0,
                       len(xor) + len(mask_row) * height, 0, 0, 0, 0)
    return info + xor + mask_row * height


def icon(entries, kind=1):
    out = struct.pack("<HHH", 0, kind, len(entries))
    offset, body = 6 + 16 * len(entries), b""
    for width, height, data in entries:
        planes, bits = (1, 32) if kind == 1 else (width // 2, height // 2)  # CUR: hotspot
        out += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, planes, bits, len(data),
                           offset + len(body))
        body += data
    return out + body


def magick(args, extension):
    with tempfile.TemporaryDirectory() as directory:
        path = os.path.join(directory, "out." + extension)
        subprocess.run(["magick", *args, path], check=True)
        with open(path, "rb") as file:
            return file.read()


def b64(mime, data):
    return "data:%s;base64,%s" % (mime, base64.b64encode(data).decode())


def marker(data, *codes):
    return min(data.index(code) for code in codes if code in data)


RED = png(3, 2)
SVG = b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'></svg>"
JPEG = magick(["-size", "5x4", "xc:#ff0000"], "jpg")
GIF = magick(["-size", "6x5", "xc:#ff0000"], "gif")
WEBP = magick(["-size", "7x6", "xc:#ff0000"], "webp")
SOF = marker(JPEG, b"\xff\xc0", b"\xff\xc1", b"\xff\xc2")
SOS = marker(JPEG, b"\xff\xda")
NS = "xmlns='http://www.w3.org/2000/svg'"
# JPEG XL (turbo-surf#43): ImageMagick here has no JPEG XL coder, so the two images are
# zune-jpegxl 0.4.0 lossless encodes (`JxlSimpleEncoder`, 8-bit) of a solid 64x40 #ff0000
# RGB and a solid 40x64 rgba(0,0,255,128) image, pinned as bytes (193 and 200 bytes).
JXL = base64.b64decode(
    "/wo4AX6ASAgCAQDQAksYm5xxhAM4gAM4IErAOQUBACBEgAgQASJAhP/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r"
    "7u7uhv/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r7u7uhv/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r7u7u"
    "hv/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r7u7uPgDHBwD43/B5+AMA/G/4XH0AgP8NHw==")
JXL_ALPHA = base64.b64decode(
    "/wr4AU6AlQgIEADsAksYm5xxhAM4gAM4IErAOQUBACBEgAgQASJAhP/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r"
    "7u7uhv/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r7u7uhv/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r7u7u"
    "hv/37/nvoTHnnGvtc2+SJAkBVVVVVVXV////c+/r7u7uPgDHBwD43/B59AMA/G/4XH0AgP8NnwcAAMD/hg8=")

DATA = [
    ("png", b64("image/png", RED)),
    ("png-wrong-mime", b64("text/plain", RED)),
    ("png-svg-mime", b64("image/svg+xml", RED)),
    ("png-corrupt-idat", b64("image/png", RED[:41] + b"\x00garbage-not-zlib" + RED[-12:])),
    ("png-idat-truncated", b64("image/png", RED[:45])),
    ("png-no-iend", b64("image/png", RED[:-12])),
    ("png-ihdr-only", b64("image/png", RED[:33])),
    ("png-ihdr-iend", b64("image/png", RED[:33] + chunk(b"IEND", b""))),
    ("png-bad-ihdr-crc", b64("image/png", RED[:29] + b"\0\0\0\0" + RED[33:])),
    ("png-signature-only", b64("image/png", RED[:8])),
    ("jpeg", b64("image/jpeg", JPEG)),
    ("jpeg-sos-header", b64("image/jpeg", JPEG[:SOS + 14])),
    ("jpeg-no-eoi", b64("image/jpeg", JPEG[:-2])),
    ("jpeg-sof-only", b64("image/jpeg", JPEG[:SOF + 19])),
    ("jpeg-before-sos", b64("image/jpeg", JPEG[:SOS])),
    ("gif", b64("image/gif", GIF)),
    ("gif-no-trailer", b64("image/gif", GIF[:-1])),
    ("gif-screen-only", b64("image/gif", GIF[:13])),
    ("webp", b64("image/webp", WEBP)),
    ("webp-header", b64("image/webp", WEBP[:30])),
    ("bmp", b64("image/bmp", bmp24(64, 40))),
    ("bmp-header-only", b64("image/bmp", bmp24(64, 40)[:60])),
    ("ico-bmp-sizes", b64("image/x-icon", icon([(16, 16, icon_bmp(16, 16)), (48, 48, icon_bmp(48, 48))]))),
    ("ico-png", b64("image/x-icon", icon([(32, 32, png(32, 32))]))),
    ("cur", b64("image/x-icon", icon([(24, 24, icon_bmp(24, 24))], kind=2))),
    ("avif", b64("image/avif", magick(["-size", "64x40", "xc:#ff0000", "-depth", "8"], "avif"))),
    ("avif-alpha", b64("image/avif", magick(["-size", "40x64", "xc:rgba(0,0,255,0.5)", "-depth", "8"], "avif"))),
    ("jxl", b64("image/jxl", JXL)),
    ("jxl-alpha", b64("image/jxl", JXL_ALPHA)),
    ("tiff", b64("image/tiff", magick(["-size", "30x20", "xc:#00ff00"], "tiff"))),
    ("svg-base64", b64("image/svg+xml", SVG)),
    ("svg-percent-encoded", "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2250%22%20height%3D%2220%22%3E%3C%2Fsvg%3E"),
    ("svg-raw-utf8", "data:image/svg+xml;utf8,<svg %s width='12' height='34'><circle cx='6' cy='6' r='5' fill='%%23f00'/></svg>" % NS),
    ("svg-viewbox-only", "data:image/svg+xml,<svg %s viewBox='0 0 20 10'></svg>" % NS),
    ("svg-width-only", "data:image/svg+xml,<svg %s width='90' viewBox='0 0 30 10'></svg>" % NS),
    ("svg-fractional", "data:image/svg+xml,<svg %s width='10.6' height='7.4px'></svg>" % NS),
    ("svg-sizeless", "data:image/svg+xml,<svg %s></svg>" % NS),
    ("svg-percent-width", "data:image/svg+xml,<svg %s width='50%%' height='20'></svg>" % NS),
    ("svg-text-plain", "data:text/plain,<svg %s width='40' height='30'></svg>" % NS),
    ("svg-default-mime", "data:,<svg %s width='40' height='30'></svg>" % NS),
    ("svg-text-xml", "data:text/xml,<svg %s width='40' height='30'></svg>" % NS),
    ("svg-malformed", "data:image/svg+xml,<svg %s width='40' height='30'><g></svg>" % NS),
    ("svg-no-namespace", "data:image/svg+xml,<svg width='40' height='30'></svg>"),
    ("base64-whitespace", "data:image/png;base64," + " ".join(
        base64.b64encode(RED).decode()[i:i + 8] for i in range(0, len(base64.b64encode(RED)), 8))),
    ("base64-unpadded", "data:image/png;base64," + base64.b64encode(RED).decode().rstrip("=")),
    ("base64-percent-escaped", "data:image/png;base64," + base64.b64encode(RED).decode()
     .replace("+", "%2B").replace("/", "%2F")),
    ("base64-invalid", "data:image/png;base64,iVBOR!!!"),
    ("base64-upper-case-mime", "DATA:IMAGE/PNG;BASE64," + base64.b64encode(RED).decode()),
    ("empty", "data:image/png;base64,"),
]

HTTP = [
    ("png-as-html", 200, "text/html", RED),
    ("png-untyped", 200, "", RED),
    ("png-404", 404, "image/png", RED),
    ("png-500", 500, "image/png", RED),
    ("svg-typed", 200, "image/svg+xml", SVG),
    ("svg-typed-charset", 200, "image/svg+xml; charset=utf-8", SVG),
    ("svg-as-plain", 200, "text/plain", SVG),
    ("svg-as-xml", 200, "application/xml", SVG),
    ("bmp-as-octets", 200, "application/octet-stream", bmp24(8, 6)),
]

# The page: one parser <img> per data: case, and the http case ids for the probe.
PAGE = "<!doctype html><body>%s<script type='application/json' id='http-cases'>%s</script>" % (
    "".join('<img data-id="%s" src="%s">' % (name, html.escape(src)) for name, src in DATA),
    json.dumps([name for name, *_ in HTTP]),
)

cases = {
    "html": PAGE,
    "data": [{"id": name, "src": src} for name, src in DATA],
    "http": [{"id": name, "status": status, "contentType": kind, "body": base64.b64encode(body).decode()}
             for name, status, kind, body in HTTP],
}
with open(OUT, "w") as file:
    json.dump(cases, file, indent=1)
    file.write("\n")

# The screenshot page (turbo-surf-mcp tests/screenshot_image_formats.rs): the decodable
# BMP/ICO/CUR/AVIF/JPEG XL cases as absolutely positioned <img>s over a cyan canvas, painted
# by turbo-dom-raster with the features surf ships (BMP/ICO always, AVIF under `avif`, JPEG
# XL under `jxl`).
SHOT = os.path.join(HERE, "..", "..", "..", "turbo-surf-mcp", "tests", "fixtures",
                    "screenshot_image_formats.html")
SHOT_IDS = ["bmp", "ico-bmp-sizes", "ico-png", "cur", "avif", "avif-alpha", "jxl", "jxl-alpha"]
os.makedirs(os.path.dirname(SHOT), exist_ok=True)
with open(SHOT, "w") as file:
    file.write("<!doctype html><html><head><style>body{margin:0;background:#00ffff}"
               "img{position:absolute;top:10px}</style></head><body>")
    sources = dict(DATA)
    for index, name in enumerate(SHOT_IDS):
        file.write('<img data-id="%s" style="left:%dpx" src="%s">'
                   % (name, 10 + index * 80, html.escape(sources[name])))
    file.write("</body></html>\n")
