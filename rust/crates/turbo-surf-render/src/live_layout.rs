//! Native live-tree input for the public layout/provenance seam. No serializer,
//! selectors, guest properties or identity attributes participate in this copy.
use std::collections::HashMap;
use turbo_dom_layout::layout_snapshot::{LayoutSnapshot, SnapshotInputs, SourceId, SourceIdentity};
use turbo_dom_layout::style::LeveledRule;
use turbo_dom_layout::{Attr, Cascade, Diagnostics, Element, Node, Tag};
use turbo_dom_parser::rtdom::tree::{Handle, NodeType};
use turbo_dom_parser::rtdom::{NodeRef, Tree};

const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 256;
const MAX_TEXT_BYTES: usize = 2_000_000;

pub(crate) struct LiveTreeInput {
    pub(crate) document_id: u32,
    pub(crate) tree_version: u64,
    pub(crate) nodes: Vec<Node>,
    pub(crate) identities: Vec<SourceIdentity>,
    pub(crate) svg_viewports: Vec<SourceId>,
    pub(crate) original_text: HashMap<SourceId, Vec<u16>>,
    pub(crate) composition: Option<crate::live_layout_composed::Composition>,
    /// BackCompat documents address viewport metrics through their body.
    pub(crate) quirks: bool,
    /// The document's body element (`document.body`), when it is an HTML body.
    pub(crate) body: Option<SourceId>,
}

pub(crate) fn source_id(document_id: u32, handle: Handle) -> SourceId {
    SourceId((u64::from(document_id) << 32) | u64::from(handle.raw()))
}

#[cfg(test)]
pub(crate) fn capture_tree(tree: &Tree, document_id: u32) -> Result<LiveTreeInput, String> {
    capture_composed_tree(tree, document_id, None)
}

pub(crate) fn capture_composed_tree(
    tree: &Tree,
    document_id: u32,
    composition: Option<crate::live_layout_composed::Composition>,
) -> Result<LiveTreeInput, String> {
    let mut copy = Copier {
        composition: composition.as_ref(),
        document_id,
        identities: Vec::new(),
        svg_viewports: Vec::new(),
        original_text: HashMap::new(),
        nodes_left: MAX_NODES,
        text_left: MAX_TEXT_BYTES,
        attributes_left: MAX_TEXT_BYTES,
    };
    let nodes = copy.children(NodeRef::new(tree, tree.root()), &[], false)?;
    Ok(LiveTreeInput {
        document_id,
        tree_version: tree.version,
        nodes,
        identities: copy.identities,
        svg_viewports: copy.svg_viewports,
        original_text: copy.original_text,
        composition,
        quirks: tree.compat_mode() == "BackCompat",
        body: document_body(tree).map(|handle| source_id(document_id, handle)),
    })
}

/// `document.body`: the first body or frameset child of the HTML root element.
fn document_body(tree: &Tree) -> Option<Handle> {
    const HTML: Option<&str> = Some("http://www.w3.org/1999/xhtml");
    let html = NodeRef::new(tree, tree.root())
        .children()
        .into_iter()
        .find(|node| node.node_type() == NodeType::Element)
        .filter(|node| {
            node.local_name() == Some("html") && tree.namespace_uri(node.handle()) == HTML
        })?;
    html.children()
        .into_iter()
        .find(|node| {
            node.node_type() == NodeType::Element
                && matches!(node.local_name(), Some("body" | "frameset"))
                && tree.namespace_uri(node.handle()) == HTML
        })
        .filter(|node| node.local_name() == Some("body"))
        .map(|node| node.handle())
}

struct Copier<'a> {
    composition: Option<&'a crate::live_layout_composed::Composition>,
    document_id: u32,
    identities: Vec<SourceIdentity>,
    svg_viewports: Vec<SourceId>,
    original_text: HashMap<SourceId, Vec<u16>>,
    nodes_left: usize,
    text_left: usize,
    attributes_left: usize,
}

impl Copier<'_> {
    fn children(
        &mut self,
        parent: NodeRef,
        path: &[usize],
        in_svg: bool,
    ) -> Result<Vec<Node>, String> {
        if path.len() > MAX_DEPTH {
            return Err("native layout source exceeds depth budget".into());
        }
        let mut nodes = Vec::new();
        let children = match self.composition {
            Some(graph) => graph
                .children
                .get(&parent.handle())
                .cloned()
                .unwrap_or_default(),
            None => parent
                .children()
                .into_iter()
                .map(|node| node.handle())
                .collect(),
        };
        for handle in children {
            let child = NodeRef::new(parent.tree(), handle);
            let mut child_path = path.to_vec();
            child_path.push(nodes.len());
            if let Some(node) = self.node(child, &child_path, in_svg)? {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    fn node(
        &mut self,
        node: NodeRef,
        path: &[usize],
        in_svg: bool,
    ) -> Result<Option<Node>, String> {
        self.nodes_left = self
            .nodes_left
            .checked_sub(1)
            .ok_or("native layout source exceeds node budget")?;
        let value = match node.node_type() {
            NodeType::Element => Node::Element(self.element(node, path, in_svg)?),
            NodeType::Text => Node::Text(self.text(node)?),
            _ => return Ok(None), // Comments/doctype/PI have no CSS layout boxes.
        };
        self.identities.push(SourceIdentity {
            id: source_id(self.document_id, node.handle()),
            path: path.to_vec(),
        });
        Ok(Some(value))
    }

    fn element(&mut self, node: NodeRef, path: &[usize], in_svg: bool) -> Result<Element, String> {
        let namespace = node.tree().namespace_uri(node.handle());
        let svg = namespace == Some("http://www.w3.org/2000/svg");
        if namespace != Some("http://www.w3.org/1999/xhtml") && !svg {
            return Err("native live layout does not yet support this element namespace".into());
        }
        if svg && node.local_name() == Some("svg") && !in_svg {
            self.svg_viewports
                .push(source_id(self.document_id, node.handle()));
        }
        let mut attrs = Vec::new();
        for (name, value) in node.attributes() {
            let length = name
                .len()
                .checked_add(value.len())
                .ok_or("native layout attribute size overflow")?;
            self.attributes_left = self
                .attributes_left
                .checked_sub(length)
                .ok_or("native layout source exceeds attribute budget")?;
            attrs.push(Attr { name, value });
        }
        if let Some(applied) = self
            .composition
            .and_then(|graph| graph.inline_overrides.get(&node.handle()))
        {
            if let Some(attribute) = attrs.iter_mut().find(|attribute| attribute.name == "style") {
                attribute.value.clone_from(applied);
            }
        }
        Ok(Element {
            tag: Tag::Html(node.local_name().unwrap_or("").to_owned()),
            attrs,
            children: self.children(node, path, in_svg || svg)?,
        })
    }

    fn text(&mut self, node: NodeRef) -> Result<String, String> {
        let units = node
            .tree()
            .node_value_utf16(node.handle())
            .unwrap_or_default();
        let text = String::from_utf16_lossy(&units);
        self.original_text
            .insert(source_id(self.document_id, node.handle()), units);
        self.text_left = self
            .text_left
            .checked_sub(text.len())
            .ok_or("native layout source exceeds text budget")?;
        Ok(text)
    }
}

// This is a UA-origin supplement, not author CSS or a DOM rewrite. Universal
// author rules must be able to override it regardless of selector specificity.
pub(crate) const LIVE_UA: &str = concat!(
    "body{margin:8px;font-family:serif}",
    "button{display:inline-block;font-family:Arial;font-size:13.3333px;font-style:normal;font-weight:400;font-stretch:normal;font-variant-caps:normal;line-height:normal;margin:0;padding:1px 6px;border-width:2px;vertical-align:baseline}",
    "iframe{display:inline;border:2px inset}",
    "head,style,script,title,meta,link,base,noscript{display:none}",
);

pub(crate) fn live_cascade(css: &str, width: f32, height: f32) -> Cascade {
    turbo_dom_layout::set_media_viewport_height(height);
    let mut cascade =
        turbo_dom_layout::build_cascade_with_width(css, "", Default::default(), width);
    let start = cascade.rules.len();
    cascade.rules.extend(
        turbo_dom_layout::style::parse_stylesheet(LIVE_UA)
            .rules
            .into_iter()
            .enumerate()
            .map(|(i, rule)| LeveledRule {
                level: 0,
                order: start + i,
                rule,
            }),
    );
    cascade
}

#[cfg(test)]
pub(crate) fn layout(
    input: &LiveTreeInput,
    css: &str,
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<LayoutSnapshot, String> {
    let cascade = live_cascade(css, inputs.width, inputs.height);
    turbo_dom_layout::layout_snapshot::snapshot_live_tree(
        &input.nodes,
        &input.identities,
        &cascade,
        inputs,
        diagnostics,
    )
    .map_err(|error| error.to_string())
}

/// Build each qualified active rule independently to preserve CSSOM source order
/// across imports and conditional blocks; the legacy PDF at-rule side list is
/// deliberately not used by this live API.
#[cfg(test)]
pub(crate) fn layout_rules(
    input: &LiveTreeInput,
    rules: &[String],
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<LayoutSnapshot, String> {
    Ok(layout_rules_resolved(input, rules, 2.0, inputs, diagnostics)?.0)
}

#[cfg(test)]
pub(crate) fn layout_rules_resolved(
    input: &LiveTreeInput,
    rules: &[String],
    layout_scale: f32,
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<(LayoutSnapshot, crate::live_layout_scroll::SourceStyles), String> {
    let active = rules
        .iter()
        .map(|text| crate::live_stylesheets::ActiveRule {
            text: text.clone(),
            scoped: None,
            resource: None,
        })
        .collect::<Vec<_>>();
    layout_active_rules_resolved(input, &active, layout_scale, inputs, diagnostics)
}
pub(crate) fn layout_active_rules_resolved(
    input: &LiveTreeInput,
    rules: &[crate::live_stylesheets::ActiveRule],
    layout_scale: f32,
    inputs: &SnapshotInputs,
    diagnostics: &mut Diagnostics,
) -> Result<(LayoutSnapshot, crate::live_layout_scroll::SourceStyles), String> {
    crate::live_layout_neutral::validate(input, rules)?;
    let qualified = rules
        .iter()
        .filter(|rule| rule.resource.is_none())
        .cloned()
        .collect::<Vec<_>>();
    let mut cascade = live_cascade("", inputs.width, inputs.height);
    for rule in &qualified {
        let text = &rule.text;
        let sheet = turbo_dom_layout::style::parse_stylesheet(text);
        if !sheet.at_rules.is_empty() {
            return Err(
                "native live layout requires privately resolved conditional CSS rules".into(),
            );
        }
        for rule in sheet.rules {
            cascade.rules.push(LeveledRule {
                level: 1,
                order: cascade.rules.len(),
                rule,
            });
        }
    }
    let mut styled = if input.composition.is_some() {
        crate::live_layout_scoped::style_composed_tree(input, &qualified)?
    } else {
        turbo_dom_layout::style_tree(&input.nodes, &cascade)
    };
    crate::live_layout_zoom::apply(&mut styled);
    crate::live_layout_resources::validate(&styled, rules)?;
    let styles = crate::live_layout_scroll::collect_styles(input, &styled);
    let original: Vec<_> = input
        .original_text
        .iter()
        .map(
            |(source_id, units)| turbo_dom_layout::layout_snapshot::SourceTextUnits {
                source_id: *source_id,
                units,
            },
        )
        .collect();
    let options = turbo_dom_layout::layout_snapshot::NativeTextOptions {
        original_text: &original,
        svg_viewports: &input.svg_viewports,
        layout_scale,
    };
    let snapshot = turbo_dom_layout::layout_snapshot::snapshot_html_styled_tree_with_native_text(
        &styled,
        &input.identities,
        &options,
        inputs,
        diagnostics,
    )
    .map_err(|error| error.to_string())?;
    Ok((snapshot, styles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbo_dom_layout::layout_snapshot::GeometryKind;
    use turbo_dom_layout::{FontRegistry, ImageCtx, NoImages};

    fn snapshot(tree: &Tree, id: u32, css: &str) -> LayoutSnapshot {
        let input = capture_tree(tree, id).unwrap();
        let fonts = FontRegistry::new();
        let images = ImageCtx {
            resolver: &NoImages,
            body_height: None,
        };
        layout(
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
    fn actual_tree_ids_attributes_and_script_style_nodes_survive_the_native_copy() {
        let tree = Tree::parse("<!doctype html><html><head><style>div{width:40px;height:20px}</style><script>let x=1;</script></head><body><!-- hi --><div data-cid='fake'>hello</div></body></html>");
        let input = capture_tree(&tree, 7).unwrap();
        assert_eq!(input.document_id, 7);
        assert_eq!(input.tree_version, tree.version);
        assert_eq!(
            input.nodes[0].as_element().unwrap().tag,
            Tag::Html("html".into())
        );
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
        assert!(after.tree_version > before.tree_version);
        let id = source_id(8, first);
        let old_path = before
            .identities
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .path
            .clone();
        let new_path = after
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
        let snapshot = layout_rules(&input, &rules, &inputs, &mut Diagnostics::default()).unwrap();
        assert_eq!(
            border(&snapshot, source_id(1, tree.query_selector("#x").unwrap()))
                .rect
                .width,
            40.0
        );
        assert!(layout_rules(
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
    fn source_copy_refuses_unimplemented_namespace_and_excess_depth_or_text() {
        let tree = Tree::parse("<math><mi>x</mi></math>");
        assert!(capture_tree(&tree, 1).err().unwrap().contains("namespace"));
        let html = format!(
            "{}x{}",
            "<div>".repeat(MAX_DEPTH + 2),
            "</div>".repeat(MAX_DEPTH + 2)
        );
        assert!(capture_tree(&Tree::parse(&html), 1)
            .err()
            .unwrap()
            .contains("depth"));
        let html = format!("<div>{}</div>", "x".repeat(MAX_TEXT_BYTES + 1));
        assert!(capture_tree(&Tree::parse(&html), 1)
            .err()
            .unwrap()
            .contains("text budget"));
    }
}
