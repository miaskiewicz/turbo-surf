//! Hit testing native border fragments with actual transforms, clipping and
//! per-owner scroll projection. Text hit regions need native cluster provenance.
use crate::live_layout_scroll::{scroll_translation, SourceStyles};
use std::collections::{BTreeMap, BTreeSet};
use turbo_html2pdf_core::layout_snapshot::{GeometryKind, LayoutSnapshot, Rect, SourceId};
fn contains(rect: Rect, matrix: [f32; 6], translation: [f32; 2], point: [f64; 2]) -> bool {
    let [a, b, c, d, e, f] = matrix.map(f64::from);
    let determinant = a * d - b * c;
    if determinant == 0.0 || !determinant.is_finite() {
        return false;
    }
    let x = point[0] + f64::from(translation[0]) - e;
    let y = point[1] + f64::from(translation[1]) - f;
    let (x, y) = (
        (d * x - c * y) / determinant,
        (-b * x + a * y) / determinant,
    );
    x >= f64::from(rect.x)
        && x < f64::from(rect.x + rect.width)
        && y >= f64::from(rect.y)
        && y < f64::from(rect.y + rect.height)
}
fn property<'a>(styles: &'a SourceStyles, mut id: SourceId, name: &str) -> Option<&'a str> {
    loop {
        let source = styles.get(&id)?;
        if let Some(value) = source.style.get(name) {
            return Some(value);
        }
        id = source.parent?;
    }
}
fn clip_axis(style: &turbo_html2pdf_core::ComputedStyle, name: &str) -> bool {
    crate::live_layout_scroll::overflow(style, name) != "visible"
}
pub(crate) fn elements_at_point(
    snapshot: &LayoutSnapshot,
    styles: &SourceStyles,
    offsets: &BTreeMap<SourceId, [f64; 2]>,
    point: [f64; 2],
) -> Result<Vec<SourceId>, String> {
    let (width, height) = snapshot.viewport();
    if !point.iter().all(|v| v.is_finite())
        || point[0] < 0.0
        || point[1] < 0.0
        || point[0] >= f64::from(width)
        || point[1] >= f64::from(height)
    {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for fragment in snapshot.fragments().iter().rev() {
        if fragment.kind == GeometryKind::TextLineContainer
            && contains(fragment.rect, fragment.world_transform, [0.0; 2], point)
        {
            return Err("native text hit testing requires shaped cluster/source provenance".into());
        }
        if fragment.kind != GeometryKind::BorderBox {
            continue;
        }
        let Some(id) = fragment.source_id else {
            continue;
        };
        if seen.contains(&id)
            || property(styles, id, "pointer-events") == Some("none")
            || property(styles, id, "visibility").is_some_and(|value| value != "visible")
        {
            continue;
        }
        let translation = scroll_translation(snapshot, styles, id, offsets)?;
        if !contains(fragment.rect, fragment.world_transform, translation, point) {
            continue;
        }
        let mut clipped = false;
        for ancestor in &fragment.source_ancestors {
            let Some(style) = styles.get(ancestor) else {
                continue;
            };
            let clip_x = clip_axis(&style.style, "overflow-x");
            let clip_y = clip_axis(&style.style, "overflow-y");
            if !clip_x && !clip_y {
                continue;
            }
            if clip_x != clip_y {
                return Err("native hit testing asymmetric overflow clipping pending".into());
            }
            let source = snapshot
                .source(*ancestor)
                .ok_or("native hit clip source missing")?;
            let border = source
                .fragment_indices
                .iter()
                .map(|&index| &snapshot.fragments()[index])
                .find(|fragment| fragment.kind == GeometryKind::BorderBox)
                .ok_or("native hit clip border missing")?;
            let used = border.used_box.ok_or("native hit clip used box missing")?;
            let translation = scroll_translation(snapshot, styles, *ancestor, offsets)?;
            if !contains(
                used.padding_rect(border.rect),
                border.world_transform,
                translation,
                point,
            ) {
                clipped = true;
                break;
            }
        }
        if !clipped {
            seen.insert(id);
            result.push(id);
        }
    }
    if let Some((&root, _)) = styles
        .iter()
        .find(|(_, style)| style.parent.is_none() && style.tag == "html")
    {
        if seen.insert(root) {
            result.push(root);
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_inverse_transform_excludes_axis_bounds_and_degenerate_boxes() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 20.0,
        };
        assert!(contains(
            rect,
            [1.0, 0.0, 0.0, 1.0, 3.0, 4.0],
            [0.0; 2],
            [3.0, 4.0]
        ));
        assert!(!contains(
            rect,
            [1.0, 0.0, 0.0, 1.0, 3.0, 4.0],
            [0.0; 2],
            [13.0, 4.0]
        ));
        assert!(contains(
            rect,
            [0.0, 1.0, -1.0, 0.0, 30.0, 0.0],
            [0.0; 2],
            [25.0, 5.0]
        ));
        assert!(!contains(
            rect,
            [0.0, 1.0, -1.0, 0.0, 30.0, 0.0],
            [0.0; 2],
            [10.0, 5.0]
        ));
        assert!(!contains(rect, [0.0; 6], [0.0; 2], [0.0, 0.0]));
    }
}
