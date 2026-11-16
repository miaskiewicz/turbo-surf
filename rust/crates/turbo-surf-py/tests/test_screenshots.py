"""Public Python raster surface; offline and runnable without pytest machinery."""

import base64
import struct
import xml.etree.ElementTree as ET

import turbo_surf as ts

PUBLIC_RASTER = (
    "screenshot", "screenshot_svg", "screenshot_with_css", "screenshot_svg_with_css",
    "screenshot_with_assets", "screenshot_svg_with_assets", "stylesheet_hrefs", "image_urls",
)
RED = "<body style='margin:0;background:red'>"
IMAGE = "<body style='margin:0'><img src='/image.png' width='3' height='2' style='display:block'>"


def png_size(png):
    assert isinstance(png, bytes)
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    return struct.unpack(">II", png[16:24])


def svg_size(svg):
    assert isinstance(svg, str)
    root = ET.fromstring(svg)
    assert root.tag == "{http://www.w3.org/2000/svg}svg"
    return float(root.attrib["width"]), float(root.attrib["height"])


def test_raster_surface_is_public():
    for name in PUBLIC_RASTER:
        assert name in ts.__all__, name
        assert callable(getattr(ts, name)), name


def test_public_png_renders_css_and_supplied_assets():
    red = ts.screenshot(RED, width=3, height=2)
    blue = ts.screenshot_with_css("<body style='margin:0'>", "body{background:blue}", width=3, height=2)
    assert png_size(red) == png_size(blue) == (3, 2)
    assert red != blue
    supplied = ts.screenshot_with_assets(IMAGE, "", {"/image.png": red}, width=3, height=2, full_page=False)
    missing = ts.screenshot_with_assets(IMAGE, "", {}, width=3, height=2, full_page=False)
    assert png_size(supplied) == (3, 2)
    assert supplied == red
    assert supplied != missing


def test_public_svg_renders_css_and_supplied_assets():
    red = ts.screenshot_svg(RED, width=3, height=2)
    blue = ts.screenshot_svg_with_css("<body style='margin:0'>", "body{background:blue}", width=3, height=2)
    assert svg_size(red) == svg_size(blue) == (3, 2)
    assert red != blue
    png = ts.screenshot(RED, width=3, height=2)
    supplied = ts.screenshot_svg_with_assets(IMAGE, "", {"/image.png": png}, width=3, height=2, full_page=False)
    assert svg_size(supplied) == (3, 2)
    images = list(ET.fromstring(supplied).iter("{http://www.w3.org/2000/svg}image"))
    assert len(images) == 1
    uri = images[0].attrib["href"]
    assert uri.startswith("data:image/png;base64,")
    assert png_size(base64.b64decode(uri.split(",", 1)[1], validate=True)) == (3, 2)


def test_public_discovery_and_full_page_capture():
    assert ts.stylesheet_hrefs("<link rel='stylesheet' href='/style.css'>") == ["/style.css"]
    assert ts.image_urls(IMAGE) == ["/image.png"]
    tall = "<body style='margin:0'><div style='height:7px;background:red'></div>"
    png = ts.screenshot_with_assets(tall, "", {}, width=3, height=2, full_page=True)
    svg = ts.screenshot_svg_with_assets(tall, "", {}, width=3, height=2, full_page=True)
    assert png_size(png) == svg_size(svg) == (3, 7)


if __name__ == "__main__":
    tests = (
        test_raster_surface_is_public,
        test_public_png_renders_css_and_supplied_assets,
        test_public_svg_renders_css_and_supplied_assets,
        test_public_discovery_and_full_page_capture,
    )
    for test in tests:
        test()
        print(test.__name__ + ": passed")
    print("4 passed; 0 skipped")
