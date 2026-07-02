//! Recover author CSS from a raw HTML snapshot.
//!
//! The layout engine's html5ever pass keeps only `<body>` children, so any
//! `<style>` in `<head>` is dropped before it can cascade. We scan the raw
//! source for `<style>…</style>` blocks and hand their concatenated text back as
//! author CSS. `<link rel="stylesheet">` is intentionally *not* followed — that
//! needs a network fetch, and a screenshot renders the snapshot as given.

/// Elements whose *text content* must never be painted: the layout engine would
/// otherwise flow their raw source (JS, CSS, fallbacks) as visible body text.
const NON_VISUAL_TAGS: [&str; 4] = ["script", "style", "noscript", "template"];

/// Strip every `<script>`/`<style>`/`<noscript>`/`<template>` element (tag +
/// body) from raw HTML so their source never renders as text. Call *after*
/// [`collect_style_blocks`] so `<style>` CSS is still cascaded. Case-insensitive;
/// tolerates attributes on the opening tag. Unclosed tags are left as-is.
pub fn strip_non_visual(html: &str) -> String {
    let mut out = html.to_string();
    for tag in NON_VISUAL_TAGS {
        out = strip_tag_blocks(&out, tag);
    }
    out
}

fn strip_tag_blocks(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find(&open) {
        let tag_start = cursor + rel;
        // Guard against a longer tag name (`<style` vs `<styled`): the char after
        // the name must end the name (whitespace, `>`, or self-close `/`).
        let after = bytes.get(tag_start + open.len()).copied();
        if !matches!(after, Some(b) if b == b'>' || b == b'/' || b.is_ascii_whitespace()) {
            out.push_str(&html[cursor..tag_start + open.len()]);
            cursor = tag_start + open.len();
            continue;
        }
        out.push_str(&html[cursor..tag_start]);
        // Drop through the matching close tag's `>` (or to EOF if unclosed).
        match lower[tag_start..].find(&close) {
            Some(crel) => {
                let close_start = tag_start + crel;
                let end = lower[close_start..]
                    .find('>')
                    .map(|g| close_start + g + 1)
                    .unwrap_or(html.len());
                cursor = end;
            }
            None => {
                cursor = html.len();
                break;
            }
        }
    }
    out.push_str(&html[cursor..]);
    out
}

/// The `href`s of every `<link rel="stylesheet">` in `html`, in source order.
/// Values are returned verbatim (possibly relative) — the caller resolves them
/// against the page URL and fetches them (the raster itself does no I/O). `rel`
/// is matched case-insensitively and may carry other tokens (`stylesheet
/// preload`). Alternate stylesheets (`rel="alternate stylesheet"`) are skipped.
pub fn stylesheet_hrefs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut hrefs = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("<link") {
        let tag_start = cursor + rel;
        let end = lower[tag_start..]
            .find('>')
            .map(|g| tag_start + g)
            .unwrap_or(html.len());
        let tag = &html[tag_start..end];
        let tag_lower = &lower[tag_start..end];
        let rel_val = attr_value(tag, tag_lower, "rel")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let is_sheet = rel_val.split_whitespace().any(|t| t == "stylesheet")
            && !rel_val.split_whitespace().any(|t| t == "alternate");
        if is_sheet {
            if let Some(href) = attr_value(tag, tag_lower, "href") {
                if !href.trim().is_empty() {
                    hrefs.push(href.trim().to_string());
                }
            }
        }
        cursor = end + 1;
    }
    hrefs
}

/// Every image reference the layout engine can paint, in source order and
/// de-duplicated: `<img src>` values and `background-image: url(...)` urls. Values
/// are returned **verbatim** — exactly the resolver key turbo-html2pdf uses (the
/// raw `src`, or the unquoted `url(...)` inner) — so the caller resolves each
/// against the page URL, fetches the bytes, and stores them under the same key.
/// `data:` URIs are skipped (self-contained, nothing to fetch).
pub fn image_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    img_srcs(html, &mut urls);
    background_image_urls(html, &mut urls);
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| !u.starts_with("data:") && seen.insert(u.clone()));
    urls
}

/// Push the `src` of every `<img>` tag onto `out`.
fn img_srcs(html: &str, out: &mut Vec<String>) {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("<img") {
        let tag_start = cursor + rel;
        // Whole tag name: char after `<img` must end it (ws / `>` / `/`).
        let after = bytes.get(tag_start + 4).copied();
        if !matches!(after, Some(b) if b == b'>' || b == b'/' || b.is_ascii_whitespace()) {
            cursor = tag_start + 4;
            continue;
        }
        let end = lower[tag_start..]
            .find('>')
            .map(|g| tag_start + g)
            .unwrap_or(html.len());
        if let Some(src) = attr_value(&html[tag_start..end], &lower[tag_start..end], "src") {
            if !src.trim().is_empty() {
                out.push(src.trim().to_string());
            }
        }
        cursor = end + 1;
    }
}

/// Push the url of every `background`/`background-image: url(...)` declaration
/// onto `out` (inline `style=` attrs and `<style>` blocks alike — a raw source
/// scan). Both the longhand and the `background:` shorthand carry image urls;
/// real stylesheets use the shorthand pervasively.
fn background_image_urls(html: &str, out: &mut Vec<String>) {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    // `background` also prefixes `background-image`, so this matches both.
    while let Some(rel) = lower[cursor..].find("background") {
        let decl = cursor + rel + "background".len();
        cursor = decl;
        // Only a `url(` inside THIS declaration (before its terminator) counts —
        // a later declaration's url must not be attributed to a `background`
        // property that had none (`background: #fff`).
        // `;`/`}` end a CSS declaration; `<`/newline bound an inline `style=`
        // attr's value. Quotes are NOT terminators — they may wrap the url itself
        // (`url("x.png")`).
        let end = lower[decl..]
            .find([';', '}', '<', '\n'])
            .map(|e| decl + e)
            .unwrap_or(html.len());
        let Some(urel) = lower[decl..end].find("url(") else {
            continue;
        };
        let inner_start = decl + urel + "url(".len();
        let Some(close) = html[inner_start..end].find(')') else {
            continue;
        };
        let name = html[inner_start..inner_start + close]
            .trim()
            .trim_matches(['"', '\''])
            .trim();
        if !name.is_empty() {
            out.push(name.to_string());
        }
        cursor = inner_start + close + 1;
    }
}

/// Read `name="value"` (or `name='value'`) out of an opening-tag slice. `tag` is
/// the original-case text; `tag_lower` its lowercase twin (for case-insensitive
/// attribute-name matching while returning the original-case value).
fn attr_value(tag: &str, tag_lower: &str, name: &str) -> Option<String> {
    let mut from = 0;
    loop {
        let rel = tag_lower[from..].find(name)?;
        let at = from + rel;
        // Must be a whole attribute name: preceded by whitespace/`<`, followed by `=`.
        let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let after = tag_lower[at + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let rest = tag[at + name.len()..].trim_start();
            let rest = rest.strip_prefix('=')?.trim_start();
            let (quote, body) = match rest.chars().next()? {
                q @ ('"' | '\'') => (q, &rest[1..]),
                _ => return rest.split_whitespace().next().map(str::to_string),
            };
            return body.find(quote).map(|e| body[..e].to_string());
        }
        from = at + name.len();
    }
}

/// Concatenate the text of every `<style>` element in `html`, in source order.
/// Attributes on the opening tag (e.g. `type`, `media`) are skipped; only the
/// element body is returned.
pub fn collect_style_blocks(html: &str) -> String {
    let mut out = String::new();
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("<style") {
        let tag_start = cursor + rel;
        // Confirm it's the `<style` element, not `<styled-x>` — next char must be
        // whitespace or the tag close.
        let after = bytes.get(tag_start + 6).copied();
        if !matches!(after, Some(b) if b == b'>' || b.is_ascii_whitespace()) {
            cursor = tag_start + 6;
            continue;
        }
        // Body starts after the opening tag's `>`.
        let Some(gt) = lower[tag_start..].find('>') else {
            break;
        };
        let body_start = tag_start + gt + 1;
        let Some(end_rel) = lower[body_start..].find("</style") else {
            break;
        };
        out.push_str(&html[body_start..body_start + end_rel]);
        out.push('\n');
        cursor = body_start + end_rel + "</style".len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::collect_style_blocks;

    #[test]
    fn pulls_head_and_body_styles_in_order() {
        let html = r#"<html><head>
            <style type="text/css">.a{color:red}</style>
          </head><body>
            <style>.b{color:blue}</style>
            <div>hi</div>
          </body></html>"#;
        let css = collect_style_blocks(html);
        assert!(css.contains(".a{color:red}"));
        assert!(css.contains(".b{color:blue}"));
        assert!(
            css.find(".a").unwrap() < css.find(".b").unwrap(),
            "source order"
        );
    }

    #[test]
    fn ignores_similar_tag_names() {
        // `<styled>` must not be mistaken for `<style>`.
        let css = collect_style_blocks("<styled>nope</styled><style>.x{}</style>");
        assert!(css.contains(".x{}"));
        assert!(!css.contains("nope"));
    }

    #[test]
    fn empty_when_no_styles() {
        assert_eq!(collect_style_blocks("<div>plain</div>"), "");
    }

    #[test]
    fn strip_non_visual_removes_script_and_style_source() {
        use super::strip_non_visual;
        let html = r#"<div>hi</div>
            <script>var x = new Granim({a:1});</script>
            <style>.a{color:red}</style>
            <noscript>enable js</noscript>
            <p>bye</p>"#;
        let out = strip_non_visual(html);
        assert!(out.contains("<div>hi</div>"));
        assert!(out.contains("<p>bye</p>"));
        assert!(!out.contains("Granim"), "script source must be gone");
        assert!(!out.contains("color:red"), "style source must be gone");
        assert!(!out.contains("enable js"), "noscript must be gone");
    }

    #[test]
    fn stylesheet_hrefs_extracts_rel_stylesheet() {
        use super::stylesheet_hrefs;
        let html = r#"<head>
            <link rel="stylesheet" href="/a.css">
            <link href='https://cdn.example/b.css' rel="stylesheet preload">
            <link rel="icon" href="/favicon.ico">
            <link rel="alternate stylesheet" href="/dark.css">
            <link rel=stylesheet href=bare.css>
          </head>"#;
        let hrefs = stylesheet_hrefs(html);
        assert_eq!(
            hrefs,
            vec!["/a.css", "https://cdn.example/b.css", "bare.css"]
        );
    }

    #[test]
    fn image_urls_extracts_img_and_background_skipping_data() {
        use super::image_urls;
        let html = r#"<img src="/a.png">
            <div style="background-image:url('b.jpg')"></div>
            <style>.x{ background-image: url(c.png) }</style>
            <div style="background:#fff url(d.webp) no-repeat"></div>
            <style>.y{ background: url("e.svg") center }</style>
            <div style="background:#000"></div>
            <img src="data:image/png;base64,AAAA">
            <img src="/a.png">"#;
        // `<img>` srcs first (source order), then background/-image urls (longhand
        // + shorthand, quoted or bare). A colour-only `background` and `data:` are
        // skipped; duplicates removed.
        assert_eq!(
            image_urls(html),
            vec!["/a.png", "b.jpg", "c.png", "d.webp", "e.svg"]
        );
    }

    #[test]
    fn strip_non_visual_spares_similar_tags_and_unclosed() {
        // `<scripting>` is not `<script>`; an unclosed `<style` is left intact.
        let out = super::strip_non_visual("<scripting>keep</scripting><b>x</b>");
        assert!(out.contains("keep") && out.contains("<b>x</b>"));
    }
}
