//! Size container queries in live layout (turbo-surf#105). An `@container` block's
//! style rules keep their prepared targets; each target applies only while its query
//! container (the nearest flat-tree ancestor size container with a matching name and
//! the queried axes) satisfies the condition, as in Chrome. Layout iterates: lay out,
//! read container content boxes, re-select targets, until the selection is stable.
use lightningcss::media_query::{
    MediaFeatureComparison, MediaFeatureName, MediaFeatureValue, Operator, QueryFeature,
};
use lightningcss::rules::container::{ContainerCondition, ContainerSizeFeatureId};
use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use std::collections::HashMap;
use turbo_dom_layout::layout_snapshot::{LayoutSnapshot, SourceId};
use turbo_dom_layout::StyledNode;

/// One `@container` prelude: an optional container name and a size condition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Query {
    name: Option<String>,
    condition: Condition,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Feature {
    Width,
    Height,
    InlineSize,
    BlockSize,
    AspectRatio,
    Orientation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Compare {
    Equal,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

#[derive(Debug, Clone, PartialEq)]
enum Condition {
    /// A boolean-context feature (`(width)`): true when the feature is non-zero.
    Present(Feature),
    /// `feature <compare> value` for every pair (one for a range, two for an interval).
    Compare(Feature, Vec<(Compare, f32)>),
    Portrait(bool),
    Not(Box<Condition>),
    All(Vec<Condition>),
    Any(Vec<Condition>),
}

/// A size container's queried state: its names, axes, and used content box.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Container {
    names: Vec<String>,
    block_axis: bool,
    width: f32,
    height: f32,
}

/// Parses an `@container` prelude (`@container name (min-width: 200px)`).
pub(crate) fn parse(prelude: &str) -> Result<Query, String> {
    let source = format!("{prelude} {{}}");
    let sheet = StyleSheet::parse(&source, ParserOptions::default())
        .map_err(|_| "native container query cannot be parsed")?;
    let [CssRule::Container(rule)] = sheet.rules.0.as_slice() else {
        return Err("native container query prelude is not one @container rule".into());
    };
    let condition = rule
        .condition
        .as_ref()
        .ok_or("native container query has no size condition")?;
    Ok(Query {
        name: rule.name.as_ref().map(|name| name.0 .0.to_string()),
        condition: condition_of(condition)?,
    })
}

fn condition_of(condition: &ContainerCondition<'_>) -> Result<Condition, String> {
    Ok(match condition {
        ContainerCondition::Feature(feature) => feature_of(feature)?,
        ContainerCondition::Not(inner) => Condition::Not(Box::new(condition_of(inner)?)),
        ContainerCondition::Operation {
            operator,
            conditions,
        } => {
            let conditions = conditions
                .iter()
                .map(condition_of)
                .collect::<Result<Vec<_>, _>>()?;
            match operator {
                Operator::And => Condition::All(conditions),
                Operator::Or => Condition::Any(conditions),
            }
        }
        _ => return Err("native container style and scroll-state queries are unsupported".into()),
    })
}

fn feature_of(feature: &QueryFeature<'_, ContainerSizeFeatureId>) -> Result<Condition, String> {
    let name = |name: &MediaFeatureName<'_, ContainerSizeFeatureId>| match name {
        MediaFeatureName::Standard(id) => Ok(match id {
            ContainerSizeFeatureId::Width => Feature::Width,
            ContainerSizeFeatureId::Height => Feature::Height,
            ContainerSizeFeatureId::InlineSize => Feature::InlineSize,
            ContainerSizeFeatureId::BlockSize => Feature::BlockSize,
            ContainerSizeFeatureId::AspectRatio => Feature::AspectRatio,
            ContainerSizeFeatureId::Orientation => Feature::Orientation,
        }),
        _ => Err("native container query has an unknown size feature".to_string()),
    };
    Ok(match feature {
        QueryFeature::Boolean { name: id } => Condition::Present(name(id)?),
        QueryFeature::Plain { name: id, value } => {
            let feature = name(id)?;
            if feature == Feature::Orientation {
                return orientation(value);
            }
            Condition::Compare(feature, vec![(Compare::Equal, number(value)?)])
        }
        QueryFeature::Range {
            name: id,
            operator,
            value,
        } => Condition::Compare(name(id)?, vec![(compare(*operator), number(value)?)]),
        QueryFeature::Interval {
            name: id,
            start,
            start_operator,
            end,
            end_operator,
        } => Condition::Compare(
            name(id)?,
            vec![
                // `start < feature` is `feature > start`.
                (flip(compare(*start_operator)), number(start)?),
                (compare(*end_operator), number(end)?),
            ],
        ),
    })
}

fn orientation(value: &MediaFeatureValue<'_>) -> Result<Condition, String> {
    match value {
        MediaFeatureValue::Ident(ident) if ident.0.eq_ignore_ascii_case("portrait") => {
            Ok(Condition::Portrait(true))
        }
        MediaFeatureValue::Ident(ident) if ident.0.eq_ignore_ascii_case("landscape") => {
            Ok(Condition::Portrait(false))
        }
        _ => Err("native container orientation query has an unknown value".into()),
    }
}

fn compare(operator: MediaFeatureComparison) -> Compare {
    match operator {
        MediaFeatureComparison::Equal => Compare::Equal,
        MediaFeatureComparison::GreaterThan => Compare::Greater,
        MediaFeatureComparison::GreaterThanEqual => Compare::GreaterEqual,
        MediaFeatureComparison::LessThan => Compare::Less,
        MediaFeatureComparison::LessThanEqual => Compare::LessEqual,
    }
}

fn flip(compare: Compare) -> Compare {
    match compare {
        Compare::Greater => Compare::Less,
        Compare::GreaterEqual => Compare::LessEqual,
        Compare::Less => Compare::Greater,
        Compare::LessEqual => Compare::GreaterEqual,
        Compare::Equal => Compare::Equal,
    }
}

/// A query value in CSS px (lengths) or as a ratio. Font-relative and viewport units
/// need the container's computed font and are refused rather than guessed.
fn number(value: &MediaFeatureValue<'_>) -> Result<f32, String> {
    match value {
        MediaFeatureValue::Length(length) => length
            .to_px()
            .ok_or_else(|| "native container query length must be absolute".into()),
        MediaFeatureValue::Number(number) => Ok(*number),
        MediaFeatureValue::Integer(number) => Ok(*number as f32),
        MediaFeatureValue::Ratio(ratio) => Ok(ratio.0 / ratio.1),
        _ => Err("native container query value is unsupported".into()),
    }
}

impl Condition {
    /// The block axis is queried (needs `container-type: size`).
    fn block_axis(&self) -> bool {
        match self {
            Condition::Present(feature) | Condition::Compare(feature, _) => {
                !matches!(feature, Feature::Width | Feature::InlineSize)
            }
            Condition::Portrait(_) => true,
            Condition::Not(inner) => inner.block_axis(),
            Condition::All(all) | Condition::Any(all) => all.iter().any(Condition::block_axis),
        }
    }
    fn holds(&self, container: &Container) -> bool {
        let value = |feature: Feature| match feature {
            Feature::Width | Feature::InlineSize => container.width,
            Feature::Height | Feature::BlockSize => container.height,
            Feature::AspectRatio => container.width / container.height,
            Feature::Orientation => 0.0,
        };
        match self {
            Condition::Present(feature) => value(*feature) != 0.0,
            Condition::Compare(feature, tests) => {
                let actual = value(*feature);
                tests.iter().all(|&(compare, expected)| match compare {
                    Compare::Equal => actual == expected,
                    Compare::Greater => actual > expected,
                    Compare::GreaterEqual => actual >= expected,
                    Compare::Less => actual < expected,
                    Compare::LessEqual => actual <= expected,
                })
            }
            Condition::Portrait(portrait) => (container.height >= container.width) == *portrait,
            Condition::Not(inner) => !inner.holds(container),
            Condition::All(all) => all.iter().all(|inner| inner.holds(container)),
            Condition::Any(all) => all.iter().any(|inner| inner.holds(container)),
        }
    }
}

impl Query {
    /// Whether this query holds for an element whose ancestor size containers are
    /// `ancestors` (nearest first). No eligible container: the query is false.
    pub(crate) fn holds(&self, ancestors: &[&Container]) -> bool {
        let block_axis = self.condition.block_axis();
        ancestors
            .iter()
            .find(|container| {
                (!block_axis || container.block_axis)
                    && self
                        .name
                        .as_ref()
                        .is_none_or(|name| container.names.contains(name))
            })
            .is_some_and(|container| self.condition.holds(container))
    }
}

/// Each element's ancestor size containers (nearest first), by source id.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Containers {
    ancestors: HashMap<SourceId, Vec<SourceId>>,
    kinds: HashMap<SourceId, (Vec<String>, bool)>,
}

impl Containers {
    /// Records the flat-tree container chain from the cascaded styled tree.
    pub(crate) fn collect(nodes: &[StyledNode], document: u32) -> Self {
        let mut result = Self::default();
        let mut pending = nodes
            .iter()
            .map(|node| (node, Vec::new()))
            .collect::<Vec<_>>();
        while let Some((node, chain)) = pending.pop() {
            let StyledNode::Element(element) = node else {
                continue;
            };
            let mut inner = chain.clone();
            if let Some(handle) = element.source {
                let id = crate::live_layout::source_id(document, handle);
                result.ancestors.insert(id, chain);
                if let Some(kind) = container_kind(&element.style) {
                    result.kinds.insert(id, kind);
                    inner.insert(0, id);
                }
            }
            pending.extend(element.children.iter().map(|child| (child, inner.clone())));
        }
        result
    }

    /// Whether every query holds for `target` against `snapshot`'s content boxes.
    pub(crate) fn matches(
        &self,
        snapshot: &LayoutSnapshot,
        target: SourceId,
        queries: &[Query],
    ) -> bool {
        let containers = self
            .ancestors
            .get(&target)
            .into_iter()
            .flatten()
            .filter_map(|&id| self.container(snapshot, id))
            .collect::<Vec<_>>();
        let refs = containers.iter().collect::<Vec<_>>();
        queries.iter().all(|query| query.holds(&refs))
    }

    fn container(&self, snapshot: &LayoutSnapshot, id: SourceId) -> Option<Container> {
        let (names, block_axis) = self.kinds.get(&id)?;
        // A container without a box (display: none / contents) cannot be queried.
        let fragment = snapshot.source(id)?.fragment_indices.first()?;
        let used = snapshot.fragments().get(*fragment)?.used_box?;
        Some(Container {
            names: names.clone(),
            block_axis: *block_axis,
            width: used.content_width,
            height: used.content_height,
        })
    }
}

/// `container-type` (or the `container` shorthand) of a size container: its names and
/// whether it also contains the block axis. `normal` is not a size container.
fn container_kind(style: &turbo_dom_layout::ComputedStyle) -> Option<(Vec<String>, bool)> {
    let shorthand = style.get("container").map(|value| {
        let (names, kind) = value.split_once('/').unwrap_or((value, "normal"));
        (names.trim().to_owned(), kind.trim().to_owned())
    });
    let kind = style
        .get("container-type")
        .map(str::to_owned)
        .or_else(|| shorthand.as_ref().map(|(_, kind)| kind.clone()))?;
    let block_axis = match kind.trim().to_ascii_lowercase().as_str() {
        "size" => true,
        "inline-size" => false,
        _ => return None,
    };
    let names = style
        .get("container-name")
        .map(str::to_owned)
        .or_else(|| shorthand.map(|(names, _)| names))
        .unwrap_or_default();
    let names = names
        .split_ascii_whitespace()
        .filter(|name| !name.eq_ignore_ascii_case("none"))
        .map(str::to_owned)
        .collect();
    Some((names, block_axis))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(names: &[&str], block_axis: bool, width: f32, height: f32) -> Container {
        Container {
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            block_axis,
            width,
            height,
        }
    }

    fn holds(prelude: &str, ancestors: &[&Container]) -> bool {
        parse(prelude).unwrap().holds(ancestors)
    }

    #[test]
    fn size_features_compare_against_the_content_box() {
        let wide = container(&[], true, 300.0, 80.0);
        for (prelude, expected) in [
            ("@container (min-width: 200px)", true),
            ("@container (min-width: 300px)", true),
            ("@container (min-width: 301px)", false),
            ("@container (max-width: 299px)", false),
            ("@container (width: 300px)", true),
            ("@container (width: 299px)", false),
            ("@container (width > 300px)", false),
            ("@container (width >= 300px)", true),
            ("@container (width < 301px)", true),
            ("@container (width <= 299px)", false),
            ("@container (200px < width < 400px)", true),
            ("@container (300px < width <= 400px)", false),
            ("@container (100px <= width < 300px)", false),
            ("@container (inline-size > 299px)", true),
            ("@container (height: 80px)", true),
            ("@container (block-size < 80px)", false),
            ("@container (width)", true),
            ("@container (aspect-ratio > 3)", true),
            ("@container (aspect-ratio: 15/4)", true),
            ("@container (orientation: landscape)", true),
            ("@container (orientation: portrait)", false),
            ("@container not (min-width: 400px)", true),
            ("@container (min-width: 400px) or (height: 80px)", true),
            ("@container (min-width: 200px) and (height: 81px)", false),
            ("@container (min-width: 2in)", true),
        ] {
            assert_eq!(holds(prelude, &[&wide]), expected, "{prelude}");
        }
        let empty = container(&[], true, 0.0, 0.0);
        assert!(!holds("@container (width)", &[&empty]));
        assert!(holds("@container (orientation: portrait)", &[&empty]));
    }

    #[test]
    fn the_nearest_eligible_named_container_answers() {
        let inline = container(&["card"], false, 120.0, 0.0);
        let outer = container(&["page", "card"], true, 500.0, 40.0);
        assert!(holds("@container (max-width: 200px)", &[&inline, &outer]));
        // An inline-size container cannot answer a block-axis query: the next one does.
        assert!(holds("@container (height: 40px)", &[&inline, &outer]));
        assert!(holds(
            "@container page (min-width: 400px)",
            &[&inline, &outer]
        ));
        assert!(!holds(
            "@container card (min-width: 400px)",
            &[&inline, &outer]
        ));
        assert!(!holds(
            "@container other (min-width: 1px)",
            &[&inline, &outer]
        ));
        assert!(!holds("@container (min-width: 1px)", &[]));
        assert!(!holds("@container (height: 0px)", &[&inline]));
    }

    #[test]
    fn unsupported_queries_are_refused() {
        for prelude in [
            "@container style(--x: 1)",
            "@container (min-width: 10em)",
            "@container (orientation: sideways)",
            "@container (resolution: 2dppx)",
            "@container card",
            "@media screen",
            "@container (",
        ] {
            assert!(parse(prelude).is_err(), "{prelude}");
        }
    }
}
