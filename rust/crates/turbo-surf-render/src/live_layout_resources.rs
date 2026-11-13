//! Validate resource use against matched native styles before geometry.
use lightningcss::properties::{animation::AnimationName, font::FontFamily, Property, PropertyId};
use lightningcss::rules::{font_face::FontFaceProperty, keyframes::KeyframesName, CssRule};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use turbo_dom_layout::{ComputedStyle, StyledNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryKind {
    LocalBox,
    ScrollExtent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Query {
    pub(crate) source: turbo_dom_parser::rtdom::tree::Handle,
    pub(crate) kind: QueryKind,
}

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
        // Opacity does not change layout boxes, even while its animation is active.
        [CssRule::Keyframes(rule)]
            if rule.keyframes.iter().all(|frame| {
                frame
                    .declarations
                    .declarations
                    .iter()
                    .chain(frame.declarations.important_declarations.iter())
                    .all(|property| matches!(property, Property::Opacity(_)))
            }) =>
        {
            Ok(Definition::Inert)
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

pub(crate) fn family_name(family: &FontFamily<'_>) -> Result<String, String> {
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
    animations: std::collections::HashMap<String, Vec<Option<AnimationValues>>>,
}

thread_local! {
    /// `@font-face` families (lowercased) a layout found in use; the font loader
    /// takes them after the layout to start those faces' loads, as Chrome does.
    static USED_FONTS: std::cell::RefCell<std::collections::BTreeSet<String>> =
        std::cell::RefCell::default();
}

/// The `@font-face` families layouts used since the last call.
pub(crate) fn take_used_fonts() -> Vec<String> {
    USED_FONTS.with(|fonts| {
        std::mem::take(&mut *fonts.borrow_mut())
            .into_iter()
            .collect()
    })
}
impl Definitions {
    fn collect(rules: &[crate::live_stylesheets::ActiveRule]) -> Self {
        let mut result = Self::default();
        for rule in rules {
            let Some(definition) = rule.resource.as_ref() else {
                continue;
            };
            match definition {
                Definition::Font(name) => {
                    result.fonts.insert(name.to_lowercase());
                }
                Definition::Animation(name) => {
                    result
                        .animations
                        .entry(name.clone())
                        .or_default()
                        .push(animation_values(&rule.text));
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

// A constant animation is geometry-safe only for the element whose underlying
// longhand values were compared; the same animation name can change another node.
#[derive(Default)]
struct AnimationValues {
    paint_or_transform_only: bool,
    properties: Vec<(String, String)>,
}

fn animation_values(source: &str) -> Option<AnimationValues> {
    let sheet = StyleSheet::parse(source, ParserOptions::default()).ok()?;
    let [CssRule::Keyframes(rule)] = sheet.rules.0.as_slice() else {
        return None;
    };
    let mut result = AnimationValues {
        paint_or_transform_only: true,
        ..AnimationValues::default()
    };
    for frame in &rule.keyframes {
        // Important declarations do not apply in CSS keyframes.
        for property in &frame.declarations.declarations {
            if !matches!(property, Property::Opacity(_)) {
                // Typed vendor aliases share transform dependencies. Stroke dash
                // paint also leaves another element's CSS border box unchanged.
                result.paint_or_transform_only &= matches!(
                    property,
                    Property::Transform(_, _)
                        | Property::StrokeDasharray(_)
                        | Property::StrokeDashoffset(_)
                );
                result.properties.push(comparable_value(property)?);
            }
        }
    }
    Some(result)
}

fn comparable_value(property: &Property<'_>) -> Option<(String, String)> {
    if matches!(
        property,
        Property::Unparsed(_) | Property::Custom(_) | Property::All(_)
    ) {
        return None;
    }
    let id = property.property_id();
    if id.longhands().is_some() {
        return None;
    }
    let value = property
        .value_to_css_string(PrinterOptions::default())
        .ok()?;
    if ["inherit", "initial", "unset", "revert", "revert-layer"].contains(&value.as_str()) {
        return None;
    }
    Some((id.to_css_string(PrinterOptions::default()).ok()?, value))
}

fn constant_animation(style: &ComputedStyle, frames: &[Option<AnimationValues>]) -> bool {
    if style
        .get("animation-composition")
        .is_some_and(|value| value.split(',').any(|part| part.trim() != "replace"))
    {
        return false;
    }
    frames.iter().all(|frame| {
        frame.as_ref().is_some_and(|frame| {
            frame
                .properties
                .iter()
                .all(|(name, expected)| matches_underlying(style, name, expected))
        })
    })
}

fn paint_or_transform_only(style: &ComputedStyle, frames: &[Option<AnimationValues>]) -> bool {
    !style
        .get("animation-composition")
        .is_some_and(|value| value.split(',').any(|part| part.trim() != "replace"))
        && frames.iter().all(|frame| {
            frame
                .as_ref()
                .is_some_and(|frame| !frame.properties.is_empty() && frame.paint_or_transform_only)
        })
}

fn matches_underlying(style: &ComputedStyle, name: &str, expected: &str) -> bool {
    let Some(value) = style.get(name) else {
        return false;
    };
    Property::parse_string(PropertyId::from(name), value, ParserOptions::default())
        .ok()
        .and_then(|property| comparable_value(&property))
        .is_some_and(|(_, actual)| actual == expected)
}

// Visual rects exclude descendants' overflow. An animation of transforms
// or stroke-dash paint outside the requested ancestor chain can be ignored only
// when no scroll-container dependency can feed it back into layout or the
// requested scroll translation.
fn independent_effect(
    path: &[&turbo_dom_layout::StyledElement],
    requested: &[&turbo_dom_layout::StyledElement],
    kind: QueryKind,
) -> bool {
    let Some(element) = path.last() else {
        return false;
    };
    if requested
        .iter()
        .any(|ancestor| ancestor.source == element.source)
    {
        return false;
    }
    if kind == QueryKind::ScrollExtent
        && requested
            .last()
            .is_some_and(|target| path.iter().any(|ancestor| ancestor.source == target.source))
    {
        return false;
    }
    // With overlay scrollbars and zero retained offsets, clipping/scrolling
    // branches cannot allocate a gutter or translate another element's box.
    // Scroll extents additionally depend on every animated descendant above.
    path.iter().chain(requested).all(|ancestor| {
        overflow_axes(&ancestor.style).is_some()
            && ancestor
                .style
                .get("scrollbar-gutter")
                .is_none_or(|value| value.eq_ignore_ascii_case("auto"))
    })
}

fn overflow_axes(style: &ComputedStyle) -> Option<[&'static str; 2]> {
    for (name, id) in [
        ("overflow", PropertyId::Overflow),
        ("overflow-x", PropertyId::OverflowX),
        ("overflow-y", PropertyId::OverflowY),
    ] {
        if let Some(value) = style.get(name) {
            let property = Property::parse_string(id, value, ParserOptions::default()).ok()?;
            if !matches!(
                property,
                Property::Overflow(_) | Property::OverflowX(_) | Property::OverflowY(_)
            ) {
                return None;
            }
        }
    }
    // The native CSSOM View resolver pairs visible with hidden/scroll as auto;
    // looking only at authored axis tokens would incorrectly admit that case.
    Some([
        turbo_dom_layout::cssom_view::scroll::overflow(style, "overflow-x"),
        turbo_dom_layout::cssom_view::scroll::overflow(style, "overflow-y"),
    ])
}

fn element_path<'a>(
    nodes: &'a [StyledNode],
    source: turbo_dom_parser::rtdom::tree::Handle,
    path: &mut Vec<&'a turbo_dom_layout::StyledElement>,
) -> bool {
    for node in nodes {
        let StyledNode::Element(element) = node else {
            continue;
        };
        path.push(element);
        if element.source == Some(source) || element_path(&element.children, source, path) {
            return true;
        }
        path.pop();
    }
    false
}

pub(crate) type CompletedAnimations =
    std::collections::HashMap<turbo_dom_parser::rtdom::tree::Handle, Vec<String>>;

pub(crate) fn validate_with_lifecycle(
    nodes: &[StyledNode],
    rules: &[crate::live_stylesheets::ActiveRule],
    requested: Option<Query>,
    completed: &CompletedAnimations,
) -> Result<bool, String> {
    let definitions = Definitions::collect(rules);
    // Only a full strict success permits sharing this layout across query targets.
    let strict = validate_nodes(nodes, &definitions, completed);
    if strict.is_ok() {
        return Ok(true);
    }
    if definitions.animations.is_empty() {
        return strict.map(|()| true);
    }
    let Some(query) = requested else {
        return strict.map(|()| true);
    };
    let mut path = Vec::new();
    if !element_path(nodes, query.source, &mut path) {
        return strict.map(|()| true);
    }
    validate_rect_nodes(
        nodes,
        &definitions,
        &path,
        query.kind,
        &mut Vec::new(),
        &mut Seen::new(),
        completed,
    )?;
    Ok(false)
}

fn validate_rect_nodes<'a>(
    nodes: &'a [StyledNode],
    definitions: &Definitions,
    requested: &[&turbo_dom_layout::StyledElement],
    kind: QueryKind,
    path: &mut Vec<&'a turbo_dom_layout::StyledElement>,
    seen: &mut Seen<'a>,
    completed: &CompletedAnimations,
) -> Result<(), String> {
    for node in nodes {
        let StyledNode::Element(element) = node else {
            continue;
        };
        if turbo_dom_layout::layout::value::display_of(&element.style)
            == turbo_dom_layout::layout::value::Display::None
        {
            continue;
        }
        path.push(element);
        validate_style_with_lifecycle(
            &element.style,
            definitions,
            seen,
            element
                .source
                .and_then(|source| completed.get(&source))
                .is_some_and(|names| completed_names_match(&element.style, names)),
            || independent_effect(path, requested, kind),
        )?;
        validate_rect_nodes(
            &element.children,
            definitions,
            requested,
            kind,
            path,
            seen,
            completed,
        )?;
        path.pop();
    }
    Ok(())
}

type Seen<'a> = std::collections::HashSet<(&'static str, &'a str)>;
fn validate_nodes(
    nodes: &[StyledNode],
    definitions: &Definitions,
    completed: &CompletedAnimations,
) -> Result<(), String> {
    if definitions.fonts.is_empty() && definitions.animations.is_empty() {
        return Ok(());
    }
    let mut seen = Seen::new();
    let mut pending = nodes.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        let StyledNode::Element(element) = node else {
            continue;
        };
        if turbo_dom_layout::layout::value::display_of(&element.style)
            == turbo_dom_layout::layout::value::Display::None
        {
            continue;
        }
        validate_style_with_lifecycle(
            &element.style,
            definitions,
            &mut seen,
            element
                .source
                .and_then(|source| completed.get(&source))
                .is_some_and(|names| completed_names_match(&element.style, names)),
            || false,
        )?;
        pending.extend(element.children.iter());
    }
    Ok(())
}

#[cfg(test)]
fn validate_style<'a>(
    style: &'a ComputedStyle,
    definitions: &Definitions,
    seen: &mut Seen<'a>,
) -> Result<(), String> {
    validate_style_for_rect(style, definitions, seen, || false)
}
#[cfg(test)]
fn validate_style_for_rect<'a>(
    style: &'a ComputedStyle,
    definitions: &Definitions,
    seen: &mut Seen<'a>,
    independent: impl Fn() -> bool,
) -> Result<(), String> {
    validate_style_with_lifecycle(style, definitions, seen, false, independent)
}
fn validate_style_with_lifecycle<'a>(
    style: &'a ComputedStyle,
    definitions: &Definitions,
    seen: &mut Seen<'a>,
    completed: bool,
    independent: impl Fn() -> bool,
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
                let used = family_name(&family)?.to_lowercase();
                if definitions.fonts.contains(&used) {
                    USED_FONTS.with(|fonts| fonts.borrow_mut().insert(used));
                }
            }
        }
    }
    if !definitions.animations.is_empty() && !completed {
        validate_animations(style, definitions, independent)?;
    }
    Ok(())
}
fn animation_properties() -> [(&'static str, PropertyId<'static>); 4] {
    use lightningcss::vendor_prefix::VendorPrefix;
    [
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
    ]
}
fn declared_animation_names(id: PropertyId, value: &str) -> Result<Vec<String>, String> {
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
    Ok(names
        .into_iter()
        .filter_map(|name| match name {
            AnimationName::None => None,
            AnimationName::Ident(name) => Some(name.0.to_string()),
            AnimationName::String(name) => Some(name.0.to_string()),
        })
        .collect())
}
// Native scoped cascade and canonical effect input must identify the same effects.
// Missing or disagreeing producer style data is never a completion certificate.
fn completed_names_match(style: &ComputedStyle, completed: &[String]) -> bool {
    let mut matched = false;
    for (name, id) in animation_properties() {
        let Some(value) = style.get(name) else {
            continue;
        };
        if declared_animation_names(id, value).is_ok_and(|names| names == completed) {
            matched = true;
        } else {
            return false;
        }
    }
    matched && !completed.is_empty()
}
fn validate_animations(
    style: &ComputedStyle,
    definitions: &Definitions,
    independent: impl Fn() -> bool,
) -> Result<(), String> {
    for (name, id) in animation_properties() {
        let Some(value) = style.get(name) else {
            continue;
        };
        let names = declared_animation_names(id, value)?;
        for used in names {
            if definitions.animations.get(&used).is_some_and(|frames| {
                !constant_animation(style, frames)
                    && !(paint_or_transform_only(style, frames) && independent())
            }) {
                return Err(format!(
                    "native live layout animation resource is unsupported: {used}"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod definition_tests {
    use super::*;
    fn animation_definitions(source: &str) -> Definitions {
        Definitions::collect(&[crate::live_stylesheets::ActiveRule {
            text: source.into(),
            scoped: None,
            resource: Some(definition(source).unwrap()),
            containers: Vec::new(),
        }])
    }

    #[test]
    fn completed_effect_names_cannot_waive_a_different_scoped_animation() {
        let scoped = ComputedStyle::from_pairs([("animation", "local 100s")]);
        assert!(!completed_names_match(&scoped, &["outer".into()]));
        assert!(!completed_names_match(&scoped, &[]));
        assert!(completed_names_match(&scoped, &["local".into()]));
        let multiple = ComputedStyle::from_pairs([("animation-name", "first,second")]);
        assert!(!completed_names_match(
            &multiple,
            &["second".into(), "first".into()]
        ));
        assert!(completed_names_match(
            &multiple,
            &["first".into(), "second".into()]
        ));
    }

    #[test]
    fn rect_dependency_proof_only_runs_for_used_paint_animation() {
        let definitions = animation_definitions("@keyframes spin{to{transform:rotate(360deg)}}");
        let calls = std::cell::Cell::new(0);
        let independent = || {
            calls.set(calls.get() + 1);
            true
        };
        validate_style_for_rect(
            &ComputedStyle::default(),
            &definitions,
            &mut Seen::new(),
            independent,
        )
        .unwrap();
        assert_eq!(calls.get(), 0);
        let changing = animation_definitions("@keyframes spin{to{width:100px}}");
        let used = ComputedStyle::from_pairs([("animation-name", "spin")]);
        assert!(validate_style_for_rect(&used, &changing, &mut Seen::new(), independent).is_err());
        assert_eq!(calls.get(), 0);
        validate_style_for_rect(&used, &definitions, &mut Seen::new(), independent).unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn active_keyframes_equal_to_each_underlying_style_are_layout_inert() {
        for (css, property, value) in [
            ("@keyframes steady{0%{display:block}}", "display", "block"),
            (
                "@keyframes steady{0%{width:43px}100%{width:43px;opacity:.5}}",
                "width",
                "43.0px",
            ),
        ] {
            let definitions = animation_definitions(css);
            let style =
                ComputedStyle::from_pairs([("animation-name", "steady"), (property, value)]);
            validate_style(&style, &definitions, &mut Seen::new()).unwrap();
        }
    }

    #[test]
    fn animation_proof_is_not_reused_for_a_different_underlying_style() {
        let definitions = animation_definitions("@keyframes steady{0%{display:block}}");
        let mut seen = Seen::new();
        let safe = ComputedStyle::from_pairs([("animation-name", "steady"), ("display", "block")]);
        validate_style(&safe, &definitions, &mut seen).unwrap();
        let changing =
            ComputedStyle::from_pairs([("animation-name", "steady"), ("display", "inline-block")]);
        assert!(validate_style(&changing, &definitions, &mut seen)
            .unwrap_err()
            .contains("animation resource"));
    }

    #[test]
    fn changing_unresolved_or_additive_keyframes_remain_unsupported() {
        for (css, extra) in [
            (
                "@keyframes steady{0%{width:43px}100%{width:44px}}",
                "replace",
            ),
            ("@keyframes steady{0%{width:var(--size)}}", "replace"),
            ("@keyframes steady{0%{width:43px}}", "add"),
            ("@keyframes steady{0%{width:inherit}}", "replace"),
        ] {
            let definitions = animation_definitions(css);
            let style = ComputedStyle::from_pairs([
                ("animation-name", "steady"),
                ("width", "43px"),
                ("--size", "43px"),
                ("animation-composition", extra),
            ]);
            assert!(
                validate_style(&style, &definitions, &mut Seen::new())
                    .unwrap_err()
                    .contains("animation resource"),
                "{css}"
            );
        }
    }

    #[test]
    fn resource_rules_classify_by_what_they_can_define() {
        let cases = [
            (
                "@font-face{font-family:\"My Font\";src:url(a.woff2)}",
                Ok(Definition::Font("My Font".into())),
            ),
            // A generic family keyword or a missing source defines nothing usable.
            (
                "@font-face{font-family:serif;src:url(a.woff2)}",
                Ok(Definition::Inert),
            ),
            ("@font-face{font-family:x}", Ok(Definition::Inert)),
            (
                "@keyframes fade{from{opacity:0}to{opacity:1}}",
                Ok(Definition::Inert),
            ),
            (
                "@keyframes spin{to{transform:rotate(1deg)}}",
                Ok(Definition::Animation("spin".into())),
            ),
            (
                "@keyframes \"spin it\"{to{width:1px}}",
                Ok(Definition::Animation("spin it".into())),
            ),
            (
                "@container card (width>1px){p{color:red}}",
                Ok(Definition::Container("card".into())),
            ),
            (
                "@property --c{syntax:'<color>';inherits:false;initial-value:red}",
                Ok(Definition::RegisteredColor("--c".into())),
            ),
            (
                "@view-transition{navigation:auto}",
                Ok(Definition::ViewTransition),
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(definition(source), expected, "{source}");
        }
        for source in [
            "@container (width>1px){p{color:red}}",
            "@property --n{syntax:'<length>';inherits:false;initial-value:1px}",
            "@view-transition{navigation:auto;types:slide}",
            "p{color:red}",
            "@media screen{p{color:red}}",
        ] {
            assert!(definition(source).is_err(), "{source}");
        }
    }
}
