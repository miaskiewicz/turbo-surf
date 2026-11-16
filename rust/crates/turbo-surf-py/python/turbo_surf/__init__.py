"""turbo-surf — browserless, native-speed web crawler + extractor (PyO3 binding).

Pass a page's HTML in, get a view out — Markdown, visible text, links, a typed
extraction, an accessibility tree — or, for JS-gated pages, the hydrated HTML
after the page's own scripts run in a real V8 isolate (no headless browser).

    import turbo_surf as ts

    html = open("page.html").read()
    print(ts.markdown(html, base_url="https://example.com/"))
    print(ts.links(html, base_url="https://example.com/"))

    # Typed extraction: a schema maps field names to selector specs.
    schema = '{"title": {"selector": "h1"}, "prices": {"selector": ".price", "list": true}}'
    print(ts.extract(html, schema, base_url="https://example.com/"))

    # JS-gated page: run its own scripts and read the hydrated DOM.
    hydrated = ts.render(html, script, base_url="https://example.com/")

Fatal faults (malformed schema JSON, a render-tier failure) raise
:class:`TurboSurfError`; the non-JS views never raise.
"""

from ._turbo_surf import (  # noqa: F401
    TurboSurfError,
    accessibility_tree,
    detect,
    evaluate,
    extract,
    html,
    hydration_state,
    image_urls,
    interactive_elements,
    links,
    markdown,
    query,
    render,
    screenshot,
    screenshot_svg,
    screenshot_svg_with_assets,
    screenshot_svg_with_css,
    screenshot_with_assets,
    screenshot_with_css,
    stylesheet_hrefs,

    text,
    title,
    transform,
    version,
)

__all__ = [
    "TurboSurfError",
    "accessibility_tree",
    "detect",
    "evaluate",
    "extract",
    "html",
    "hydration_state",
    "image_urls",
    "interactive_elements",
    "links",
    "markdown",
    "query",
    "render",
    "screenshot",
    "screenshot_svg",
    "screenshot_svg_with_assets",
    "screenshot_svg_with_css",
    "screenshot_with_assets",
    "screenshot_with_css",
    "stylesheet_hrefs",

    "text",
    "title",
    "transform",
    "version",
]
