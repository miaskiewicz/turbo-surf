//! CSS Syntax parsing with selector/property validation, preserving authored values.
use cssparser::{
    parse_important, AtRuleParser, CowRcStr, DeclarationParser, Delimiter, ParseError, Parser,
    ParserInput, ParserState, QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
    StyleSheetParser,
};
use lightningcss::media_query::MediaList;
use lightningcss::properties::{
    custom::{Token, TokenList, TokenOrValue},
    Property, PropertyId,
};
use lightningcss::rules::{font_face::FontFaceProperty, CssRule};
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::targets::{Features, Targets};
use lightningcss::traits::ToCss;

pub fn is_supported_property(name: &str) -> bool {
    !matches!(PropertyId::from(name), PropertyId::Custom(_))
        || matches!(
            name,
            "content"
                | "float"
                | "clear"
                | "direction"
                | "unicode-bidi"
                | "table-layout"
                | "caption-side"
                | "empty-cells"
                | "cursor"
                | "user-select"
                | "-webkit-user-select"
                | "vertical-align"
                | "z-index"
                | "visibility"
                | "pointer-events"
                | "touch-action"
                | "will-change"
                | "appearance"
                | "-webkit-appearance"
        )
}

fn deferred_value(tokens: &TokenList<'_>) -> bool {
    tokens.0.iter().any(|token| match token {
        TokenOrValue::Var(_) | TokenOrValue::Env(_) => true,
        TokenOrValue::Function(function) => deferred_value(&function.arguments),
        TokenOrValue::Token(Token::Ident(name)) => matches!(
            name.as_ref(),
            "inherit" | "initial" | "unset" | "revert" | "revert-layer"
        ),
        _ => false,
    })
}

pub(super) fn valid_selector(source: &str) -> bool {
    valid_rule(&format!("{source} {{ color: red }}"))
}

fn valid_rule(source: &str) -> bool {
    StyleSheet::parse(source, ParserOptions::default()).is_ok_and(|sheet| {
        !sheet.rules.0.is_empty()
            && !sheet
                .rules
                .0
                .iter()
                .any(|rule| matches!(rule, CssRule::Unknown(_)))
    })
}

// cssparser supplies tokenization, block boundaries and error recovery. Lightning
// CSS supplies selector/property grammar; its printer would rewrite blue to #00f,
// so CSSOM keeps validated declaration lexemes rather than minifier serialization.
#[derive(Default)]
struct CssParser {
    keyframes: bool,
    font_face: bool,
}

fn consume(input: &mut Parser<'_, '_>) -> String {
    let start = input.position();
    while input.next_including_whitespace_and_comments().is_ok() {}
    input.slice_from(start).trim().to_string()
}

impl<'i> QualifiedRuleParser<'i> for CssParser {
    type Prelude = String;
    type QualifiedRule = String;
    type Error = ();
    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<String, ParseError<'i, ()>> {
        let prelude = consume(input);
        let test = if self.keyframes {
            format!("@keyframes test {{ {prelude} {{}} }}")
        } else {
            format!("{prelude} {{}}")
        };
        if !valid_rule(&test) {
            return Err(input.new_custom_error(()));
        }
        Ok(prelude)
    }
    fn parse_block<'t>(
        &mut self,
        prelude: String,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<String, ParseError<'i, ()>> {
        let body = RuleBodyParser::new(input, self)
            .filter_map(Result::ok)
            .collect::<Vec<_>>()
            .join(" ");
        Ok(format!("{prelude} {{ {body} }}"))
    }
}

// Font descriptors have their own grammar: ordinary property validation drops
// `src` and wrongly admits layout properties inside @font-face.
fn font_descriptor(name: &str, value: &str) -> Option<String> {
    let source = format!("@font-face {{ {name}: {value}; }}");
    let sheet = StyleSheet::parse(&source, ParserOptions::default()).ok()?;
    let CssRule::FontFace(rule) = sheet.rules.0.first()? else {
        return None;
    };
    let descriptor = rule.properties.first()?;
    if !matches!(descriptor, FontFaceProperty::Custom(_)) {
        return if matches!(descriptor, FontFaceProperty::Source(_)) {
            descriptor
                .to_css_string(PrinterOptions::default())
                .ok()
                .map(|text| format!("{text};"))
        } else {
            Some(format!("{name}: {value};"))
        };
    }
    let mut source = ParserInput::new(value);
    let mut input = Parser::new(&mut source);
    let valid = match name {
        "font-display" => input.expect_ident().is_ok_and(|value| {
            matches!(
                value.as_ref(),
                "auto" | "block" | "swap" | "fallback" | "optional"
            )
        }),
        "size-adjust" | "ascent-override" | "descent-override" | "line-gap-override" => {
            (name != "size-adjust"
                && input
                    .try_parse(|input| input.expect_ident_matching("normal"))
                    .is_ok())
                || input.expect_percentage().is_ok_and(|value| value >= 0.0)
        }
        "font-feature-settings" | "font-variation-settings" => Property::parse(
            PropertyId::from(name),
            &mut input,
            &ParserOptions::default(),
        )
        .is_ok_and(|property| !matches!(property, Property::Unparsed(_) | Property::Custom(_))),
        _ => false,
    };
    (valid && input.expect_exhausted().is_ok()).then(|| format!("{name}: {value};"))
}

impl<'i> DeclarationParser<'i> for CssParser {
    type Declaration = String;
    type Error = ();
    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<String, ParseError<'i, ()>> {
        let start = input.position();
        if self.font_face {
            let value = consume(input);
            return font_descriptor(&name.to_ascii_lowercase(), &value)
                .ok_or_else(|| input.new_custom_error(()));
        }
        let valid = input.parse_until_before(Delimiter::Bang, |input| {
            let property=Property::parse(
                PropertyId::from(lightningcss::values::string::CowArcStr::from(name.to_string())),
                input,
                &ParserOptions::default(),
            )
            .map_err(|_| input.new_custom_error(()))?;
            Ok(!matches!(&property, Property::Unparsed(property) if !deferred_value(&property.value)))
        })?;
        if !valid {
            return Err(input.new_custom_error(()));
        }
        let value = input.slice_from(start).trim().to_string();
        let important = input.try_parse(parse_important).is_ok();
        input.expect_exhausted()?;
        let name = if name.starts_with("--") {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        };
        Ok(format!(
            "{name}: {value}{};",
            if important { " !important" } else { "" }
        ))
    }
}

impl<'i> AtRuleParser<'i> for CssParser {
    type Prelude = (String, String);
    type AtRule = String;
    type Error = ();
    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, ()>> {
        Ok((name.to_ascii_lowercase(), consume(input)))
    }
    fn rule_without_block(
        &mut self,
        (name, prelude): Self::Prelude,
        _: &ParserState,
    ) -> Result<String, ()> {
        let rule = format!("@{name} {prelude};");
        if valid_rule(&rule) {
            Ok(rule)
        } else {
            Err(())
        }
    }
    fn parse_block<'t>(
        &mut self,
        (name, prelude): Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<String, ParseError<'i, ()>> {
        let test = format!("@{name} {prelude} {{}}");
        if !valid_rule(&test) {
            return Err(input.new_custom_error(()));
        }
        let body = if matches!(
            name.as_str(),
            "font-face"
                | "page"
                | "property"
                | "counter-style"
                | "font-palette-values"
                | "viewport"
                | "position-try"
                | "view-transition"
        ) {
            let mut declarations = CssParser {
                font_face: name == "font-face",
                ..Default::default()
            };
            RuleBodyParser::new(input, &mut declarations)
                .filter_map(Result::ok)
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            let mut nested = CssParser {
                keyframes: name == "keyframes" || name.ends_with("-keyframes"),
                ..Default::default()
            };
            StyleSheetParser::new(input, &mut nested)
                .filter_map(Result::ok)
                .collect::<Vec<_>>()
                .join(" ")
        };
        Ok(format!("@{name} {prelude} {{ {body} }}"))
    }
}

impl<'i> RuleBodyItemParser<'i, String, ()> for CssParser {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        !self.keyframes
    }
}

pub fn parse(source: &str) -> String {
    let mut source = ParserInput::new(source);
    let mut input = Parser::new(&mut source);
    StyleSheetParser::new(&mut input, &mut CssParser::default())
        .filter_map(Result::ok)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Serialize a query using the media grammar rather than stylesheet prelude text.
pub fn parse_media(source: &str) -> String {
    // The typed AST normalizes min/max features into ranges. Preserve the common
    // authored legacy form instead of introducing a new range syntax in CSSOM.
    let legacy =
        (source.contains("min-") || source.contains("max-")) && !source.contains(['<', '>']);
    let options = PrinterOptions {
        targets: Targets {
            include: if legacy {
                Features::MediaRangeSyntax
            } else {
                Features::empty()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let mut source = ParserInput::new(source);
    let mut input = Parser::new(&mut source);
    MediaList::parse(&mut input, &ParserOptions::default())
        .ok()
        .filter(|_| input.expect_exhausted().is_ok())
        .and_then(|media| media.to_css_string(options).ok())
        .unwrap_or_else(|| "not all".to_string())
}

/// Parse a single rule without dropping invalid neighboring rules. Declaration
/// recovery remains CSSOM-compatible inside a syntactically valid rule.
pub fn parse_one(source: &str) -> Option<String> {
    let mut source = ParserInput::new(source);
    let mut input = Parser::new(&mut source);
    let mut parser = CssParser::default();
    let mut rules = StyleSheetParser::new(&mut input, &mut parser);
    let rule = rules.next()?.ok()?;
    if rules.next().is_some() {
        return None;
    }
    Some(rule)
}

#[cfg(test)]
mod tests {
    use super::{parse, parse_media, parse_one};

    #[test]
    fn recovers_bad_selectors_and_declarations_without_losing_valid_neighbors() {
        let parsed = parse("a { color: red; width: garbage; } ??? { color: blue; } sentinel {}");
        assert!(parsed.contains("color: red"), "{parsed}");
        assert!(!parsed.contains("garbage"), "{parsed}");
        assert!(!parsed.contains("???"), "{parsed}");
        assert!(parsed.contains("sentinel"), "{parsed}");
    }

    #[test]
    fn strings_comments_custom_properties_and_grouping_use_css_grammar() {
        let parsed = parse(
            r#"/* { */ @media screen { a[data-x="}"] { content: "a;b}"; --token: url("a;b"); } }"#,
        );
        assert!(parsed.contains("@media screen"), "{parsed}");
        assert!(parsed.contains("a[data-x="), "{parsed}");
        assert!(parsed.contains("a;b}"), "{parsed}");
        assert!(parsed.contains("--token"), "{parsed}");
    }
    #[test]
    fn single_rule_parsing_does_not_recover_invalid_neighbors() {
        assert!(parse_one("??? {} a {}").is_none());
        assert!(parse_one("a {} b {}").is_none());
        assert!(parse_one("a { width: nonsense; color: blue }")
            .unwrap()
            .contains("color: blue"));
    }

    #[test]
    fn media_queries_use_their_typed_grammar() {
        assert_eq!(parse_media("SCREEN"), "screen");
        assert_eq!(
            parse_media("screen and (min-width:10px)"),
            "screen and (min-width: 10px)"
        );
        assert_eq!(parse_media("???"), "not all");
    }
}
