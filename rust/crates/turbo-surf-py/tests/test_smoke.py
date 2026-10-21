"""Smoke tests for the turbo-surf PyO3 binding — offline, no network."""

import json
from importlib.metadata import version

import turbo_surf as ts

HTML = """<!doctype html>
<html><head><title>  Hello  </title></head>
<body>
  <h1>Heading</h1>
  <a href="/about">About</a>
  <span class="price">9.99</span>
  <span class="price">19.99</span>
</body></html>"""


def test_version():
    assert ts.version() == version("turbo-surf")


def test_title_trimmed():
    assert ts.title(HTML) == "Hello"


def test_text_has_heading():
    assert "Heading" in ts.text(HTML)


def test_links_resolved():
    assert "https://example.com/about" in ts.links(HTML, base_url="https://example.com/")


def test_markdown_nonempty():
    assert "Heading" in ts.markdown(HTML, base_url="https://example.com/")


def test_query_returns_json():
    out = json.loads(ts.query(HTML, ".price"))
    assert isinstance(out, list)
    assert len(out) == 2


def test_extract_schema():
    schema = json.dumps(
        {
            "title": {"selector": "h1"},
            "prices": {"selector": ".price", "list": True},
        }
    )
    out = json.loads(ts.extract(HTML, schema, base_url="https://example.com/"))
    assert out["title"] == "Heading"
    assert out["prices"] == ["9.99", "19.99"]


def test_extract_bad_schema_raises():
    try:
        ts.extract(HTML, "{not json")
    except ts.TurboSurfError:
        return
    raise AssertionError("expected TurboSurfError on malformed schema JSON")


def test_evaluate_runs_js():
    out = ts.evaluate("<div id=x>hi</div>", "document.getElementById('x').textContent")
    assert out == "hi"


def test_render_hydrates():
    html = "<body><div id=root></div></body>"
    script = "document.getElementById('root').textContent = 'mounted';"
    out = ts.render(html, script, base_url="https://example.com/")
    assert "mounted" in out
