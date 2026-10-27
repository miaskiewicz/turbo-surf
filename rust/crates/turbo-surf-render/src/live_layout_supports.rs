//! Evaluate native conditional declarations with the same CSS grammar as CSSOM.
use lightningcss::properties::{
    custom::{CustomPropertyName, TokenList, TokenOrValue},
    Property, PropertyId,
};
use lightningcss::rules::supports::SupportsCondition;
use lightningcss::stylesheet::ParserOptions;
use lightningcss::traits::Parse;

pub(crate) fn matches(source: &str) -> Result<bool, String> {
    let condition = SupportsCondition::parse_string(source)
        .map_err(|_| "native layout received an invalid supports condition")?;
    evaluate(&condition)
}

fn evaluate(condition: &SupportsCondition<'_>) -> Result<bool, String> {
    match condition {
        SupportsCondition::Not(value) => Ok(!evaluate(value)?),
        SupportsCondition::And(values) => {
            for value in values {
                if !evaluate(value)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        SupportsCondition::Or(values) => {
            for value in values {
                if evaluate(value)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        SupportsCondition::Declaration { property_id, value } => {
            // Lightning CSS has no typed `zoom`; use the canonical CSSOM grammar.
            if property_id.name().eq_ignore_ascii_case("zoom") {
                return Ok(zoom(value));
            }
            if property_id.prefix().intersects(
                lightningcss::vendor_prefix::VendorPrefix::Moz
                    | lightningcss::vendor_prefix::VendorPrefix::Ms
                    | lightningcss::vendor_prefix::VendorPrefix::O,
            ) || matches!(
                property_id,
                PropertyId::Custom(CustomPropertyName::Unknown(_))
            ) {
                return Ok(false);
            }
            Ok(
                Property::parse_string(property_id.clone(), value, ParserOptions::default())
                    .is_ok_and(|property| match property {
                        Property::Unparsed(unparsed) => {
                            css_wide(value) || deferred(&unparsed.value)
                        }
                        _ => true,
                    }),
            )
        }
        SupportsCondition::Unknown(_) => Ok(false),
        SupportsCondition::Selector(source) => Ok(selector(source)),
    }
}

fn zoom(value: &str) -> bool {
    if turbo_dom_browser_env::css_values::zoom_value(value).is_some() || css_wide(value) {
        return true;
    }
    Property::parse_string(PropertyId::from("zoom"), value, ParserOptions::default()).is_ok_and(
        |property| match property {
            Property::Custom(property) => deferred(&property.value),
            Property::Unparsed(property) => deferred(&property.value),
            _ => false,
        },
    )
}

#[deno_core::op2(fast)]
pub(crate) fn op_native_css_supports(
    #[string] source: &str,
    #[string] value: &str,
    declaration: bool,
) -> bool {
    if declaration {
        property(source, value)
    } else {
        matches(source)
            .or_else(|_| matches(&format!("({source})")))
            .unwrap_or(false)
    }
}

fn property(name: &str, value: &str) -> bool {
    if name.trim() != name {
        return false;
    }
    let mut input = cssparser::ParserInput::new(name);
    let mut parser = cssparser::Parser::new(&mut input);
    let Ok(name) = parser.expect_ident_cloned() else {
        return false;
    };
    if parser.expect_exhausted().is_err() {
        return false;
    }
    let name = if name.starts_with("--") {
        name.to_string()
    } else {
        name.to_ascii_lowercase()
    };
    let id = PropertyId::from(lightningcss::values::string::CowArcStr::from(name));
    evaluate(&SupportsCondition::Declaration {
        property_id: id,
        value: value.into(),
    })
    .unwrap_or(false)
}

/// `selector(…)`: supported when turbo-dom's selector engine (the one the DOM APIs
/// and every native cascade use) parses it as one unforgiving complex selector, as
/// Chrome does (`harness/detection/fixtures/css-supports-chrome156.json`).
fn selector(source: &str) -> bool {
    turbo_dom_parser::rtdom::query::parse_for_supports(source).is_ok()
}

fn css_wide(value: &str) -> bool {
    lightningcss::properties::CSSWideKeyword::parse_string(value).is_ok()
}

fn deferred(tokens: &TokenList<'_>) -> bool {
    tokens.0.iter().any(|token| match token {
        TokenOrValue::Var(_) | TokenOrValue::Env(_) => true,
        TokenOrValue::Function(function) => deferred(&function.arguments),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::{matches, property};
    #[test]
    fn css_supports_two_argument_form_and_zoom_follow_chrome() {
        // CSS.supports(name, value): the name must be exactly one identifier.
        assert!(property("width", "1px"));
        assert!(!property(" width", "1px"));
        assert!(!property("width height", "1px"));
        assert!(!property("1px", "1px"));
        // zoom: numbers/percentages, CSS-wide keywords and var() are supported.
        assert!(property("zoom", "1.5"));
        assert!(property("zoom", "inherit"));
        assert!(property("zoom", "var(--z)"));
        assert!(!property("zoom", "big"));
        assert_eq!(matches("(zoom: 50%)"), Ok(true));
        assert_eq!(matches("(zoom: var(--z))"), Ok(true));
        assert_eq!(matches("(zoom: wide)"), Ok(false));
        // An `or` with no supported branch is false.
        assert_eq!(matches("(unknown: 1) or (width: nonsense)"), Ok(false));
    }
    #[test]
    fn native_declarations_validate_values_and_nested_boolean_grammar() {
        for source in [
            "(display: grid)",
            "(width: 2px)",
            "(width: var(--w))",
            "(width: inherit)",
            "(width: /*comment*/inherit)",
            "(--token: 3)",
            "not (unknown: 1)",
            "(display: grid) and (not (width: nonsense))",
            "(unknown: 1) or (display: flex)",
        ] {
            assert_eq!(matches(source), Ok(true), "{source}");
        }
        for source in [
            "(width: nonsense)",
            "(width: nonsense inherit)",
            "(unknown: 1)",
            "(width: 2px) and (height: nonsense)",
            "future(1)",
        ] {
            assert_eq!(matches(source), Ok(false), "{source}");
        }
        assert!(matches("(width: 2px) and").is_err());
        assert_eq!(matches("selector(div > .x)"), Ok(true));
        assert_eq!(matches("selector(:bogus)"), Ok(false));
        // Changed@S4 (Chrome 156 control css-supports-chrome156.json): the any-namespace
        // type selector, escapes and Chrome's other valid selectors are supported;
        // forgiving lists are evaluated unforgivingly.
        for source in [
            "selector(*|p)",
            r"selector(.\31 23)",
            r"selector(\64iv > .x)",
            "selector(:hover)",
            "selector(::before)",
        ] {
            assert_eq!(matches(source), Ok(true), "{source}");
        }
        for source in [
            "selector(ns|p)",
            "selector(:is(:bogus))",
            "selector(div, span)",
        ] {
            assert_eq!(matches(source), Ok(false), "{source}");
        }
    }
}
