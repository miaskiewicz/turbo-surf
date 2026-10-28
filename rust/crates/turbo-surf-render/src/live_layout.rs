//! Native live-tree input for the public layout/provenance seam: turbo-dom-layout's
//! live-DOM input (`turbo_dom_layout::live`) styles and lays out the page's own tree
//! as surf composes it. No serializer, guest properties or identity attributes
//! participate, and the tree is never copied.
use turbo_dom_layout::layout_snapshot::{LayoutSnapshot, SnapshotInputs};
use turbo_dom_layout::live::{LiveCapture, LiveOptions, LiveStyle, LiveTree, OriginRules};
use turbo_dom_layout::style::CascadeOrigin;
use turbo_dom_layout::Diagnostics;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;

pub(crate) use turbo_dom_layout::live::source_id;

pub(crate) struct LiveTreeInput {
    /// Identities, SVG viewports, original text, quirks mode, `document.body` and the
    /// tree version, recorded under the source budgets at capture time.
    pub(crate) capture: LiveCapture,
    pub(crate) composition: Option<crate::live_layout_composed::Composition>,
}

impl LiveTreeInput {
    /// The inert input of a disconnected or non-browsing-context document.
    pub(crate) fn empty(document_id: u32, tree_version: u64) -> Self {
        Self {
            capture: LiveCapture::empty(document_id, tree_version),
            composition: None,
        }
    }
    pub(crate) fn document_id(&self) -> u32 {
        self.capture.document
    }
}

/// A light-DOM composition of `tree` (every node's composed children are its tree
/// children) with the per-scope UA rules: the test-only stand-in for the
/// browsing-context capture in `live_layout_dom`.
#[cfg(test)]
pub(crate) fn light_composition(
    tree: &Tree,
    document_id: u32,
) -> Result<crate::live_layout_composed::Composition, String> {
    let mut graph = crate::live_layout_composed::Composition::scopes(tree)?;
    let mut pending = vec![tree.root()];
    while let Some(node) = pending.pop() {
        let children = tree.children(node);
        for &child in &children {
            graph.parents.insert(child, node);
        }
        pending.extend(children.iter().copied());
        graph.children.insert(node, children);
    }
    graph.ua_rules = crate::live_layout_scoped::ua_rules(tree, document_id, &graph)?;
    Ok(graph)
}

/// Adapted@S4: the non-composed test capture is the composed capture of the light
/// tree (`light_composition`).
#[cfg(test)]
pub(crate) fn capture_tree(tree: &Tree, document_id: u32) -> Result<LiveTreeInput, String> {
    capture_composed_tree(tree, document_id, light_composition(tree, document_id)?)
}

/// Capture the composed tree's layout provenance under the source budgets.
pub(crate) fn capture_composed_tree(
    tree: &Tree,
    document_id: u32,
    composition: crate::live_layout_composed::Composition,
) -> Result<LiveTreeInput, String> {
    let children = |node: Handle| composition.children_of(node);
    let capture = turbo_dom_layout::live::capture(&LiveTree {
        tree,
        document: document_id,
        children: &children,
    })
    .map_err(|error| error.to_string())?;
    Ok(LiveTreeInput {
        capture,
        composition: Some(composition),
    })
}

// This is a UA-origin supplement, not author CSS or a DOM rewrite. Universal
// author rules must be able to override it regardless of selector specificity.
pub(crate) const LIVE_UA: &str = concat!(
    "body{margin:8px;font-family:serif}",
    "button{display:inline-block;font-family:Arial;font-size:13.3333px;font-style:normal;font-weight:400;font-stretch:normal;font-variant-caps:normal;line-height:normal;margin:0;padding:1px 6px;border-width:2px;vertical-align:baseline}",
    "iframe{display:inline;border:2px inset}",
    "head,style,script,title,meta,link,base,noscript{display:none}",
);

/// The continuous-media viewport this thread's layout resolves `vw`/`vh` and
/// layout-side media against (the live CSS was already resolved by surf).
fn live_viewport(width: f32, height: f32) {
    turbo_dom_layout::set_media_viewport_height(height);
    turbo_dom_layout::set_media_viewport_width(width);
}

/// The prepared rules of `css` in the document scope, as an author stylesheet.
#[cfg(test)]
pub(crate) fn test_rules(
    tree: &Tree,
    input: &LiveTreeInput,
    css: &str,
) -> Result<Vec<crate::live_stylesheets::ActiveRule>, String> {
    use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
    let graph = input
        .composition
        .as_ref()
        .ok_or("test input has no composition")?;
    let parsed = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|error| format!("test CSS parse failed: {error:?}"))?;
    let mut rules = Vec::new();
    for rule in &parsed.rules.0 {
        let text = lightningcss::traits::ToCss::to_css_string(rule, PrinterOptions::default())
            .map_err(|error| format!("test CSS serialization failed: {error:?}"))?;
        let mut sheet = crate::live_stylesheets::SheetSnapshot {
            cycle: false,
            disabled: false,
            media: String::new(),
            rules: vec![crate::live_stylesheets::RuleSnapshot {
                kind: if matches!(rule, lightningcss::rules::CssRule::Style(_)) {
                    1
                } else {
                    0
                },
                css_text: text.clone(),
                prelude: None,
                rules: None,
                media: String::new(),
                style_sheet: None,
                scoped: None,
            }],
        };
        crate::live_layout_scoped::prepare_sheet(
            tree,
            input.document_id(),
            tree.root(),
            graph,
            &mut sheet,
        )?;
        rules.push(crate::live_stylesheets::ActiveRule {
            text,
            scoped: sheet.rules.remove(0).scoped,
            resource: None,
        });
    }
    Ok(rules)
}

#[cfg(test)]
pub(crate) fn layout(
    tree: &Tree,
    input: &LiveTreeInput,
    css: &str,
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<LayoutSnapshot, String> {
    let rules = test_rules(tree, input, css)?;
    Ok(layout_active_rules_resolved(tree, input, &rules, 2.0, inputs, diagnostics)?.0)
}

/// Build each qualified active rule independently to preserve CSSOM source order
/// across imports and conditional blocks; the legacy PDF at-rule side list is
/// deliberately not used by this live API.
#[cfg(test)]
pub(crate) fn layout_rules(
    tree: &Tree,
    input: &LiveTreeInput,
    rules: &[String],
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<LayoutSnapshot, String> {
    Ok(layout_rules_resolved(tree, input, rules, 2.0, inputs, diagnostics)?.0)
}

#[cfg(test)]
pub(crate) fn layout_rules_resolved(
    tree: &Tree,
    input: &LiveTreeInput,
    rules: &[String],
    layout_scale: f32,
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<
    (
        LayoutSnapshot,
        turbo_dom_layout::cssom_view::scroll::SourceStyles,
    ),
    String,
> {
    let mut active = Vec::new();
    for text in rules {
        active.extend(test_rules(tree, input, text)?);
    }
    layout_active_rules_resolved(tree, input, &active, layout_scale, inputs, diagnostics)
}

pub(crate) fn layout_active_rules_resolved(
    tree: &Tree,
    input: &LiveTreeInput,
    rules: &[crate::live_stylesheets::ActiveRule],
    layout_scale: f32,
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<
    (
        LayoutSnapshot,
        turbo_dom_layout::cssom_view::scroll::SourceStyles,
    ),
    String,
> {
    crate::live_layout_neutral::validate(tree, input, rules)?;
    let qualified = rules
        .iter()
        .filter(|rule| rule.resource.is_none())
        .collect::<Vec<_>>();
    live_viewport(inputs.width, inputs.height);
    for rule in &qualified {
        if !turbo_dom_layout::style::parse_stylesheet(&rule.text)
            .at_rules
            .is_empty()
        {
            return Err(
                "native live layout requires privately resolved conditional CSS rules".into(),
            );
        }
    }
    let graph = input
        .composition
        .as_ref()
        .ok_or("native composed style graph unavailable")?;
    let origins = [
        OriginRules {
            origin: CascadeOrigin::UserAgent,
            rules: graph
                .ua_rules
                .iter()
                .map(|rule| rule.scoped.as_ref())
                .collect(),
        },
        OriginRules {
            origin: CascadeOrigin::Author,
            rules: qualified.iter().map(|rule| rule.scoped.as_ref()).collect(),
        },
    ];
    let children = |node: Handle| graph.children_of(node);
    let scope_depth = |node: Handle| {
        let root = graph
            .scopes
            .get(&node)
            .ok_or("native composed source scope unavailable")?;
        graph
            .depths
            .get(root)
            .copied()
            .ok_or_else(|| "native composed source depth unavailable".to_string())
    };
    let inline_override = |node: Handle| graph.inline_overrides.get(&node).map(String::as_str);
    let style = LiveStyle {
        tree: LiveTree {
            tree,
            document: input.document_id(),
            children: &children,
        },
        origins: &origins,
        scope_depth: &scope_depth,
        inline_override: &inline_override,
        inline_declarations: &crate::live_layout_scoped::inline_declarations,
    };
    let document = input.document_id();
    turbo_dom_layout::live::snapshot(
        &style,
        &input.capture,
        &LiveOptions { layout_scale },
        |styled| {
            let styles = crate::live_layout_zoom::apply(styled, document);
            crate::live_layout_resources::validate(styled, rules)?;
            Ok(styles)
        },
        inputs,
        diagnostics,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbo_dom_layout::layout_snapshot::{GeometryKind, SourceId};
    use turbo_dom_layout::live::{MAX_DEPTH, MAX_TEXT_BYTES};
    use turbo_dom_layout::{FontRegistry, ImageCtx, NoImages};

    // Adapted@S4: every test layout goes through the composed live path (rules
    // prepared by surf's scoped matcher), as production layout does.
    fn snapshot(tree: &Tree, id: u32, css: &str) -> LayoutSnapshot {
        let input = capture_tree(tree, id).unwrap();
        let fonts = FontRegistry::new();
        let images = ImageCtx {
            resolver: &NoImages,
            body_height: None,
        };
        layout(
            tree,
            &input,
            css,
            &SnapshotInputs {
                width: 200.0,
                height: 200.0,
                fonts: &fonts,
                images: &images,
            },
            &mut Diagnostics::default(),
        )
        .unwrap()
    }

    fn border(
        snapshot: &LayoutSnapshot,
        id: SourceId,
    ) -> &turbo_dom_layout::layout_snapshot::SnapshotFragment {
        snapshot
            .source(id)
            .unwrap()
            .fragment_indices
            .iter()
            .map(|&i| &snapshot.fragments()[i])
            .find(|f| f.kind == GeometryKind::BorderBox)
            .unwrap()
    }

    #[test]
    fn actual_tree_ids_attributes_and_script_style_nodes_survive_the_native_capture() {
        let tree = Tree::parse("<!doctype html><html><head><style>div{width:40px;height:20px}</style><script>let x=1;</script></head><body><!-- hi --><div data-cid='fake'>hello</div></body></html>");
        let input = capture_tree(&tree, 7).unwrap();
        assert_eq!(input.document_id(), 7);
        assert_eq!(input.capture.tree_version, tree.version);
        // The doctype is not a source: <html> is the first kept child.
        let html = tree.query_selector("html").unwrap();
        assert!(input
            .capture
            .identities
            .iter()
            .any(|identity| identity.id == source_id(7, html) && identity.path == [0]));
        let div = tree.query_selector("div").unwrap();
        let snap = snapshot(&tree, 7, "div{width:40px;height:20px}");
        assert_eq!(border(&snap, source_id(7, div)).rect.width, 40.0);
        let style = tree.query_selector("style").unwrap();
        let source = snap.source(source_id(7, style)).unwrap();
        assert!(source.fragment_indices.is_empty());
        assert_eq!(tree.get_attribute(div, "data-cid"), Some("fake"));
        assert_eq!(tree.get_attribute(div, "style"), None);
    }

    #[test]
    fn mutations_move_the_same_source_id_and_snapshot_version_changes() {
        let mut tree = Tree::parse("<html><body style='margin:0'><div id='a' style='width:40px;height:20px'></div><div id='b' style='height:30px'></div></body></html>");
        let first = tree.query_selector("#a").unwrap();
        let second = tree.query_selector("#b").unwrap();
        let body = tree.query_selector("body").unwrap();
        let before = capture_tree(&tree, 8).unwrap();
        tree.append_child(body, first);
        tree.set_attribute(second, "style", "height:50px");
        let after = capture_tree(&tree, 8).unwrap();
        assert!(after.capture.tree_version > before.capture.tree_version);
        let id = source_id(8, first);
        let old_path = before
            .capture
            .identities
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .path
            .clone();
        let new_path = after
            .capture
            .identities
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .path
            .clone();
        assert_ne!(old_path, new_path);
        let snapshot = snapshot(&tree, 8, "");
        assert_eq!(border(&snapshot, id).rect.y, 50.0);
    }

    #[test]
    fn ua_origin_does_not_outrank_universal_author_css_and_scripts_are_not_stripped() {
        let tree = Tree::parse("<html><head><script style='height:10px'>x</script></head><body><div style='height:20px'></div></body></html>");
        let body = tree.query_selector("body").unwrap();
        let default = snapshot(&tree, 1, "");
        assert_eq!(border(&default, source_id(1, body)).rect.x, 8.0);
        let reset = snapshot(
            &tree,
            1,
            "*{margin:0}head,script{display:block}script{font-size:0;line-height:0}",
        );
        assert_eq!(border(&reset, source_id(1, body)).rect.x, 0.0);
        let script = tree.query_selector("script").unwrap();
        assert_eq!(border(&reset, source_id(1, script)).rect.height, 10.0);
    }

    #[test]
    fn separate_documents_never_reuse_one_source_namespace() {
        let tree = Tree::parse("<div></div>");
        let node = tree.query_selector("div").unwrap();
        assert_ne!(source_id(1, node), source_id(2, node));
        assert_eq!(source_id(1, node), source_id(1, node));
    }

    #[test]
    fn ordered_live_css_rules_do_not_move_conditionals_after_following_rules() {
        let tree = Tree::parse("<html><body style='margin:0'><div id=x></div></body></html>");
        let input = capture_tree(&tree, 1).unwrap();
        let fonts = FontRegistry::new();
        let images = ImageCtx {
            resolver: &NoImages,
            body_height: None,
        };
        let inputs = SnapshotInputs {
            width: 200.0,
            height: 200.0,
            fonts: &fonts,
            images: &images,
        };
        let rules = vec![
            "#x{width:10px;height:20px}".into(),
            "#x{width:30px}".into(),
            "#x{width:40px}".into(),
        ];
        let snapshot =
            layout_rules(&tree, &input, &rules, &inputs, &mut Diagnostics::default()).unwrap();
        assert_eq!(
            border(&snapshot, source_id(1, tree.query_selector("#x").unwrap()))
                .rect
                .width,
            40.0
        );
        assert!(layout_rules(
            &tree,
            &input,
            &["@media screen {#x{width:70px}}".into()],
            &inputs,
            &mut Diagnostics::default()
        )
        .err()
        .unwrap()
        .contains("privately resolved"));
    }

    #[test]
    fn source_capture_refuses_unimplemented_namespace_and_excess_depth_or_text() {
        let tree = Tree::parse("<math><mi>x</mi></math>");
        assert!(capture_tree(&tree, 1).err().unwrap().contains("namespace"));
        let html = format!(
            "{}x{}",
            "<div>".repeat(MAX_DEPTH + 2),
            "</div>".repeat(MAX_DEPTH + 2)
        );
        // Adapted@S4: like a browsing-context capture, the test capture builds the
        // scope graph first, and its depth budget reports before the source walk's
        // (live_layout_budget_tests pins the walk's own depth error).
        assert_eq!(
            capture_tree(&Tree::parse(&html), 1).err().unwrap(),
            "native layout scope graph exceeds source budget"
        );
        let html = format!("<div>{}</div>", "x".repeat(MAX_TEXT_BYTES + 1));
        assert!(capture_tree(&Tree::parse(&html), 1)
            .err()
            .unwrap()
            .contains("text budget"));
    }

    /// Stays in surf at S4 (its module moved to turbo-dom-layout's cssom_view): the
    /// Chrome control is turbo-surf's harness fixture.
    #[test]
    fn scroll_extent_uses_actual_overflow_padding_margins_transforms_and_nested_clips() {
        use turbo_dom_layout::cssom_view::scroll::scroll_metrics;
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
                &tree,
                &input,
                &[],
                2.0,
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
