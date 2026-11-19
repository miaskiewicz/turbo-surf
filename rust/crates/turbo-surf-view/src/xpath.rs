//! Minimal XPath evaluator (port of `src/xpath.mjs`) over the turbo-dom `Tree`.
//! A pragmatic subset — NOT full XPath 1.0. Supported:
//!
//! - `/a/b`, `//a` descendant, relative `a/b`, `*` wildcard
//! - predicates `[@a='v']`, `[@a]`, `[contains(@a,'v')]`, `[text()='v']`,
//!   `[contains(text(),'v')]`, `[n]` (1-based)
//! - parent/self/ancestor axes and node unions (`|`)
//! - trailing `//a/@href` → returns the attribute string(s)

use std::collections::HashSet;
use turbo_dom_parser::rtdom::tree::Handle;
use turbo_dom_parser::rtdom::Tree;

const ELEMENT_NODE: u8 = 1;

#[derive(Clone)]
enum Term {
    Text,
    Attr(String),
}

#[derive(Clone)]
enum Pred {
    Pos(usize),
    Eq(Term, String),
    Contains(Term, String),
    Exists(String),
    Never,
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    Child,
    Descendant,
    Parent,
    SelfNode,
    Ancestor,
    AncestorOrSelf,
    DescendantOrSelf,
}

struct Step {
    axis: Axis,
    attr: Option<String>,
    test: String,
    preds: Vec<Pred>,
}

/// Result of an XPath evaluation: matched nodes, or attribute string values for
/// a trailing `@attr` step.
pub enum XPath {
    Nodes(Vec<Handle>),
    Values(Vec<String>),
}

// --- step splitting (respects //, quotes, bracketed predicates) -------------

struct Scan {
    depth: i32,
    quote: Option<char>,
}

fn advance_scan(st: &mut Scan, ch: char) -> bool {
    if let Some(q) = st.quote {
        if ch == q {
            st.quote = None;
        }
        return true;
    }
    if ch == '"' || ch == '\'' {
        st.quote = Some(ch);
        return true;
    }
    if ch == '[' {
        st.depth += 1;
    } else if ch == ']' {
        st.depth -= 1;
    }
    st.depth != 0 || ch != '/'
}

fn scan_step_end(expr: &[char], mut i: usize) -> usize {
    let mut st = Scan {
        depth: 0,
        quote: None,
    };
    while i < expr.len() && advance_scan(&mut st, expr[i]) {
        i += 1;
    }
    i
}

fn consume_axis(expr: &[char], i: usize) -> (Axis, usize) {
    // Only the first step can lack a leading slash (the splitter always leaves
    // the cursor on the separating '/'); a relative root means descendant.
    if expr.get(i) != Some(&'/') {
        return (Axis::Descendant, i);
    }
    if expr.get(i + 1) == Some(&'/') {
        (Axis::Descendant, i + 2)
    } else {
        (Axis::Child, i + 1)
    }
}

fn split_steps(expr: &str) -> Vec<Step> {
    let chars: Vec<char> = expr.chars().collect();
    let mut steps = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (axis, start) = consume_axis(&chars, i);
        i = scan_step_end(&chars, start);
        let text: String = chars[start..i].iter().collect();
        if !text.is_empty() {
            steps.push(parse_step(axis, &text));
        }
    }
    steps
}

fn named_axis(name: &str) -> Option<Axis> {
    match name {
        "child" => Some(Axis::Child),
        "descendant" => Some(Axis::Descendant),
        "parent" => Some(Axis::Parent),
        "self" => Some(Axis::SelfNode),
        "ancestor" => Some(Axis::Ancestor),
        "ancestor-or-self" => Some(Axis::AncestorOrSelf),
        "descendant-or-self" => Some(Axis::DescendantOrSelf),
        _ => None,
    }
}

fn parse_step(axis: Axis, text: &str) -> Step {
    let (axis, text) = match text {
        ".." => (Axis::Parent, "*"),
        "." => (Axis::SelfNode, "*"),
        _ => text
            .split_once("::")
            .and_then(|(name, test)| named_axis(name).map(|axis| (axis, test)))
            .unwrap_or((axis, text)),
    };
    if let Some(attr) = text.strip_prefix('@') {
        return Step {
            axis,
            attr: Some(attr.to_string()),
            test: String::new(),
            preds: Vec::new(),
        };
    }
    let (test, rest) = split_node_test(text);
    Step {
        axis,
        attr: None,
        test,
        preds: parse_predicates(rest),
    }
}

// Leading node test: [A-Za-z*][\w-]*, else "*".
fn split_node_test(text: &str) -> (String, &str) {
    let bytes = text.as_bytes();
    let first_ok = bytes
        .first()
        .is_some_and(|&b| b.is_ascii_alphabetic() || b == b'*');
    if !first_ok {
        return ("*".to_string(), text);
    }
    let end = text
        .char_indices()
        .take_while(|&(i, c)| i == 0 || c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    (text[..end].to_string(), &text[end..])
}

fn parse_predicates(rest: &str) -> Vec<Pred> {
    let mut preds = Vec::new();
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == ']') {
                let body: String = chars[i + 1..i + 1 + end].iter().collect();
                preds.push(compile_pred(body.trim()));
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    preds
}

// --- predicate compilation --------------------------------------------------

fn parse_term(s: &str) -> Option<Term> {
    if s == "text()" {
        Some(Term::Text)
    } else {
        s.strip_prefix('@').map(|a| Term::Attr(a.to_string()))
    }
}

fn split_eq(body: &str) -> Option<(Term, String)> {
    let (lhs, rhs) = body.split_once('=')?;
    let term = parse_term(lhs.trim())?;
    let want = unquote(rhs.trim())?;
    Some((term, want))
}

fn split_contains(body: &str) -> Option<(Term, String)> {
    let inner = body.strip_prefix("contains(")?.strip_suffix(')')?;
    let (lhs, rhs) = inner.split_once(',')?;
    let term = parse_term(lhs.trim())?;
    let want = unquote(rhs.trim())?;
    Some((term, want))
}

fn unquote(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        Some(s[1..s.len() - 1].to_string())
    } else {
        None
    }
}

fn compile_pred(body: &str) -> Pred {
    if !body.is_empty() && body.chars().all(|c| c.is_ascii_digit()) {
        return Pred::Pos(body.parse().unwrap_or(0));
    }
    if let Some((t, w)) = split_eq(body) {
        return Pred::Eq(t, w);
    }
    if let Some((t, w)) = split_contains(body) {
        return Pred::Contains(t, w);
    }
    if let Some(attr) = body.strip_prefix('@') {
        if !attr.is_empty()
            && attr
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Pred::Exists(attr.to_string());
        }
    }
    Pred::Never
}

// --- evaluation -------------------------------------------------------------

fn read_term(tree: &Tree, h: Handle, term: &Term) -> Option<String> {
    match term {
        Term::Text => Some(tree.text_content(h).trim().to_string()),
        Term::Attr(name) => tree.get_attribute(h, name).map(str::to_string),
    }
}

fn pred_matches(tree: &Tree, h: Handle, pred: &Pred) -> bool {
    match pred {
        Pred::Eq(t, want) => read_term(tree, h, t).as_deref() == Some(want.as_str()),
        Pred::Contains(t, want) => read_term(tree, h, t).is_some_and(|v| v.contains(want)),
        Pred::Exists(name) => tree.get_attribute(h, name).is_some(),
        Pred::Never => false,
        Pred::Pos(_) => unreachable!("apply_predicate resolves positional predicates itself"),
    }
}

fn element_children(tree: &Tree, ctx: Handle) -> Vec<Handle> {
    tree.children(ctx)
        .into_iter()
        .filter(|&c| tree.node_type_id(c) == ELEMENT_NODE)
        .collect()
}

fn tag_matches(tree: &Tree, h: Handle, test: &str) -> bool {
    test == "*" || tree.tag_name(h).as_deref() == Some(&test.to_ascii_uppercase())
}

fn ancestor_nodes(tree: &Tree, ctx: Handle, include_self: bool) -> Vec<Handle> {
    let mut nodes = Vec::new();
    let mut current = if include_self {
        Some(ctx)
    } else {
        tree.parent(ctx)
    };
    while let Some(node) = current {
        nodes.push(node);
        current = tree.parent(node);
    }
    nodes
}

fn axis_nodes(tree: &Tree, ctx: Handle, axis: Axis) -> Vec<Handle> {
    match axis {
        Axis::Child => element_children(tree, ctx),
        Axis::Descendant => tree.descendants(ctx),
        Axis::Parent => tree.parent(ctx).into_iter().collect(),
        Axis::SelfNode => vec![ctx],
        Axis::Ancestor => ancestor_nodes(tree, ctx, false),
        Axis::AncestorOrSelf => ancestor_nodes(tree, ctx, true),
        Axis::DescendantOrSelf => std::iter::once(ctx).chain(tree.descendants(ctx)).collect(),
    }
}

fn candidates(tree: &Tree, ctx: Handle, step: &Step) -> Vec<Handle> {
    axis_nodes(tree, ctx, step.axis)
        .into_iter()
        .filter(|&node| {
            tree.node_type_id(node) == ELEMENT_NODE && tag_matches(tree, node, &step.test)
        })
        .collect()
}

fn apply_predicate(tree: &Tree, nodes: Vec<Handle>, pred: &Pred) -> Vec<Handle> {
    if let Pred::Pos(n) = pred {
        return nodes.get(n.wrapping_sub(1)).copied().into_iter().collect();
    }
    nodes
        .into_iter()
        .filter(|&h| pred_matches(tree, h, pred))
        .collect()
}

fn dedupe(nodes: Vec<Handle>) -> Vec<Handle> {
    let mut seen = HashSet::new();
    nodes.into_iter().filter(|&h| seen.insert(h)).collect()
}

fn run_step(tree: &Tree, ctx: &[Handle], step: &Step) -> Vec<Handle> {
    let mut matched = Vec::new();
    for &root in ctx {
        let mut nodes = candidates(tree, root, step);
        for pred in &step.preds {
            nodes = apply_predicate(tree, nodes, pred);
        }
        matched.extend(nodes);
    }
    dedupe(matched)
}

fn union_branches(expr: &str) -> Vec<&str> {
    let mut branches = Vec::new();
    let mut start = 0;
    let mut scan = Scan {
        depth: 0,
        quote: None,
    };
    for (index, ch) in expr.char_indices() {
        if ch == '|' && scan.depth == 0 && scan.quote.is_none() {
            branches.push(expr[start..index].trim());
            start = index + 1;
        } else {
            advance_scan(&mut scan, ch);
        }
    }
    branches.push(expr[start..].trim());
    branches
}

/// Evaluate an XPath subset against `root`.
fn evaluate_branch(tree: &Tree, root: Handle, expr: &str) -> XPath {
    let steps = split_steps(expr.trim());
    let mut ctx = vec![root];
    for step in &steps {
        if let Some(attr) = &step.attr {
            let values = ctx
                .iter()
                .filter_map(|&h| tree.get_attribute(h, attr).map(str::to_string))
                .collect();
            return XPath::Values(values);
        }
        ctx = run_step(tree, &ctx, step);
    }
    XPath::Nodes(ctx)
}

/// Evaluate paths and node unions, preserving the document order of a union.
pub fn evaluate(tree: &Tree, root: Handle, expr: &str) -> XPath {
    let branches = union_branches(expr.trim());
    if branches.len() == 1 {
        return evaluate_branch(tree, root, branches[0]);
    }
    let mut nodes = Vec::new();
    let mut values = Vec::new();
    for branch in branches {
        match evaluate_branch(tree, root, branch) {
            XPath::Nodes(found) => nodes.extend(found),
            XPath::Values(found) => values.extend(found),
        }
    }
    if nodes.is_empty() && !values.is_empty() {
        return XPath::Values(values);
    }
    let selected: HashSet<_> = nodes.into_iter().collect();
    XPath::Nodes(
        tree.descendants(tree.root())
            .into_iter()
            .filter(|node| selected.contains(node))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(tree: &Tree, expr: &str) -> Vec<Handle> {
        match evaluate(tree, tree.root(), expr) {
            XPath::Nodes(n) => n,
            XPath::Values(_) => panic!("expected nodes"),
        }
    }
    fn vals(tree: &Tree, expr: &str) -> Vec<String> {
        match evaluate(tree, tree.root(), expr) {
            XPath::Values(v) => v,
            XPath::Nodes(_) => panic!("expected values"),
        }
    }

    #[test]
    fn attribute_unions_keep_value_results_and_empty_node_unions_stay_nodes() {
        let tree = Tree::parse("<a href='/a'></a><b href='/b'></b>");
        assert_eq!(vals(&tree, "//a/@href | //b/@href"), vec!["/a", "/b"]);
        assert!(
            matches!(evaluate(&tree, tree.root(), "//missing | //absent"), XPath::Nodes(found) if found.is_empty())
        );
    }

    #[test]
    fn node_unions_deduplicate_and_return_document_order() {
        let tree = Tree::parse(
            "<div id='a' data-kind='a|b'><span id='b'>one</span></div><div id='c'>two</div>",
        );
        let result = nodes(&tree, "//*[@id='c'] | //*[@id='a'] | //*[@id='c']");
        assert_eq!(
            result
                .iter()
                .map(|&node| tree.get_attribute(node, "id").unwrap())
                .collect::<Vec<_>>(),
            vec!["a", "c"]
        );
        assert_eq!(nodes(&tree, "//*[@data-kind='a|b'] | //missing").len(), 1);
        let child = tree.query_selector("#b").unwrap();
        assert!(
            matches!(evaluate(&tree, child, "ancestor::*[1]"), XPath::Nodes(found) if found == vec![tree.query_selector("#a").unwrap()])
        );
        assert!(
            matches!(evaluate(&tree, child, "ancestor-or-self::span"), XPath::Nodes(found) if found == vec![child])
        );
    }

    #[test]
    fn relative_parent_axes_and_union_preserve_context() {
        let tree = Tree::parse("<section class='MuiPickersInputBase-root'><div id='field'><div class='MuiPickersSectionList-root'>01</div></div></section><button>outside</button>");
        let field = tree.query_selector("#field").unwrap();
        for (expression, count) in [
            ("child::div", 1),
            ("descendant::div", 1),
            ("parent::section", 1),
            ("self::div", 1),
            ("ancestor::section", 1),
            ("ancestor-or-self::div", 1),
            ("descendant-or-self::div", 2),
            ("unsupported::div", 0),
        ] {
            assert!(
                matches!(evaluate(&tree, field, expression), XPath::Nodes(found) if found.len() == count),
                "{expression}"
            );
        }
        let parent = match evaluate(&tree, field, "..") {
            XPath::Nodes(nodes) => nodes,
            _ => unreachable!(),
        };
        assert_eq!(parent, vec![tree.parent(field).unwrap()]);
        let expr = "descendant-or-self::div[contains(@class,'MuiPickersSectionList-root')] | ancestor::*[contains(@class,'MuiPickersInputBase-root')][1]//div[contains(@class,'MuiPickersSectionList-root')]";
        let matches = match evaluate(&tree, field, expr) {
            XPath::Nodes(nodes) => nodes,
            _ => unreachable!(),
        };
        assert_eq!(matches.len(), 1);
        assert_eq!(tree.text_content(matches[0]), "01");
        assert!(
            matches! (evaluate(&tree, field, "."), XPath::Nodes(nodes) if nodes == vec![field])
        );
    }

    #[test]
    fn descendant_and_tag_test() {
        let tree = Tree::parse("<div><p>a</p><p>b</p><span>c</span></div>");
        assert_eq!(nodes(&tree, "//p").len(), 2);
        assert_eq!(nodes(&tree, "//span").len(), 1);
        assert!(nodes(&tree, "//*").len() >= 4);
    }

    #[test]
    fn predicate_only_step_defaults_to_wildcard() {
        // A step that starts with '[' has no node test → defaults to "*".
        let tree = Tree::parse("<div id='a'><span id='b'>x</span></div>");
        assert_eq!(nodes(&tree, "//[@id]").len(), 2);
    }

    #[test]
    fn unquoted_predicate_value_matches_nothing() {
        // RHS without quotes fails to compile to Eq → Never.
        let tree = Tree::parse("<a x='5'>z</a>");
        assert_eq!(nodes(&tree, "//a[@x=5]").len(), 0);
    }

    #[test]
    fn trailing_text_after_predicate_ignored() {
        let tree = Tree::parse("<ul><li>1</li><li>2</li></ul>");
        // Junk after a predicate is skipped by the predicate scanner.
        assert_eq!(nodes(&tree, "//li[1]x").len(), 1);
    }

    #[test]
    fn child_axis_and_positional() {
        let tree = Tree::parse("<ul><li>1</li><li>2</li><li>3</li></ul>");
        let li2 = nodes(&tree, "//ul/li[2]");
        assert_eq!(li2.len(), 1);
        assert_eq!(tree.text_content(li2[0]), "2");
    }

    #[test]
    fn attr_and_text_predicates() {
        let tree = Tree::parse("<a href='/x' class='k'>Next</a><a href='/y'>Prev</a><a>none</a>");
        assert_eq!(nodes(&tree, "//a[@href]").len(), 2);
        assert_eq!(nodes(&tree, "//a[@class='k']").len(), 1);
        assert_eq!(nodes(&tree, "//a[text()='Next']").len(), 1);
        assert_eq!(nodes(&tree, "//a[contains(text(),'re')]").len(), 1); // "Prev"
        assert_eq!(nodes(&tree, "//a[contains(@href,'x')]").len(), 1);
    }

    #[test]
    fn trailing_attribute_step_returns_values() {
        let tree = Tree::parse("<a href='/x'>x</a><a href='/y'>y</a>");
        assert_eq!(vals(&tree, "//a/@href"), vec!["/x", "/y"]);
    }

    #[test]
    fn unknown_predicate_matches_nothing() {
        let tree = Tree::parse("<p>a</p>");
        assert_eq!(nodes(&tree, "//p[bogus()]").len(), 0);
    }

    #[test]
    fn relative_expr_without_leading_slash() {
        // Expression not starting with '/' → first step is a descendant search.
        let tree = Tree::parse("<div><p>a</p><p>b</p></div>");
        assert_eq!(nodes(&tree, "p").len(), 2);
    }

    #[test]
    fn relative_child_path() {
        let tree = Tree::parse("<div><section><h1>T</h1></section></div>");
        // descendant section, then child h1
        assert_eq!(nodes(&tree, "//section/h1").len(), 1);
    }
}
