//! Standardized CSS `zoom` (Chrome156). Every absolute length an element uses
//! is multiplied by its effective zoom: the product of computed zoom factors
//! along the flat tree. Layout therefore runs in zoomed pixels, while CSSOM
//! offset/client/scroll metrics convert back through the element's own zoom.
use turbo_html2pdf_core::{ComputedStyle, StyledNode};

/// The computed zoom factor of one element: invalid, CSS-wide and zero values
/// compute to 1 (an `inherit` was already replaced by the parent's value).
pub(crate) fn factor(style: &ComputedStyle) -> f32 {
    style
        .get("zoom")
        .and_then(turbo_dom_browser_env::css_values::zoom_value)
        .map(|(_, factor)| factor)
        .filter(|factor| *factor > 0.0 && factor.is_finite())
        .unwrap_or(1.0)
}

/// Scale each zoomed element's used lengths in place, in flat-tree order.
pub(crate) fn apply(nodes: &mut [StyledNode]) {
    walk(nodes, 1.0);
}

fn walk(nodes: &mut [StyledNode], parent: f32) {
    for node in nodes {
        let StyledNode::Element(element) = node else {
            continue;
        };
        let effective = parent * factor(&element.style);
        if effective != 1.0 {
            element.style = scaled(&element.style, effective);
        }
        walk(&mut element.children, effective);
    }
}

// The native layout default font size; an element without a cascaded size
// otherwise lays out text at the unzoomed default.
const DEFAULT_FONT_PX: f32 = 16.0;

fn scaled(style: &ComputedStyle, zoom: f32) -> ComputedStyle {
    let mut pairs: Vec<(String, String)> = style
        .iter()
        .map(|(name, value)| {
            let value = if scalable(name) {
                scale_value(value, zoom, border_width(name))
            } else {
                value.to_owned()
            };
            (name.to_owned(), value)
        })
        .collect();
    if style.get("font-size").is_none() {
        pairs.push(("font-size".into(), format!("{}px", DEFAULT_FONT_PX * zoom)));
    }
    ComputedStyle::from_pairs(pairs)
}

fn scalable(name: &str) -> bool {
    !name.starts_with("--")
        && !name.starts_with("animation")
        && !name.starts_with("transition")
        && !matches!(
            name,
            "zoom"
                | "font-family"
                | "content"
                | "quotes"
                | "font-feature-settings"
                | "font-variation-settings"
                | "grid-template-areas"
                | "counter-reset"
                | "counter-increment"
                | "counter-set"
                | "will-change"
        )
}

// Border/outline width keywords are absolute lengths that zoom like any other.
fn border_width(name: &str) -> bool {
    let border = name.starts_with("border")
        && !["radius", "style", "color", "image", "spacing", "collapse"]
            .iter()
            .any(|part| name.contains(part));
    border
        || matches!(
            name,
            "outline" | "outline-width" | "column-rule" | "column-rule-width"
        )
}

// Absolute and viewport-relative units; font- and percentage-relative values
// already follow the zoomed font size or containing block.
const ZOOMED_UNITS: &[&str] = &[
    "px", "pt", "pc", "in", "cm", "mm", "q", "rem", "vw", "vh", "vi", "vb", "vmin", "vmax", "svw",
    "svh", "svi", "svb", "svmin", "svmax", "lvw", "lvh", "lvi", "lvb", "lvmin", "lvmax", "dvw",
    "dvh", "dvi", "dvb", "dvmin", "dvmax",
];

fn ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte >= 0x80
}

fn number_start(bytes: &[u8], index: usize) -> bool {
    let digit = |offset: usize| bytes.get(index + offset).is_some_and(u8::is_ascii_digit);
    let dot_digit = |offset: usize| bytes.get(index + offset) == Some(&b'.') && digit(offset + 1);
    match bytes[index] {
        b'0'..=b'9' => true,
        b'.' => digit(1),
        b'+' | b'-' => digit(1) || dot_digit(1),
        _ => false,
    }
}

fn number_end(bytes: &[u8], mut index: usize) -> usize {
    if matches!(bytes[index], b'+' | b'-') {
        index += 1;
    }
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(index + 1), Some(b'+' | b'-')));
        if bytes.get(index + 1 + sign).is_some_and(u8::is_ascii_digit) {
            index += 1 + sign;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
    }
    index
}

fn skip_string(bytes: &[u8], mut index: usize) -> usize {
    let quote = bytes[index];
    index += 1;
    while index < bytes.len() && bytes[index] != quote {
        index += if bytes[index] == b'\\' { 2 } else { 1 };
    }
    (index + 1).min(bytes.len())
}

fn skip_ident(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && ident_char(bytes[index]) {
        index += 1;
    }
    index
}

/// Multiply every zoomable dimension token, leaving strings, URLs, hashes,
/// identifiers and unitless numbers untouched.
pub(crate) fn scale_value(value: &str, zoom: f32, keywords: bool) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let end = if byte == b'"' || byte == b'\'' {
            skip_string(bytes, index)
        } else if byte == b'#' {
            skip_ident(bytes, index + 1)
        } else if number_start(bytes, index) {
            let number = number_end(bytes, index);
            let unit = if bytes.get(number) == Some(&b'%') {
                number + 1
            } else {
                skip_ident(bytes, number)
            };
            let suffix = &value[number..unit];
            let zoomed = ZOOMED_UNITS
                .iter()
                .any(|known| suffix.eq_ignore_ascii_case(known));
            match value[index..number].parse::<f32>() {
                Ok(amount) if zoomed => {
                    out.push_str(&format!("{}{suffix}", amount * zoom));
                    index = unit;
                    continue;
                }
                _ => unit,
            }
        } else if ident_char(byte) && byte != b'-' || byte == b'-' && !number_start(bytes, index) {
            let end = skip_ident(bytes, index + 1);
            let word = &value[index..end];
            if word.eq_ignore_ascii_case("url")
                && bytes.get(end) == Some(&b'(')
                && !value[end..].starts_with("(\"")
                && !value[end..].starts_with("('")
            {
                value[end..]
                    .find(')')
                    .map_or(bytes.len(), |close| end + close + 1)
            } else {
                let width = ["thin", "medium", "thick"]
                    .iter()
                    .position(|keyword| word.eq_ignore_ascii_case(keyword))
                    .filter(|_| keywords);
                if let Some(width) = width {
                    out.push_str(&format!("{}px", [1.0f32, 3.0, 5.0][width] * zoom));
                    index = end;
                    continue;
                }
                end
            }
        } else {
            index + value[index..].chars().next().map_or(1, char::len_utf8)
        };
        out.push_str(&value[index..end]);
        index = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_tokens_scale_and_everything_else_is_preserved() {
        for (value, expected) in [
            ("10px", "20px"),
            ("calc(100% - 10px)", "calc(100% - 20px)"),
            ("1.5em 2rem -3px +4px .5px", "1.5em 4rem -6px 8px 1px"),
            ("10vw 1e1px 2E+1PX", "20vw 20px 40PX"),
            (
                "translate3d(1px, 2px, 0) rotate(45deg)",
                "translate3d(2px, 4px, 0) rotate(45deg)",
            ),
            (
                "url(a10px.png) 'x 10px' \"5px\"",
                "url(a10px.png) 'x 10px' \"5px\"",
            ),
            ("#100 #1e1e1e h1px", "#100 #1e1e1e h1px"),
            ("1 50% 3s 2fr", "1 50% 3s 2fr"),
            ("ünïcode 2px", "ünïcode 4px"),
        ] {
            assert_eq!(scale_value(value, 2.0, false), expected, "{value}");
        }
        assert_eq!(scale_value("medium solid thin", 2.0, true), "6px solid 2px");
        assert_eq!(scale_value("medium solid", 2.0, false), "medium solid");
        assert_eq!(scale_value("thick", 0.5, true), "2.5px");
    }

    #[test]
    fn factors_follow_the_computed_value_and_effective_zoom_multiplies() {
        let style = |zoom: &str| ComputedStyle::from_pairs([("zoom", zoom)]);
        for (value, expected) in [
            ("2", 2.0),
            ("50%", 0.5),
            ("normal", 1.0),
            ("0", 1.0),
            ("initial", 1.0),
            ("abc", 1.0),
            ("calc(1 + 1)", 2.0),
        ] {
            assert_eq!(factor(&style(value)), expected, "{value}");
        }
        assert_eq!(factor(&ComputedStyle::default()), 1.0);
        let child = StyledNode::Element(turbo_html2pdf_core::StyledElement {
            tag: turbo_html2pdf_core::Tag::Html("div".into()),
            attrs: Vec::new(),
            style: ComputedStyle::from_pairs([("zoom", "3"), ("width", "10px")]),
            children: Vec::new(),
        });
        let mut nodes = vec![StyledNode::Element(turbo_html2pdf_core::StyledElement {
            tag: turbo_html2pdf_core::Tag::Html("div".into()),
            attrs: Vec::new(),
            style: ComputedStyle::from_pairs([
                ("zoom", "2"),
                ("font-family", "a10px"),
                ("--gap", "1px"),
            ]),
            children: vec![child, StyledNode::Text("10px".into())],
        })];
        apply(&mut nodes);
        let parent = nodes[0].as_element().unwrap();
        assert_eq!(parent.style.get("font-size"), Some("32px"));
        assert_eq!(parent.style.get("font-family"), Some("a10px"));
        assert_eq!(parent.style.get("--gap"), Some("1px"));
        assert_eq!(parent.style.get("zoom"), Some("2"));
        let child = parent.children[0].as_element().unwrap();
        assert_eq!(child.style.get("width"), Some("60px"));
        assert_eq!(child.style.get("font-size"), Some("96px"));
    }
}
