//! Scrollable overflow from real native border fragments and resolved styles.
//! Clipping and scroll state remain separate from immutable layout provenance.
use crate::live_layout::LiveTreeInput;
use std::collections::BTreeMap;
use turbo_html2pdf_core::layout_snapshot::{GeometryKind, LayoutSnapshot, SourceId};
use turbo_html2pdf_core::{ComputedStyle, StyledNode};

pub(crate) struct SourceStyle {
    pub(crate) style: ComputedStyle,
    pub(crate) parent: Option<SourceId>,
    pub(crate) tag: String,
}
pub(crate) type SourceStyles = BTreeMap<SourceId, SourceStyle>;

pub(crate) fn collect_styles(input: &LiveTreeInput, styled: &[StyledNode]) -> SourceStyles {
    let identities: BTreeMap<_, _> = input
        .identities
        .iter()
        .map(|identity| (identity.path.as_slice(), identity.id))
        .collect();
    fn walk(
        nodes: &[StyledNode],
        path: &mut Vec<usize>,
        parent: Option<SourceId>,
        ids: &BTreeMap<&[usize], SourceId>,
        out: &mut SourceStyles,
    ) {
        for (i, node) in nodes.iter().enumerate() {
            path.push(i);
            let id = ids.get(path.as_slice()).copied();
            if let StyledNode::Element(element) = node {
                if let Some(id) = id {
                    out.insert(
                        id,
                        SourceStyle {
                            style: element.style.clone(),
                            parent,
                            tag: match &element.tag {
                                turbo_html2pdf_core::Tag::Html(tag) => tag.clone(),
                                _ => String::new(),
                            },
                        },
                    );
                }
                walk(&element.children, path, id.or(parent), ids, out);
            }
            path.pop();
        }
    }
    let mut out = BTreeMap::new();
    walk(styled, &mut Vec::new(), None, &identities, &mut out);
    out
}
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) struct ScrollMetrics {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) scroll_x: bool,
    pub(crate) scroll_y: bool,
    pub(crate) max_x: f64,
    pub(crate) max_y: f64,
}
fn overflow_value(value: &str) -> Option<&'static str> {
    ["visible", "clip", "hidden", "scroll", "auto"]
        .into_iter()
        .find(|keyword| value.eq_ignore_ascii_case(keyword))
}
fn raw_overflow(style: &ComputedStyle, axis: &str) -> &'static str {
    if let Some(value) = style.get(axis).and_then(overflow_value) {
        return value;
    }
    let mut shorthand = style
        .get("overflow")
        .unwrap_or("visible")
        .split_ascii_whitespace();
    let first = shorthand
        .next()
        .and_then(overflow_value)
        .unwrap_or("visible");
    if axis == "overflow-y" {
        shorthand.next().and_then(overflow_value).unwrap_or(first)
    } else {
        first
    }
}
pub(crate) fn overflow(style: &ComputedStyle, axis: &str) -> &'static str {
    let value = raw_overflow(style, axis);
    let other = raw_overflow(
        style,
        if axis == "overflow-x" {
            "overflow-y"
        } else {
            "overflow-x"
        },
    );
    if !matches!(other, "visible" | "clip") {
        match value {
            "visible" => "auto",
            "clip" => "hidden",
            _ => value,
        }
    } else {
        value
    }
}
fn permits_scroll(value: &str) -> bool {
    matches!(value, "auto" | "scroll" | "hidden")
}
fn clips(value: &str) -> bool {
    value != "visible"
}
fn rounded(value: f32) -> Result<i32, String> {
    if !value.is_finite() || value < 0.0 || f64::from(value) > f64::from(i32::MAX) {
        return Err("native scroll extent exceeds integer range".into());
    }
    Ok(value.round() as i32)
}
pub(crate) fn scroll_metrics(
    snapshot: &LayoutSnapshot,
    styles: &SourceStyles,
    id: SourceId,
) -> Result<ScrollMetrics, String> {
    let source = snapshot.source(id).ok_or("native scroll source missing")?;
    if source.excluded_from_layout {
        return Ok(ScrollMetrics {
            width: 0,
            height: 0,
            scroll_x: false,
            scroll_y: false,
            max_x: 0.0,
            max_y: 0.0,
        });
    }
    let first = source
        .fragment_indices
        .iter()
        .map(|&i| &snapshot.fragments()[i])
        .find(|fragment| fragment.kind == GeometryKind::BorderBox)
        .ok_or("native scroll requires inline border provenance")?;
    let used = first.used_box.ok_or("native scroll used box missing")?;
    let padding = used.padding_rect(first.rect);
    let own = styles
        .get(&id)
        .ok_or("native scroll computed style missing")?;
    let x = overflow(&own.style, "overflow-x");
    let y = overflow(&own.style, "overflow-y");
    if own.style.get("direction") == Some("rtl")
        || own
            .style
            .get("writing-mode")
            .is_some_and(|v| v != "horizontal-tb")
    {
        return Err("native scroll RTL/vertical writing provenance pending".into());
    }
    let matrix = first.world_transform.map(f64::from);
    let determinant = matrix[0] * matrix[3] - matrix[1] * matrix[2];
    if determinant == 0.0 || !determinant.is_finite() {
        return Err(
            "native scroll degenerate-container relative transform provenance pending".into(),
        );
    }
    let projected = |fragment: &turbo_html2pdf_core::layout_snapshot::SnapshotFragment| {
        let points = fragment.quad.map(|point| {
            let x = f64::from(point.x) - matrix[4];
            let y = f64::from(point.y) - matrix[5];
            [
                ((matrix[3] * x - matrix[2] * y) / determinant) as f32,
                ((-matrix[1] * x + matrix[0] * y) / determinant) as f32,
            ]
        });
        [
            points
                .iter()
                .map(|point| point[0])
                .fold(f32::NEG_INFINITY, f32::max),
            points
                .iter()
                .map(|point| point[1])
                .fold(f32::NEG_INFINITY, f32::max),
        ]
    };
    let mut right = padding.x + padding.width;
    let mut bottom = padding.y + padding.height;
    for &index in &source.descendant_fragment_indices {
        let fragment = &snapshot.fragments()[index];
        if fragment.kind != GeometryKind::BorderBox {
            continue;
        }
        // A nested clipping ancestor owns its descendants' overflow. Its own
        // border still contributes to the outer scroll area's extent.
        let mut blocked_x = false;
        let mut blocked_y = false;
        for ancestor in &fragment.source_ancestors {
            if *ancestor == id {
                blocked_x = false;
                blocked_y = false;
                continue;
            }
            if *ancestor == fragment.source_id.unwrap_or(id) {
                continue;
            }
            if let Some(ancestor) = styles.get(ancestor) {
                blocked_x |= clips(overflow(&ancestor.style, "overflow-x"));
                blocked_y |= clips(overflow(&ancestor.style, "overflow-y"));
            }
        }
        let transformed = projected(fragment);
        let rect = fragment.rect;
        if !blocked_x {
            right = right.max(transformed[0]).max(rect.x + rect.width);
        }
        if !blocked_y {
            bottom = bottom.max(transformed[1]).max(rect.y + rect.height);
        }
        // Flow content's ending padding/margin contributes to scroll containers,
        // while out-of-flow boxes and transforms contribute only their edges.
        if let Some(metrics) = fragment.used_box {
            if metrics.position != turbo_html2pdf_core::layout::value::Position::Absolute
                && metrics.position != turbo_html2pdf_core::layout::value::Position::Fixed
            {
                if !blocked_x && permits_scroll(x) {
                    right =
                        right.max(rect.x + rect.width + metrics.margin.right + used.padding.right);
                }
                if !blocked_y && permits_scroll(y) {
                    bottom = bottom
                        .max(rect.y + rect.height + metrics.margin.bottom + used.padding.bottom);
                }
            }
        }
    }
    Ok(ScrollMetrics {
        width: rounded(right - padding.x)?,
        height: rounded(bottom - padding.y)?,
        scroll_x: permits_scroll(x),
        scroll_y: permits_scroll(y),
        max_x: f64::from((right - padding.x - padding.width).max(0.0)),
        max_y: f64::from((bottom - padding.y - padding.height).max(0.0)),
    })
}

pub(crate) fn scroll_translation(
    snapshot: &LayoutSnapshot,
    styles: &SourceStyles,
    id: SourceId,
    offsets: &BTreeMap<SourceId, [f64; 2]>,
) -> Result<[f32; 2], String> {
    let source = snapshot.source(id).ok_or("native scroll source missing")?;
    if source.excluded_from_layout {
        return Ok([0.0; 2]);
    }
    let fragment = source
        .fragment_indices
        .iter()
        .map(|&i| &snapshot.fragments()[i])
        .find(|fragment| fragment.kind == GeometryKind::BorderBox)
        .ok_or("native scroll requires inline border provenance")?;
    let position = fragment
        .used_box
        .ok_or("native scroll used box missing")?
        .position;
    use turbo_html2pdf_core::layout::value::Position;
    if position == Position::Fixed {
        return Ok([0.0; 2]);
    }
    let containing = if position == Position::Absolute {
        fragment.source_ancestors.iter().rposition(|ancestor| {
            styles.get(ancestor).is_some_and(|style| {
                style
                    .style
                    .get("position")
                    .is_some_and(|value| value != "static")
            })
        })
    } else {
        Some(fragment.source_ancestors.len())
    };
    let mut translation = [0.0; 2];
    for (i, ancestor) in fragment.source_ancestors.iter().enumerate() {
        if containing.is_none_or(|index| i > index) {
            continue;
        }
        let Some(offset) = offsets.get(ancestor) else {
            continue;
        };
        if offset == &[0.0; 2] {
            continue;
        }
        let ancestor = snapshot
            .source(*ancestor)
            .ok_or("native scroll ancestor source missing")?;
        let box_ = ancestor
            .fragment_indices
            .iter()
            .map(|&index| &snapshot.fragments()[index])
            .find(|fragment| fragment.kind == GeometryKind::BorderBox)
            .ok_or("native scroll ancestor box missing")?;
        let matrix = box_.world_transform;
        translation[0] += matrix[0] * offset[0] as f32 + matrix[2] * offset[1] as f32;
        translation[1] += matrix[1] * offset[0] as f32 + matrix[3] * offset[1] as f32;
    }
    Ok(translation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_layout::{capture_tree, layout_rules_resolved, source_id};
    use turbo_dom_parser::rtdom::Tree;
    use turbo_html2pdf_core::layout_snapshot::SnapshotInputs;
    use turbo_html2pdf_core::{Diagnostics, FontRegistry, ImageCtx, NoImages};
    #[test]
    fn scroll_extent_uses_actual_overflow_padding_margins_transforms_and_nested_clips() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../harness/detection/fixtures/scroll-layout-chrome155.json"
        ))
        .unwrap();
        let cases = [
            (
                "padded",
                "padding:5px;border:2px solid;overflow:auto",
                "width:180px;height:140px",
            ),
            (
                "visible",
                "padding:5px;border:2px solid;overflow:visible",
                "width:180px;height:140px",
            ),
            (
                "hidden",
                "padding:5px;border:2px solid;overflow:hidden",
                "width:180px;height:140px",
            ),
            (
                "negative",
                "padding:5px;border:2px solid;position:relative;overflow:auto",
                "position:absolute;left:-50px;top:-40px;width:20px;height:20px",
            ),
            (
                "absolute",
                "padding:5px;border:2px solid;position:relative;overflow:auto",
                "position:absolute;left:120px;top:90px;width:30px;height:20px",
            ),
            (
                "transform",
                "padding:5px;border:2px solid;overflow:auto",
                "width:50px;height:30px;transform:translate(120px,90px)",
            ),
            (
                "margin",
                "padding:5px;border:2px solid;overflow:auto",
                "width:180px;height:140px;margin:10px",
            ),
            ("scaledContainer","padding:5px;border:2px solid;overflow:auto;transform:scale(2);transform-origin:0 0","width:180px;height:140px"),
            ("transformedNegative","padding:5px;border:2px solid;overflow:auto","width:180px;height:140px;transform:translate(-100px,-80px)"),
            (
                "nestedclip",
                "padding:5px;border:2px solid;overflow:auto",
                "width:50px;height:30px;overflow:hidden",
            ),
        ];
        let mut mismatches = Vec::new();
        for (index, (name, outer, inner)) in cases.into_iter().enumerate() {
            let nested = if name == "nestedclip" {
                "<div style='width:250px;height:220px'></div>"
            } else {
                ""
            };
            let tree=Tree::parse(&format!("<html><body style='margin:0'><div id=box style='width:100px;height:80px;{outer}'><div style='{inner}'>{nested}</div></div></body></html>"));
            let input = capture_tree(&tree, 1).unwrap();
            let fonts = FontRegistry::new();
            let images = ImageCtx {
                resolver: &NoImages,
                body_height: None,
            };
            let (snapshot, styles) = layout_rules_resolved(
                &input,
                &[],
                &SnapshotInputs {
                    width: 800.0,
                    height: 600.0,
                    fonts: &fonts,
                    images: &images,
                },
                &mut Diagnostics::default(),
            )
            .unwrap();
            let actual = scroll_metrics(
                &snapshot,
                &styles,
                source_id(1, tree.query_selector("#box").unwrap()),
            )
            .unwrap();
            let expected = &oracle["result"][index]["before"];
            let expected = (
                expected["width"].as_i64().unwrap() as i32,
                expected["height"].as_i64().unwrap() as i32,
            );
            if (actual.width, actual.height) != expected {
                mismatches.push(format!(
                    "{name}: native={:?} Chrome={expected:?}",
                    (actual.width, actual.height)
                ));
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("; "));
    }
}
