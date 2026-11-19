//! Lower CSS nesting only for the native layout snapshot; the page's CSSOM stays intact.
use crate::live_stylesheets::RuleSnapshot;
use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::targets::{Features, Targets};

pub(super) fn flatten(rules: Vec<RuleSnapshot>) -> Result<Vec<RuleSnapshot>, String> {
    let mut result = Vec::with_capacity(rules.len());
    for rule in rules {
        // Nested blocks need another opening brace. Quoted braces may cause an
        // extra parse, but ordinary flat rules keep their single parsing pass.
        if rule.kind != 1 || rule.css_text.match_indices('{').nth(1).is_none() {
            result.push(rule);
            continue;
        }
        let Ok(parsed) = StyleSheet::parse(&rule.css_text, ParserOptions::default()) else {
            result.push(rule);
            continue;
        };
        let nested = matches!(parsed.rules.0.as_slice(), [CssRule::Style(style)] if !style.rules.0.is_empty());
        if !nested {
            drop(parsed);
            result.push(rule);
            continue;
        }
        let lowered = parsed
            .to_css(PrinterOptions {
                targets: Targets {
                    include: Features::Nesting,
                    ..Default::default()
                },
                ..Default::default()
            })
            .map_err(|error| format!("native CSS nesting serialization failed: {error:?}"))?
            .code;
        let lowered = StyleSheet::parse(&lowered, ParserOptions::default())
            .map_err(|error| format!("native lowered CSS parse failed: {error:?}"))?;
        for rule in &lowered.rules.0 {
            result.push(snapshot(rule)?);
        }
    }
    Ok(result)
}

fn snapshot(rule: &CssRule<'_>) -> Result<RuleSnapshot, String> {
    use lightningcss::traits::ToCss;
    let css_text = rule
        .to_css_string(PrinterOptions::default())
        .map_err(|error| format!("native lowered CSS serialization failed: {error:?}"))?;
    let (kind, children) = match rule {
        CssRule::Style(style) if style.rules.0.is_empty() => (1, None),
        CssRule::Media(group) => (4, Some(&group.rules.0)),
        CssRule::Supports(group) => (12, Some(&group.rules.0)),
        CssRule::Container(group) => (0, Some(&group.rules.0)),
        _ => return Err("native CSS nesting produced an unsupported rule".into()),
    };
    let rules = children
        .map(|rules| rules.iter().map(snapshot).collect::<Result<Vec<_>, _>>())
        .transpose()?;
    let prelude = if rules.is_some() {
        Some(prelude(&css_text)?)
    } else {
        None
    };
    Ok(RuleSnapshot {
        kind,
        css_text,
        prelude,
        rules,
        media: String::new(),
        style_sheet: None,
        scoped: None,
    })
}

fn prelude(source: &str) -> Result<String, String> {
    let mut input = cssparser::ParserInput::new(source);
    let mut input = cssparser::Parser::new(&mut input);
    let result: Result<String, cssparser::ParseError<'_, ()>> =
        input.parse_until_before(cssparser::Delimiter::CurlyBracketBlock, |input| {
            let start = input.position();
            while input.next_including_whitespace_and_comments().is_ok() {}
            Ok(input.slice_from(start).trim().to_owned())
        });
    result.map_err(|_| "native lowered CSS prelude parsing failed".into())
}
