//! Admit definitions only after actual matched styles prove them unused.
use lightningcss::properties::{animation::AnimationName, font::FontFamily, Property, PropertyId};
use lightningcss::rules::{font_face::FontFaceProperty, keyframes::KeyframesName, CssRule};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use turbo_html2pdf_core::{ComputedStyle, StyledNode};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Definition {
    Font(String),
    Animation(String),
    Inert,
    Container(String),
    RegisteredColor(String),
    ViewTransition,
}

pub(crate) fn definition(source: &str) -> Result<Definition, String> {
    let sheet = StyleSheet::parse(source, ParserOptions::default())
        .map_err(|_| "native layout resource definition cannot be parsed")?;
    match sheet.rules.0.as_slice() {
        [CssRule::FontFace(rule)] => {
            let has_source = rule.properties.iter().any(|property| matches!(property, FontFaceProperty::Source(values) if !values.is_empty()));
            if !has_source {
                return Ok(Definition::Inert);
            }
            let family = rule
                .properties
                .iter()
                .rev()
                .find_map(|property| match property {
                    FontFaceProperty::FontFamily(family) => Some(family),
                    _ => None,
                });
            Ok(match family {
                Some(family @ FontFamily::FamilyName(_)) => Definition::Font(family_name(family)?),
                _ => Definition::Inert,
            })
        }
        [CssRule::Keyframes(rule)] => Ok(Definition::Animation(match &rule.name {
            KeyframesName::Ident(name) => name.0.to_string(),
            KeyframesName::Custom(name) => name.to_string(),
        })),
        [CssRule::Container(rule)] => rule
            .name
            .as_ref()
            .map(|name| Definition::Container(name.0 .0.to_string()))
            .ok_or_else(|| "native layout unnamed container queries are unsupported".into()),
        [CssRule::Property(rule)]
            if rule
                .syntax
                .to_css_string(PrinterOptions::default())
                .is_ok_and(|syntax| syntax == "\"<color>\"")
                && matches!(
                    rule.initial_value,
                    Some(lightningcss::values::syntax::ParsedComponent::Color(_))
                ) =>
        {
            Ok(Definition::RegisteredColor(rule.name.0.to_string()))
        }
        [CssRule::ViewTransition(rule)]
            if rule.properties.iter().all(|property| {
                matches!(
                    property,
                    lightningcss::rules::view_transition::ViewTransitionProperty::Navigation(_)
                )
            }) =>
        {
            Ok(Definition::ViewTransition)
        }
        _ => Err("native layout resource definition has an unexpected rule kind".into()),
    }
}

fn family_name(family: &FontFamily<'_>) -> Result<String, String> {
    let text = family
        .to_css_string(PrinterOptions::default())
        .map_err(|_| "native layout font family cannot be serialized")?;
    let mut input = cssparser::ParserInput::new(&text);
    let mut input = cssparser::Parser::new(&mut input);
    let mut words = Vec::new();
    while !input.is_exhausted() {
        words.push(
            input
                .expect_ident_or_string()
                .map_err(|_| "native layout font family cannot be decoded")?
                .to_string(),
        );
    }
    Ok(words.join(" "))
}

#[derive(Default)]
struct Definitions {
    fonts: std::collections::HashSet<String>,
    animations: std::collections::HashSet<String>,
    non_ascii_font: bool,
}
impl Definitions {
    fn collect(rules: &[crate::live_stylesheets::ActiveRule]) -> Self {
        let mut result = Self::default();
        for definition in rules.iter().filter_map(|rule| rule.resource.as_ref()) {
            match definition {
                Definition::Font(name) => {
                    result.non_ascii_font |= !name.is_ascii();
                    result.fonts.insert(name.to_ascii_lowercase());
                }
                Definition::Animation(name) => {
                    result.animations.insert(name.clone());
                }
                Definition::Inert
                | Definition::Container(_)
                | Definition::RegisteredColor(_)
                | Definition::ViewTransition => {}
            }
        }
        result
    }
}

type Seen<'a> = std::collections::HashSet<(&'static str, &'a str)>;
pub(crate) fn validate(
    nodes: &[StyledNode],
    rules: &[crate::live_stylesheets::ActiveRule],
) -> Result<(), String> {
    let definitions = Definitions::collect(rules);
    if definitions.fonts.is_empty() && definitions.animations.is_empty() {
        return Ok(());
    }
    let mut seen = Seen::new();
    let mut pending = nodes.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        let StyledNode::Element(element) = node else {
            continue;
        };
        if turbo_html2pdf_core::layout::value::display_of(&element.style)
            == turbo_html2pdf_core::layout::value::Display::None
        {
            continue;
        }
        validate_style(&element.style, &definitions, &mut seen)?;
        pending.extend(element.children.iter());
    }
    Ok(())
}

fn validate_style<'a>(
    style: &'a ComputedStyle,
    definitions: &Definitions,
    seen: &mut Seen<'a>,
) -> Result<(), String> {
    if !definitions.fonts.is_empty() {
        for (name, id) in [
            ("font-family", PropertyId::FontFamily),
            ("font", PropertyId::Font),
        ] {
            let Some(value) = style.get(name).filter(|value| seen.insert((name, value))) else {
                continue;
            };
            let property = Property::parse_string(id, value, ParserOptions::default())
                .map_err(|_| "native layout cannot prove a font definition unused")?;
            let families = match property {
                Property::FontFamily(families) => families,
                Property::Font(font) => font.family,
                _ => return Err("native layout cannot prove a font definition unused".into()),
            };
            for family in families {
                let FontFamily::FamilyName(_) = family else {
                    continue;
                };
                let used = family_name(&family)?;
                if definitions.fonts.contains(&used.to_ascii_lowercase()) {
                    return Err(format!(
                        "native live layout font-face resource is unsupported: {used}"
                    ));
                }
                if !used.is_ascii() || definitions.non_ascii_font {
                    return Err(
                        "native layout cannot prove non-ASCII font name matching unused".into(),
                    );
                }
            }
        }
    }
    if !definitions.animations.is_empty() {
        validate_animations(style, definitions, seen)?;
    }
    Ok(())
}
fn validate_animations<'a>(
    style: &'a ComputedStyle,
    definitions: &Definitions,
    seen: &mut Seen<'a>,
) -> Result<(), String> {
    use lightningcss::vendor_prefix::VendorPrefix;
    for (name, id) in [
        (
            "animation-name",
            PropertyId::AnimationName(VendorPrefix::None),
        ),
        ("animation", PropertyId::Animation(VendorPrefix::None)),
        (
            "-webkit-animation-name",
            PropertyId::AnimationName(VendorPrefix::WebKit),
        ),
        (
            "-webkit-animation",
            PropertyId::Animation(VendorPrefix::WebKit),
        ),
    ] {
        let Some(value) = style.get(name).filter(|value| seen.insert((name, value))) else {
            continue;
        };
        let property = Property::parse_string(id, value, ParserOptions::default())
            .map_err(|_| "native layout cannot prove an animation definition unused")?;
        let names = match property {
            Property::AnimationName(names, _) => names.into_iter().collect::<Vec<_>>(),
            Property::Animation(animations, _) => animations
                .into_iter()
                .map(|animation| animation.name)
                .collect(),
            _ => return Err("native layout cannot prove an animation definition unused".into()),
        };
        for name in names {
            let used = match name {
                AnimationName::None => continue,
                AnimationName::Ident(name) => name.0.to_string(),
                AnimationName::String(name) => name.0.to_string(),
            };
            if definitions.animations.contains(&used) {
                return Err(format!(
                    "native live layout animation resource is unsupported: {used}"
                ));
            }
        }
    }
    Ok(())
}
