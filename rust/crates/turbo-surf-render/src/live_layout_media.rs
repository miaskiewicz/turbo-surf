//! Typed viewport media evaluation. Hardware/preferences require real host data;
//! a missing capability never silently makes a media condition true.
use cssparser::{Parser, ParserInput};
use lightningcss::media_query::{
    MediaCondition, MediaFeature, MediaFeatureComparison as Compare, MediaFeatureId as Feature,
    MediaFeatureName, MediaFeatureValue, MediaList, MediaType, Operator, Qualifier, QueryFeature,
};
use lightningcss::stylesheet::ParserOptions;
use lightningcss::values::length::{Length, LengthValue};

/// An immutable native environment, captured before guest scripts. None denotes
/// an unavailable platform capability, never an inferred negative answer.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HostMedia {
    forced_colors: Option<bool>,
    increased_contrast: Option<bool>,
    dark_scheme: Option<bool>,
    reduced_motion: Option<bool>,
    reduced_transparency: Option<bool>,
    pointer: Option<bool>,
    hover: Option<bool>,
}
impl HostMedia {
    pub(crate) fn capture(native_pointer: bool) -> Self {
        #[cfg(target_os = "macos")]
        let mut value = macos::capture();
        #[cfg(not(target_os = "macos"))]
        let mut value = Self::default();
        // This browserless view implements mouse input and hit testing through
        // its installed native driver, independently of attached OS devices.
        value.pointer = native_pointer.then_some(true);
        value.hover = native_pointer.then_some(true);
        value
    }
    fn keyword(self, feature: Feature) -> Result<Option<(&'static str, bool)>, String> {
        let (value, positive, negative) = match feature {
            Feature::ForcedColors => (self.forced_colors, "active", "none"),
            Feature::PrefersContrast => (self.increased_contrast, "more", "no-preference"),
            Feature::PrefersColorScheme => (self.dark_scheme, "dark", "light"),
            Feature::PrefersReducedMotion => (self.reduced_motion, "reduce", "no-preference"),
            Feature::PrefersReducedTransparency => {
                (self.reduced_transparency, "reduce", "no-preference")
            }
            Feature::Pointer | Feature::AnyPointer => (self.pointer, "fine", "none"),
            Feature::Hover | Feature::AnyHover => (self.hover, "hover", "none"),
            _ => return Ok(None),
        };
        let Some(value) = value else {
            return Err(format!(
                "native live layout media requires host capability {feature:?}"
            ));
        };
        let boolean = feature == Feature::PrefersColorScheme || value;
        Ok(Some((if value { positive } else { negative }, boolean)))
    }
}
#[cfg(target_os = "macos")]
mod macos {
    use super::HostMedia;
    use std::ffi::{c_char, c_void};
    type Id = *mut c_void;
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Id;
        fn objc_msgSend();
        fn objc_autoreleasePoolPush() -> Id;
        fn objc_autoreleasePoolPop(pool: Id);
    }
    struct Pool(Id);
    impl Drop for Pool {
        fn drop(&mut self) {
            unsafe { objc_autoreleasePoolPop(self.0) }
        }
    }
    unsafe fn selector(name: &[u8]) -> Id {
        sel_registerName(name.as_ptr().cast())
    }
    unsafe fn object(receiver: Id, name: &[u8]) -> Id {
        let call: unsafe extern "C" fn(Id, Id) -> Id =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        call(receiver, selector(name))
    }
    unsafe fn flag(receiver: Id, name: &[u8]) -> Option<bool> {
        if receiver.is_null() {
            return None;
        }
        let responds: unsafe extern "C" fn(Id, Id, Id) -> bool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let method = selector(name);
        if !responds(receiver, selector(b"respondsToSelector:\0"), method) {
            return None;
        }
        let call: unsafe extern "C" fn(Id, Id) -> bool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        Some(call(receiver, method))
    }
    unsafe fn string(value: &[u8]) -> Id {
        let class = objc_getClass(c"NSString".as_ptr());
        let call: unsafe extern "C" fn(Id, Id, *const c_char) -> Id =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        call(
            class,
            selector(b"stringWithUTF8String:\0"),
            value.as_ptr().cast(),
        )
    }
    unsafe fn dark_scheme() -> Option<bool> {
        let class = objc_getClass(c"NSUserDefaults".as_ptr());
        if class.is_null() {
            return None;
        }
        let defaults = object(class, b"standardUserDefaults\0");
        if defaults.is_null() {
            return None;
        }
        let call: unsafe extern "C" fn(Id, Id, Id) -> Id =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let style = call(
            defaults,
            selector(b"stringForKey:\0"),
            string(b"AppleInterfaceStyle\0"),
        );
        if style.is_null() {
            return Some(false);
        }
        let equal: unsafe extern "C" fn(Id, Id, Id) -> bool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        Some(equal(
            style,
            selector(b"isEqualToString:\0"),
            string(b"Dark\0"),
        ))
    }
    pub(super) fn capture() -> HostMedia {
        unsafe {
            let _pool = Pool(objc_autoreleasePoolPush());
            let class = objc_getClass(c"NSWorkspace".as_ptr());
            let workspace = if class.is_null() {
                std::ptr::null_mut()
            } else {
                object(class, b"sharedWorkspace\0")
            };
            HostMedia {
                // macOS accessibility contrast changes contrast preference, not
                // the forced palette. The native view has no page-color override.
                forced_colors: Some(false),
                increased_contrast: flag(
                    workspace,
                    b"accessibilityDisplayShouldIncreaseContrast\0",
                ),
                dark_scheme: dark_scheme(),
                reduced_motion: flag(workspace, b"accessibilityDisplayShouldReduceMotion\0"),
                reduced_transparency: flag(
                    workspace,
                    b"accessibilityDisplayShouldReduceTransparency\0",
                ),
                ..HostMedia::default()
            }
        }
    }
}

pub(crate) struct ViewportMedia {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) initial_font_size: f32,
    pub(crate) host: HostMedia,
}

impl ViewportMedia {
    pub(crate) fn matches(&self, source: &str) -> Result<bool, String> {
        if ![self.width, self.height, self.initial_font_size]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            return Err("native media viewport/font size must be finite and positive".into());
        }
        let mut input = ParserInput::new(source);
        let mut parser = Parser::new(&mut input);
        let Ok(list) = MediaList::parse(&mut parser, &ParserOptions::default()) else {
            return Ok(false);
        };
        if parser.expect_exhausted().is_err() {
            return Ok(false);
        }
        if list.media_queries.is_empty() {
            return Ok(true);
        }
        let mut unavailable = None;
        for query in &list.media_queries {
            let media = matches!(query.media_type, MediaType::Screen | MediaType::All);
            let condition = if media {
                query
                    .condition
                    .as_ref()
                    .map_or(Ok(true), |c| self.condition(c))
            } else {
                Ok(false)
            };
            match condition {
                Ok(value) => {
                    let value = if query.qualifier == Some(Qualifier::Not) {
                        !value
                    } else {
                        value
                    };
                    if value {
                        return Ok(true);
                    }
                }
                Err(error) => {
                    unavailable.get_or_insert(error);
                }
            }
        }
        unavailable.map_or(Ok(false), Err)
    }

    fn condition(&self, condition: &MediaCondition) -> Result<bool, String> {
        match condition {
            MediaCondition::Feature(feature) => self.feature(feature),
            MediaCondition::Not(inner) => Ok(!self.condition(inner)?),
            MediaCondition::Operation {
                operator,
                conditions,
            } => {
                let and = *operator == Operator::And;
                let mut unavailable = None;
                for condition in conditions {
                    match self.condition(condition) {
                        Ok(value) if value != and => return Ok(value),
                        Ok(_) => {}
                        Err(error) => {
                            unavailable.get_or_insert(error);
                        }
                    }
                }
                unavailable.map_or(Ok(and), Err)
            }
            MediaCondition::Unknown(_) => Ok(false),
        }
    }

    fn feature(&self, feature: &MediaFeature) -> Result<bool, String> {
        match feature {
            QueryFeature::Plain { name, value } => self.compare(name, value, Compare::Equal),
            QueryFeature::Boolean { name } => {
                if let MediaFeatureName::Standard(feature) = name {
                    if let Some((_, value)) = self.host.keyword(*feature)? {
                        return Ok(value);
                    }
                }
                Ok(self.actual(name)?.is_some_and(|value| value != 0.0))
            }
            QueryFeature::Range {
                name,
                operator,
                value,
            } => self.compare(name, value, *operator),
            QueryFeature::Interval {
                name,
                start,
                start_operator,
                end,
                end_operator,
            } => {
                let Some(actual) = self.actual(name)? else {
                    return Ok(false);
                };
                Ok(compare(self.value(start)?, actual, *start_operator)
                    && compare(actual, self.value(end)?, *end_operator))
            }
        }
    }

    fn compare(
        &self,
        name: &MediaFeatureName<Feature>,
        value: &MediaFeatureValue,
        operator: Compare,
    ) -> Result<bool, String> {
        if let MediaFeatureName::Standard(feature) = name {
            if let Some((actual, _)) = self.host.keyword(*feature)? {
                return Ok(
                    matches!((value,operator),(MediaFeatureValue::Ident(value),Compare::Equal) if actual.eq_ignore_ascii_case(value.0.as_ref())),
                );
            }
        }
        let Some(actual) = self.actual(name)? else {
            return Ok(false);
        };
        if matches!(name, MediaFeatureName::Standard(Feature::Orientation)) {
            let MediaFeatureValue::Ident(value) = value else {
                return Ok(false);
            };
            return Ok(match value.0.as_ref() {
                "portrait" => self.height >= self.width,
                "landscape" => self.width > self.height,
                _ => false,
            });
        }
        Ok(compare(actual, self.value(value)?, operator))
    }

    fn actual(&self, name: &MediaFeatureName<Feature>) -> Result<Option<f64>, String> {
        match name {
            MediaFeatureName::Standard(Feature::Width) => Ok(Some(f64::from(self.width))),
            MediaFeatureName::Standard(Feature::Height) => Ok(Some(f64::from(self.height))),
            MediaFeatureName::Standard(Feature::AspectRatio) => {
                Ok(Some(f64::from(self.width) / f64::from(self.height)))
            }
            MediaFeatureName::Standard(Feature::Orientation) => Ok(Some(1.0)),
            MediaFeatureName::Standard(feature) => Err(format!(
                "native live layout media requires host capability {feature:?}"
            )),
            // Unknown author-defined feature names are false in CSS, including
            // under an explicit not; they are different from known host features.
            MediaFeatureName::Custom(_) | MediaFeatureName::Unknown(_) => Ok(None),
        }
    }

    fn value(&self, value: &MediaFeatureValue) -> Result<f64, String> {
        match value {
            MediaFeatureValue::Length(Length::Value(LengthValue::Em(v) | LengthValue::Rem(v))) => {
                Ok(f64::from(*v) * f64::from(self.initial_font_size))
            }
            MediaFeatureValue::Length(length) => length
                .to_px()
                .map(f64::from)
                .ok_or("native media length requires unresolved font/viewport/calc units".into()),
            MediaFeatureValue::Ratio(value) => Ok(f64::from(value.0) / f64::from(value.1)),
            _ => Err("native media feature has no available numerical value".into()),
        }
    }
}

fn compare(left: f64, right: f64, operator: Compare) -> bool {
    match operator {
        Compare::Equal => left == right,
        Compare::GreaterThan => left > right,
        Compare::GreaterThanEqual => left >= right,
        Compare::LessThan => left < right,
        Compare::LessThanEqual => left <= right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewport() -> ViewportMedia {
        ViewportMedia {
            width: 800.0,
            height: 600.0,
            initial_font_size: 16.0,
            host: HostMedia::default(),
        }
    }
    #[test]
    fn viewport_features_use_actual_dimensions_ranges_negation_and_initial_font_units() {
        for query in [
            "",
            "screen",
            "not print",
            "(width: 800px)",
            "(min-width: 50em)",
            "(max-height: 37.5rem)",
            "(600px < width <= 800px)",
            "(aspect-ratio: 4 / 3)",
            "(orientation: landscape)",
            "not (unknown-feature: invalid)",
            "print, screen and (width > 700px)",
        ] {
            assert!(viewport().matches(query).unwrap(), "{query}");
        }
        for query in [
            "print",
            "not screen",
            "(width < 800px)",
            "(min-width: 51em)",
            "(orientation: portrait)",
            "(unknown-feature: invalid)",
            "???",
        ] {
            assert!(!viewport().matches(query).unwrap(), "{query}");
        }
        let square = ViewportMedia {
            width: 600.0,
            height: 600.0,
            initial_font_size: 16.0,
            host: HostMedia::default(),
        };
        assert!(square.matches("(orientation: portrait)").unwrap());
    }
    #[test]
    fn missing_known_host_features_and_invalid_viewports_are_explicit() {
        assert!(viewport()
            .matches("(hover: hover)")
            .unwrap_err()
            .contains("host capability"));
        assert!(viewport()
            .matches("(prefers-color-scheme: light)")
            .unwrap_err()
            .contains("host capability"));
        assert!(viewport()
            .matches("(width: 10ch)")
            .unwrap_err()
            .contains("unresolved"));
        let invalid = ViewportMedia {
            width: 0.0,
            ..viewport()
        };
        assert!(invalid.matches("screen").is_err());
    }
    #[test]
    fn logically_decisive_viewport_conditions_do_not_require_unrelated_host_features() {
        let view = viewport();
        assert!(view.matches("(prefers-color-scheme:dark), screen").unwrap());
        assert!(!view
            .matches("(prefers-color-scheme:dark) and (width:1px)")
            .unwrap());
        assert!(view
            .matches("(prefers-color-scheme:dark) or (width:800px)")
            .unwrap());
        assert!(view
            .matches("not ((prefers-color-scheme:dark) and (width:1px))")
            .unwrap());
        assert!(view
            .matches("(prefers-color-scheme:dark) and (width:800px)")
            .is_err());
    }
    #[test]
    fn captured_host_modes_use_real_keyword_and_boolean_semantics() {
        let host = HostMedia {
            forced_colors: Some(true),
            increased_contrast: Some(true),
            dark_scheme: Some(true),
            reduced_motion: Some(true),
            reduced_transparency: Some(true),
            pointer: Some(true),
            hover: Some(true),
        };
        let active = ViewportMedia { host, ..viewport() };
        for query in [
            "(forced-colors)",
            "(forced-colors: active)",
            "(prefers-contrast)",
            "(prefers-contrast: more)",
            "(prefers-color-scheme)",
            "(prefers-color-scheme: dark)",
            "(prefers-reduced-motion: reduce)",
            "(prefers-reduced-transparency: reduce)",
            "(pointer: fine)",
            "(any-pointer: fine)",
            "(hover: hover)",
            "(any-hover: hover)",
        ] {
            assert!(active.matches(query).unwrap(), "{query}");
        }
        for query in [
            "(forced-colors: none)",
            "(forced-colors: invalid)",
            "(prefers-contrast: no-preference)",
            "(prefers-contrast: less)",
            "(prefers-contrast: custom)",
            "(prefers-color-scheme: light)",
            "(prefers-reduced-motion: no-preference)",
            "(prefers-reduced-transparency: no-preference)",
            "(pointer: coarse)",
            "(hover: none)",
        ] {
            assert!(!active.matches(query).unwrap(), "{query}");
        }
        let inactive = ViewportMedia {
            host: HostMedia {
                forced_colors: Some(false),
                increased_contrast: Some(false),
                dark_scheme: Some(false),
                reduced_motion: Some(false),
                reduced_transparency: Some(false),
                pointer: Some(false),
                hover: Some(false),
            },
            ..viewport()
        };
        for query in [
            "(forced-colors: none)",
            "(prefers-contrast: no-preference)",
            "(prefers-color-scheme)",
            "(prefers-color-scheme: light)",
            "(prefers-reduced-motion: no-preference)",
            "(prefers-reduced-transparency: no-preference)",
            "(pointer: none)",
            "(hover: none)",
        ] {
            assert!(inactive.matches(query).unwrap(), "{query}");
        }
        for query in [
            "(forced-colors)",
            "(prefers-contrast)",
            "(prefers-reduced-motion)",
            "(prefers-reduced-transparency)",
            "(pointer)",
            "(hover)",
        ] {
            assert!(!inactive.matches(query).unwrap(), "{query}");
        }
        assert!(viewport()
            .matches("(forced-colors: none)")
            .unwrap_err()
            .contains("host capability"));
        assert!(viewport()
            .matches("(prefers-contrast: no-preference)")
            .unwrap_err()
            .contains("host capability"));
    }
}
