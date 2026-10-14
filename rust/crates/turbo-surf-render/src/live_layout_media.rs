//! Typed viewport media evaluation. Hardware/preferences require real host data;
//! a missing capability never silently makes a media condition true.
use cssparser::{Parser, ParserInput};
use lightningcss::media_query::{
    MediaCondition, MediaFeature, MediaFeatureComparison as Compare, MediaFeatureId as Feature,
    MediaFeatureName, MediaFeatureValue, MediaList, MediaType, Operator, Qualifier, QueryFeature,
};
use lightningcss::stylesheet::ParserOptions;
use lightningcss::values::length::{Length, LengthValue};

pub(crate) struct ViewportMedia {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) initial_font_size: f32,
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
}
