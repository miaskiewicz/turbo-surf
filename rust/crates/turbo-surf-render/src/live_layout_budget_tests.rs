//! Characterization of the native live-layout source budgets and their errors:
//! the node, depth, text, attribute and declaration budgets, the order in which
//! a document that breaks several of them reports, and the namespace rejection.
//! These pin the copy-free capture (turbo-dom-layout's live mode) to the exact
//! contract the live-tree copier had.
use crate::live_layout::{capture_composed_tree, layout_active_rules_resolved, LiveTreeInput};
use crate::live_layout_composed::Composition;
use crate::live_layout_scoped::{ScopedRule, Target};
use crate::live_stylesheets::ActiveRule;
use turbo_dom_layout::layout_snapshot::{GeometryKind, LayoutSnapshot, SnapshotInputs, SourceId};
use turbo_dom_layout::{Diagnostics, FontRegistry, ImageCtx, NoImages};
use turbo_dom_parser::rtdom::tree::{Handle, Namespace};
use turbo_dom_parser::rtdom::Tree;

const NODES: usize = 50_000;
const DEPTH: usize = 256;
const TEXT: usize = 2_000_000;
const ATTRIBUTES: usize = 2_000_000;
const DECLARATIONS: usize = 500_000;

const NODE_BUDGET: &str = "native layout source exceeds node budget";
const DEPTH_BUDGET: &str = "native layout source exceeds depth budget";
const TEXT_BUDGET: &str = "native layout source exceeds text budget";
const ATTRIBUTE_BUDGET: &str = "native layout source exceeds attribute budget";
const DECLARATION_BUDGET: &str = "native scoped declaration budget exceeded";
const NAMESPACE: &str = "native live layout does not yet support this element namespace";

const DOCUMENT: u32 = 1;

/// The composed graph of a light-only document: every node's composed children
/// are its tree children and every node is in the document scope. Built without
/// `Composition::scopes` so the capture's own budgets are what is measured.
fn graph(tree: &Tree, overrides: &[(Handle, &str)]) -> Composition {
    let mut graph = Composition::default();
    graph.depths.insert(tree.root(), 0);
    let mut pending = vec![tree.root()];
    while let Some(node) = pending.pop() {
        graph.scopes.insert(node, tree.root());
        let children = tree.children(node);
        for &child in &children {
            graph.parents.insert(child, node);
        }
        pending.extend(children.iter().copied());
        graph.children.insert(node, children);
    }
    graph.inline_overrides = overrides
        .iter()
        .map(|(node, text)| (*node, (*text).to_owned()))
        .collect();
    graph
}

fn capture(tree: &Tree) -> Result<LiveTreeInput, String> {
    capture_with(tree, &[])
}

fn capture_with(tree: &Tree, overrides: &[(Handle, &str)]) -> Result<LiveTreeInput, String> {
    capture_composed_tree(tree, DOCUMENT, graph(tree, overrides))
}

fn error(result: Result<LiveTreeInput, String>) -> String {
    result.err().expect("the capture must fail")
}

fn layout(
    tree: &Tree,
    input: &LiveTreeInput,
    rules: &[ActiveRule],
) -> Result<LayoutSnapshot, String> {
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
    layout_active_rules_resolved(
        tree,
        input,
        rules,
        1.0,
        &inputs,
        &mut Diagnostics::default(),
    )
    .map(|(snapshot, _)| snapshot)
}

fn body(tree: &Tree) -> Handle {
    tree.query_selector("body").unwrap()
}

fn id(handle: Handle) -> SourceId {
    crate::live_layout::source_id(DOCUMENT, handle)
}

/// A chain of `count` nested divs under `parent`; returns the innermost.
fn nest(tree: &mut Tree, mut parent: Handle, count: usize) -> Handle {
    for _ in 0..count {
        let div = tree.create_element("div");
        tree.append_child(parent, div);
        parent = div;
    }
    parent
}

fn text(tree: &mut Tree, parent: Handle, data: &str) -> Handle {
    let node = tree.create_text_node(data);
    tree.append_child(parent, node);
    node
}

/// `html`, `head` and `body` of an empty parsed document.
const EMPTY_DOCUMENT_NODES: usize = 3;

#[test]
fn node_budget_counts_comments_doctypes_and_processing_instructions_before_filtering() {
    let mut tree = Tree::parse("");
    assert_eq!(
        capture(&tree).unwrap().capture.identities.len(),
        EMPTY_DOCUMENT_NODES
    );
    let body = body(&tree);
    let doctype = tree.create_document_type("html", "", "");
    let html = tree.children(tree.root())[0];
    tree.insert_before(tree.root(), doctype, Some(html));
    let pi = tree.create_processing_instruction("x", "y");
    tree.append_child(body, pi);
    for _ in 0..NODES - EMPTY_DOCUMENT_NODES - 2 {
        let comment = tree.create_comment("");
        tree.append_child(body, comment);
    }
    // Exactly at the budget: only the three elements are layout sources.
    let input = capture(&tree).unwrap();
    assert_eq!(input.capture.identities.len(), EMPTY_DOCUMENT_NODES);
    let comment = tree.create_comment("");
    tree.append_child(body, comment);
    assert_eq!(error(capture(&tree)), NODE_BUDGET);
}

#[test]
fn node_budget_counts_text_and_elements_alike() {
    let mut tree = Tree::parse("");
    let body = body(&tree);
    for index in 0..NODES - EMPTY_DOCUMENT_NODES {
        if index % 2 == 0 {
            text(&mut tree, body, "");
        } else {
            let span = tree.create_element("span");
            tree.append_child(body, span);
        }
    }
    assert_eq!(capture(&tree).unwrap().capture.identities.len(), NODES);
    text(&mut tree, body, "");
    assert_eq!(error(capture(&tree)), NODE_BUDGET);
}

#[test]
fn depth_budget_allows_elements_at_depth_256_and_text_below_them() {
    let mut tree = Tree::parse("");
    // html is at depth 1 and body at depth 2.
    let body = body(&tree);
    let deepest = nest(&mut tree, body, DEPTH - 2);
    let leaf = text(&mut tree, deepest, "x");
    let input = capture(&tree).unwrap();
    let path = |node| {
        input
            .capture
            .identities
            .iter()
            .find(|identity| identity.id == id(node))
            .unwrap()
            .path
            .len()
    };
    assert_eq!(path(deepest), DEPTH);
    assert_eq!(path(leaf), DEPTH + 1);
    let too_deep = tree.create_element("i");
    tree.append_child(deepest, too_deep);
    assert_eq!(error(capture(&tree)), DEPTH_BUDGET);
}

#[test]
fn text_budget_is_measured_on_the_lossy_utf8_conversion() {
    let mut tree = Tree::parse("");
    let body = body(&tree);
    // Three bytes per character after conversion: a lone surrogate becomes U+FFFD.
    let lone = vec![0xD800u16; TEXT / 3];
    let node = tree.create_text_node_utf16(&lone);
    tree.append_child(body, node);
    let rest = TEXT - (TEXT / 3) * 3;
    text(&mut tree, body, &"é".repeat(rest / 2));
    text(&mut tree, body, &"x".repeat(rest % 2));
    let input = capture(&tree).unwrap();
    // The original code units survive alongside the scalar copy.
    assert_eq!(input.capture.original_text.get(&id(node)).unwrap(), &lone);
    let extra = text(&mut tree, body, "x");
    assert_eq!(error(capture(&tree)), TEXT_BUDGET);
    tree.remove_child(body, extra);
    capture(&tree).unwrap();
    // UTF-16 units alone would be within budget; the UTF-8 length is not.
    let mut tree = Tree::parse("");
    let body = self::body(&tree);
    text(&mut tree, body, &"€".repeat(TEXT / 3 + 1));
    assert_eq!(error(capture(&tree)), TEXT_BUDGET);
}

#[test]
fn attribute_budget_sums_qualified_names_and_values_across_the_document() {
    let mut tree = Tree::parse("<svg><use></use></svg><p></p>");
    let use_element = tree.query_selector("use").unwrap();
    let p = tree.query_selector("p").unwrap();
    // `xlink:href` counts with its prefix (10 bytes).
    tree.set_attribute_ns(
        use_element,
        Some("http://www.w3.org/1999/xlink"),
        "xlink:href",
        &"a".repeat(1_000_000 - 10),
    );
    tree.set_attribute(p, "title", &"b".repeat(1_000_000 - 5));
    capture(&tree).unwrap();
    tree.set_attribute(p, "title", &"b".repeat(1_000_000 - 4));
    assert_eq!(error(capture(&tree)), ATTRIBUTE_BUDGET);
    assert_eq!(ATTRIBUTES, 2_000_000);
}

#[test]
fn attribute_budget_measures_the_original_style_before_the_inline_override() {
    let mut tree = Tree::parse("<div id=a></div><div id=b></div>");
    let a = tree.get_element_by_id("a").unwrap();
    let b = tree.get_element_by_id("b").unwrap();
    // Two `id` attributes with one-byte values.
    let ids = 2 * ("id".len() + 1);
    // The original value is over budget; a short override does not help.
    tree.set_attribute(
        a,
        "style",
        &" ".repeat(ATTRIBUTES - ids - "style".len() + 1),
    );
    assert_eq!(
        error(capture_with(&tree, &[(a, "width:1px")])),
        ATTRIBUTE_BUDGET
    );
    // A short original with an over-budget override is admitted.
    tree.set_attribute(a, "style", "width:1px");
    let long = format!("width:2px;{}", " ".repeat(ATTRIBUTES));
    capture_with(&tree, &[(a, &long)]).unwrap();
    // Exactly at the budget.
    tree.set_attribute(a, "style", &" ".repeat(ATTRIBUTES - ids - "style".len()));
    capture_with(&tree, &[(b, "width:1px")]).unwrap();
}

#[test]
fn inline_override_replaces_only_an_existing_style_attribute() {
    let mut tree = Tree::parse(
        "<body style='margin:0'><div id=a style='width:10px;height:5px'></div><div id=b></div></body>",
    );
    let a = tree.get_element_by_id("a").unwrap();
    let b = tree.get_element_by_id("b").unwrap();
    let overrides = [(a, "width:30px;height:5px"), (b, "width:40px;height:5px")];
    let input = capture_with(&tree, &overrides).unwrap();
    let snapshot = layout(&tree, &input, &[]).unwrap();
    let border = |node| {
        let source = snapshot.source(id(node)).unwrap();
        source
            .fragment_indices
            .iter()
            .map(|&index| &snapshot.fragments()[index])
            .find(|fragment| fragment.kind == GeometryKind::BorderBox)
            .map(|fragment| (fragment.rect.width, fragment.rect.height))
    };
    assert_eq!(border(a), Some((30.0, 5.0)));
    // No style attribute: the override is not applied, the div keeps auto height.
    assert_eq!(border(b), Some((200.0, 0.0)));
    // The DOM itself is never rewritten.
    assert_eq!(
        tree.get_attribute(a, "style"),
        Some("width:10px;height:5px")
    );
    tree.remove_attribute(a, "style");
    assert_eq!(tree.get_attribute(b, "style"), None);
}

fn scoped_rule(targets: &[Handle], declarations: usize) -> ActiveRule {
    let declaration = turbo_dom_layout::style::parse_declarations("height:1px").remove(0);
    ActiveRule {
        text: "div{height:1px}".into(),
        scoped: Some(ScopedRule {
            depth: 0,
            declarations: vec![declaration; declarations],
            targets: targets
                .iter()
                .map(|&node| Target {
                    source: id(node),
                    specificity: (0, 0, 1),
                })
                .collect(),
        }),
        resource: None,
    }
}

#[test]
fn declaration_budget_counts_every_declaration_for_every_target() {
    let tree = Tree::parse("<div id=a></div><div id=b></div>");
    let a = tree.get_element_by_id("a").unwrap();
    let b = tree.get_element_by_id("b").unwrap();
    let input = capture(&tree).unwrap();
    let half = DECLARATIONS / 2;
    let rules = [scoped_rule(&[a, b], half - 1), scoped_rule(&[a], 2)];
    layout(&tree, &input, &rules).unwrap();
    let rules = [scoped_rule(&[a, b], half - 1), scoped_rule(&[a], 3)];
    assert_eq!(
        layout(&tree, &input, &rules).err().unwrap(),
        DECLARATION_BUDGET
    );
    // A rule matching nothing contributes nothing.
    let rules = [scoped_rule(&[], DECLARATIONS + 1), scoped_rule(&[b], 1)];
    layout(&tree, &input, &rules).unwrap();
}

#[test]
fn namespace_rejection_covers_mathml_and_null_namespace_elements_but_not_svg() {
    let tree = Tree::parse("<svg><g><text>x</text></g></svg>");
    let input = capture(&tree).unwrap();
    let svg = tree.query_selector("svg").unwrap();
    let g = tree.query_selector("g").unwrap();
    // Only the outermost svg is a viewport.
    assert_eq!(input.capture.svg_viewports, vec![id(svg)]);
    assert!(input
        .capture
        .identities
        .iter()
        .any(|identity| identity.id == id(g)));
    assert_eq!(
        error(capture(&Tree::parse("<math><mi>x</mi></math>"))),
        NAMESPACE
    );
    let mut tree = Tree::parse("");
    let body = body(&tree);
    let element = tree.create_element_namespace(None, "x");
    tree.append_child(body, element);
    assert_eq!(error(capture(&tree)), NAMESPACE);
    let mut tree = Tree::parse("");
    let body = self::body(&tree);
    let element = tree.create_element_ns("x", Namespace::Svg);
    tree.append_child(body, element);
    capture(&tree).unwrap();
}

#[test]
fn errors_report_the_first_violation_in_composed_document_order() {
    let huge = "a".repeat(ATTRIBUTES);
    // On one element the namespace is checked before its attributes.
    let mut tree = Tree::parse("<math></math>");
    let math = tree.query_selector("math").unwrap();
    tree.set_attribute(math, "title", &huge);
    assert_eq!(error(capture(&tree)), NAMESPACE);
    // An element's attributes are checked before its descendants' depth.
    let mut tree = Tree::parse("");
    let parent = body(&tree);
    let outer = nest(&mut tree, parent, 1);
    nest(&mut tree, outer, DEPTH);
    tree.set_attribute(outer, "title", &huge);
    assert_eq!(error(capture(&tree)), ATTRIBUTE_BUDGET);
    // Earlier siblings report first: text before a rejected namespace ...
    let mut tree = Tree::parse("");
    let body = self::body(&tree);
    text(&mut tree, body, &"x".repeat(TEXT + 1));
    let math = tree.create_element_ns("math", Namespace::MathMl);
    tree.append_child(body, math);
    assert_eq!(error(capture(&tree)), TEXT_BUDGET);
    // ... and a rejected namespace before later text.
    let mut tree = Tree::parse("");
    let body = self::body(&tree);
    let math = tree.create_element_ns("math", Namespace::MathMl);
    tree.append_child(body, math);
    text(&mut tree, body, &"x".repeat(TEXT + 1));
    assert_eq!(error(capture(&tree)), NAMESPACE);
    // A too-deep chain before an over-budget text node.
    let mut tree = Tree::parse("");
    let body = self::body(&tree);
    nest(&mut tree, body, DEPTH);
    text(&mut tree, body, &"x".repeat(TEXT + 1));
    assert_eq!(error(capture(&tree)), DEPTH_BUDGET);
    // The node that exceeds the node budget fails on the budget, not on its
    // namespace.
    let mut tree = Tree::parse("");
    let body = self::body(&tree);
    for _ in 0..NODES - EMPTY_DOCUMENT_NODES {
        let comment = tree.create_comment("");
        tree.append_child(body, comment);
    }
    let math = tree.create_element_ns("math", Namespace::MathMl);
    tree.append_child(body, math);
    assert_eq!(error(capture(&tree)), NODE_BUDGET);
}
