//! Match selectors before slot placement changes physical ancestry. Snapshot
//! targets retain native identity and the original encapsulation depth.
#[path = "live_layout_compiled.rs"]
mod compiled;
#[path = "live_layout_nesting.rs"]
mod nesting;

use crate::live_layout::source_id;
use crate::live_layout_composed::Composition;
use crate::live_stylesheets::{RuleSnapshot, SheetSnapshot};
use cssparser::ToCss;
use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::{Property, PropertyId};
use lightningcss::rules::CssRule;
use lightningcss::selector::{Combinator, Component, Selector};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use turbo_dom_parser::rtdom::query::parse_for_cascade;
use turbo_dom_parser::rtdom::{tree::Handle, Tree};

/// A rule matched against one original tree scope: turbo-dom-layout's live-DOM
/// input applies its declarations to its targets at the scope's depth.
pub(crate) use turbo_dom_layout::live::{MatchedTarget as Target, PreMatched as ScopedRule};

pub(crate) fn prepare_sheet(
    tree: &Tree,
    document: u32,
    root: Handle,
    graph: &Composition,
    sheet: &mut SheetSnapshot,
) -> Result<(), String> {
    sheet.rules = nesting::flatten(std::mem::take(&mut sheet.rules))?;
    for rule in &mut sheet.rules {
        if rule.kind == 1 {
            rule.scoped = Some(prepare_rule(tree, document, root, graph, rule)?);
        }
        // Keyframe percentages are animation steps, never DOM selectors.
        let grouping = matches!(rule.kind, 4 | 12) || crate::live_stylesheets::is_container(rule);
        if let Some(rules) = rule.rules.as_mut().filter(|_| grouping) {
            let mut nested = SheetSnapshot {
                cycle: false,
                disabled: false,
                media: String::new(),
                rules: std::mem::take(rules),
            };
            prepare_sheet(tree, document, root, graph, &mut nested)?;
            *rules = nested.rules;
        }
        if let Some(sheet) = &mut rule.style_sheet {
            prepare_sheet(tree, document, root, graph, sheet)?;
        }
    }
    Ok(())
}
fn prepare_rule(
    tree: &Tree,
    document: u32,
    root: Handle,
    graph: &Composition,
    rule: &RuleSnapshot,
) -> Result<ScopedRule, String> {
    if let Some(prepared) = compiled::prepare(tree, document, root, graph, &rule.css_text)? {
        return Ok(prepared);
    }
    let depth = *graph
        .depths
        .get(&root)
        .ok_or("native CSS scope depth missing")?;
    // The CSSOM kept this rule, so Chrome keeps it too; a selector the native parser
    // rejects (`::selection:window-inactive`) cannot match a laid-out box, so the rule
    // gets no targets instead of failing the whole layout (like #117).
    #[cfg(test)]
    RULE_COMPILATIONS.with(|count| count.set(count.get() + 1));
    let Ok(parsed) = StyleSheet::parse(&rule.css_text, ParserOptions::default()) else {
        return Ok(ScopedRule {
            declarations: Vec::new(),
            depth,
            targets: Vec::new(),
        });
    };
    let [CssRule::Style(style)] = parsed.rules.0.as_slice() else {
        return Err("native scoped CSS requires one qualified rule".into());
    };
    if !style.rules.0.is_empty() {
        return Err("native scoped nested rules are unsupported".into());
    }
    let declarations = declarations(&style.declarations)?;
    let sources = selector_sources(&rule.css_text)?;
    if sources.len() != style.selectors.0.len() {
        return Err("native selector source provenance mismatch".into());
    }
    // A selector list Chrome drops (an unknown pseudo-class, a misplaced
    // pseudo-element) makes the rule invalid on its own: it matches nothing, and
    // the rest of the page keeps its layout (#117).
    if parse_for_cascade(&sources.join(",")).is_err() {
        return Ok(ScopedRule {
            declarations,
            depth,
            targets: Vec::new(),
        });
    }
    let matcher = Matcher { tree, root, graph };
    let mut specificity_by_node = std::collections::HashMap::<Handle, u32>::new();
    for (selector, source) in style.selectors.0.iter().zip(&sources) {
        for node in matcher.selector_nodes(selector, source)? {
            let value = specificity_by_node.entry(node).or_default();
            *value = (*value).max(selector.specificity());
        }
    }
    let mut targets = specificity_by_node
        .into_iter()
        .map(|(node, value)| Target {
            source: source_id(document, node),
            specificity: (value >> 20, (value >> 10) & 1023, value & 1023),
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| target.source.0);
    Ok(ScopedRule {
        declarations,
        depth,
        targets,
    })
}

fn matches_validated(
    tree: &Tree,
    node: Handle,
    source: &str,
    validated: &mut bool,
) -> Result<bool, String> {
    #[cfg(test)]
    ORDINARY_MATCHES.with(|count| count.set(count.get() + 1));
    // The checked API validates only the selector, then calls matches. Keep the
    // first eligible checked call and evaluate every later node's current state.
    if *validated {
        return Ok(tree.matches(node, source));
    }
    #[cfg(test)]
    ORDINARY_VALIDATIONS.with(|count| count.set(count.get() + 1));
    let result = tree
        .matches_checked(node, source)
        .map_err(|error| format!("native selector match failed: {error:?}"))?;
    *validated = true;
    Ok(result)
}

fn selector_sources(source: &str) -> Result<Vec<String>, String> {
    let mut input = cssparser::ParserInput::new(source);
    let mut input = cssparser::Parser::new(&mut input);
    let result: Result<Vec<String>, cssparser::ParseError<'_, ()>> =
        input.parse_until_before(cssparser::Delimiter::CurlyBracketBlock, |input| {
            input.parse_comma_separated(|input| {
                let start = input.position();
                while input.next_including_whitespace_and_comments().is_ok() {}
                Ok(input.slice_from(start).trim().to_owned())
            })
        });
    result.map_err(|_| "native selector source parsing failed".into())
}
fn candidate_seed(selector: &Selector<'_>) -> Option<String> {
    let mut tag = None;
    let mut functional = None;
    for component in selector.iter_raw_match_order() {
        let (prefix, name) = match component {
            Component::Combinator(_) => break,
            Component::ID(value) => ("#", value.as_ref()),
            Component::Class(value) => (".", value.as_ref()),
            Component::LocalName(value) => {
                tag = Some(value.name.as_ref());
                continue;
            }
            Component::Is(values) | Component::Where(values) | Component::Any(_, values) => {
                functional = functional.or_else(|| positive_functional_seed(values));
                continue;
            }
            _ => continue,
        };
        return candidate_identifier(prefix, name);
    }
    // Keep the more selective ID/class buckets; a type-only bucket still avoids
    // evaluating dynamic and positional selectors against unrelated elements.
    tag.and_then(|name| candidate_identifier("", name))
        .or(functional)
}

// Every alternative needs a static necessary seed. Their union only narrows
// candidates; dirty checked state and other live predicates are still matched.
fn positive_functional_seed(selectors: &[Selector<'_>]) -> Option<String> {
    let seeds = selectors
        .iter()
        .map(candidate_seed)
        .collect::<Option<Vec<_>>>()?;
    (!seeds.is_empty()).then(|| seeds.join(","))
}

fn candidate_identifier(prefix: &str, name: &str) -> Option<String> {
    let mut source = String::from(prefix);
    cssparser::serialize_identifier(name, &mut source).ok()?;
    Some(source)
}

// A top-level pseudo-element selects no source Element. Functional alternatives
// and shadow selectors retain their existing matching paths.
fn ordinary_element_selector(selector: &Selector<'_>) -> bool {
    !selector
        .iter_raw_match_order()
        .any(|part| matches!(part, Component::PseudoElement(_)))
}

fn shadow_selector(selector: &Selector<'_>) -> bool {
    selector.iter_raw_match_order().any(|part| match part {
        Component::Host(_) | Component::Slotted(_) | Component::Part(_) => true,
        Component::Is(values)
        | Component::Where(values)
        | Component::Negation(values)
        | Component::Has(values) => values.iter().any(shadow_selector),
        _ => false,
    })
}

fn declarations(
    block: &DeclarationBlock<'_>,
) -> Result<Vec<turbo_dom_layout::style::Declaration>, String> {
    use lightningcss::stylesheet::PrinterOptions;
    let mut result = Vec::new();
    for (important, properties) in [
        (false, &block.declarations),
        (true, &block.important_declarations),
    ] {
        for property in properties {
            // Expand typed shorthands before ranking: a retained UA longhand
            // must not override a stronger authored border or font shorthand.
            let expanded = if matches!(
                property,
                Property::Font(_)
                    | Property::Animation(..)
                    | Property::Border(_)
                    | Property::BorderTop(_)
                    | Property::BorderRight(_)
                    | Property::BorderBottom(_)
                    | Property::BorderLeft(_)
            ) {
                property
                    .property_id()
                    .longhands()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|id| property.longhand(id))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let wide = if property.property_id() == PropertyId::Font {
                let value = property
                    .value_to_css_string(PrinterOptions::default())
                    .map_err(|error| format!("native CSS value serialization failed: {error:?}"))?;
                ["inherit", "initial", "unset", "revert", "revert-layer"]
                    .contains(&value.as_str())
                    .then_some(value)
            } else {
                None
            };
            if let Some(value) = wide {
                for id in PropertyId::Font.longhands().unwrap_or_default() {
                    result.push(turbo_dom_layout::style::Declaration {
                        property: lightningcss::traits::ToCss::to_css_string(
                            &id,
                            PrinterOptions::default(),
                        )
                        .map_err(|error| {
                            format!("native CSS property serialization failed: {error:?}")
                        })?,
                        value: value.clone(),
                        important,
                    });
                }
                continue;
            }
            for property in if expanded.is_empty() {
                vec![property.clone()]
            } else {
                expanded
            } {
                let text = property
                    .to_css_string(important, PrinterOptions::default())
                    .map_err(|error| {
                        format!("native CSS declaration serialization failed: {error:?}")
                    })?;
                result.extend(turbo_dom_layout::style::parse_declarations(&text));
            }
        }
    }
    Ok(result)
}

pub(crate) fn inline_declarations(
    text: &str,
) -> Result<Vec<turbo_dom_layout::style::Declaration>, String> {
    let block = DeclarationBlock::parse_string(
        text,
        ParserOptions {
            error_recovery: true,
            ..ParserOptions::default()
        },
    )
    .map_err(|error| format!("native inline declaration parse failed: {error:?}"))?;
    declarations(&block)
}
struct Matcher<'a> {
    tree: &'a Tree,
    root: Handle,
    graph: &'a Composition,
}
impl Matcher<'_> {
    /// The elements of this scope one selector of a rule matches.
    fn selector_nodes(&self, selector: &Selector<'_>, source: &str) -> Result<Vec<Handle>, String> {
        if !shadow_selector(selector) {
            if !ordinary_element_selector(selector) {
                return Ok(Vec::new());
            }
            return self.ordinary_nodes(source, candidate_seed(selector).as_deref());
        }
        let mut result = Vec::new();
        for node in self.graph.scopes.keys().copied() {
            if self.tree.node_type_id(node) == 1 && self.matches(selector, node, false)? {
                result.push(node);
            }
        }
        Ok(result)
    }
    fn ordinary_nodes(&self, source: &str, seed: Option<&str>) -> Result<Vec<Handle>, String> {
        if self.root == self.tree.root() && !source.contains(':') {
            return self.batched_nodes(source);
        }
        let candidates = if self.root == self.tree.root() {
            seed.map(|seed| self.batched_nodes(seed)).transpose()?
        } else {
            None
        };
        let scoped = candidates
            .is_none()
            .then(|| self.graph.scopes.keys().copied());
        let nodes = candidates
            .iter()
            .flatten()
            .copied()
            .chain(scoped.into_iter().flatten());
        let mut validated = false;
        let mut result = Vec::new();
        for node in nodes {
            if self.tree.node_type_id(node) == 1
                && self.graph.scopes.get(&node) == Some(&self.root)
                && matches_validated(self.tree, node, source, &mut validated)?
            {
                result.push(node);
            }
        }
        Ok(result)
    }
    fn batched_nodes(&self, source: &str) -> Result<Vec<Handle>, String> {
        #[cfg(test)]
        BATCH_QUERIES.with(|count| count.set(count.get() + 1));
        let nodes = self
            .tree
            .query_selector_all_checked(source)
            .map_err(|error| format!("native selector match failed: {error:?}"))?;
        Ok(nodes
            .iter()
            .copied()
            .filter(|node| self.graph.scopes.get(node) == Some(&self.root))
            .collect())
    }
    fn matches(
        &self,
        selector: &Selector<'_>,
        node: Handle,
        argument: bool,
    ) -> Result<bool, String> {
        let parts: Vec<_> = selector.iter_raw_match_order().collect();
        self.sequence(&parts, node, argument)
    }
    fn sequence(
        &self,
        parts: &[&Component<'_>],
        node: Handle,
        argument: bool,
    ) -> Result<bool, String> {
        let end = parts
            .iter()
            .position(|part| matches!(part, Component::Combinator(_)))
            .unwrap_or(parts.len());
        if !self.compound(&parts[..end], node, argument)? {
            return Ok(false);
        }
        let Some(Component::Combinator(combinator)) = parts.get(end).copied() else {
            return Ok(true);
        };
        let rest = &parts[end + 1..];
        match combinator {
            Combinator::Child => self
                .parent(node)
                .map_or(Ok(false), |parent| self.sequence(rest, parent, false)),
            Combinator::Descendant => {
                let mut parent = self.parent(node);
                while let Some(node) = parent {
                    if self.sequence(rest, node, false)? {
                        return Ok(true);
                    }
                    parent = self.parent(node);
                }
                Ok(false)
            }
            Combinator::NextSibling | Combinator::LaterSibling => {
                let mut previous = self.tree.previous_sibling(node);
                while let Some(node) = previous {
                    if self.tree.node_type_id(node) == 1 {
                        if self.sequence(rest, node, false)? {
                            return Ok(true);
                        }
                        if *combinator == Combinator::NextSibling {
                            break;
                        }
                    }
                    previous = self.tree.previous_sibling(node);
                }
                Ok(false)
            }
            Combinator::SlotAssignment => {
                let slot = self.graph.parents.get(&node).copied().filter(|slot| {
                    self.tree.local_name(*slot) == Some("slot")
                        && self.tree.parent(node) != Some(*slot)
                        && self.graph.scopes.get(slot) == Some(&self.root)
                });
                slot.map_or(Ok(false), |slot| self.sequence(rest, slot, false))
            }
            _ => Err("native scoped CSS pseudo-element/combinator is unsupported".into()),
        }
    }
    fn parent(&self, node: Handle) -> Option<Handle> {
        if self.graph.hosts.get(&self.root) == Some(&node) {
            return None;
        }
        let parent = self.tree.parent(node)?;
        if parent == self.root {
            self.graph.hosts.get(&self.root).copied()
        } else {
            (self.tree.node_type_id(parent) == 1).then_some(parent)
        }
    }
    fn compound(
        &self,
        parts: &[&Component<'_>],
        node: Handle,
        argument: bool,
    ) -> Result<bool, String> {
        let mut ordinary = String::new();
        // Set by a part that lets a node outside this scope match (the host, a
        // slotted node).
        let mut crosses = false;
        for part in parts {
            if !self.part(part, node, argument, &mut ordinary, &mut crosses)? {
                return Ok(false);
            }
        }
        if !argument && self.graph.scopes.get(&node) != Some(&self.root) && !crosses {
            return Ok(false);
        }
        if ordinary.is_empty() {
            return Ok(true);
        }
        self.tree
            .matches_checked(node, &ordinary)
            .map_err(|error| format!("native selector match failed: {error:?}"))
    }
    /// One simple selector of a compound: whether it still matches. Ordinary parts
    /// are serialized into `ordinary` for one native match.
    fn part(
        &self,
        part: &Component<'_>,
        node: Handle,
        argument: bool,
        ordinary: &mut String,
        crosses: &mut bool,
    ) -> Result<bool, String> {
        match part {
            Component::Host(selector) => {
                *crosses = true;
                self.host(selector.as_ref(), node)
            }
            Component::Slotted(selector) => {
                *crosses = true;
                self.slotted(selector, node)
            }
            Component::Is(selectors) | Component::Where(selectors) => {
                let any = self.any(selectors, node, argument)?;
                // A featureless host may participate through :is(:host)
                // or :where(:host), but not through an ordinary negation.
                *crosses |= any && self.is_host(node);
                Ok(any)
            }
            Component::Negation(selectors) => Ok(!self.any(selectors, node, argument)?),
            _ => {
                part.to_css(ordinary)
                    .map_err(|_| "native selector serialization failed")?;
                Ok(true)
            }
        }
    }
    fn is_host(&self, node: Handle) -> bool {
        self.graph.hosts.get(&self.root) == Some(&node)
    }
    fn host(&self, selector: Option<&Selector<'_>>, node: Handle) -> Result<bool, String> {
        if !self.is_host(node) {
            return Ok(false);
        }
        selector.map_or(Ok(true), |selector| self.matches(selector, node, true))
    }
    fn slotted(&self, selector: &Selector<'_>, node: Handle) -> Result<bool, String> {
        let assigned = self.graph.parents.get(&node).is_some_and(|slot| {
            self.tree.local_name(*slot) == Some("slot")
                && self.tree.parent(node) != Some(*slot)
                && self.graph.scopes.get(slot) == Some(&self.root)
        });
        Ok(assigned && self.matches(selector, node, true)?)
    }
    fn any(
        &self,
        selectors: &[Selector<'_>],
        node: Handle,
        argument: bool,
    ) -> Result<bool, String> {
        // Every argument is matched (not short-circuited), so a selector the native
        // matcher rejects surfaces wherever it appears in the list.
        let mut any = false;
        for selector in selectors {
            any |= self.matches(selector, node, argument)?;
        }
        Ok(any)
    }
}

/// The UA stylesheet (turbo-dom-layout's + surf's live supplement + `slot`) matched
/// in every tree scope of `graph`.
pub(crate) fn ua_rules(
    tree: &Tree,
    document: u32,
    graph: &Composition,
) -> Result<Vec<crate::live_stylesheets::ActiveRule>, String> {
    // Chrome's fullscreen.css joins while the binding reports a fullscreen element
    // (turbo-dom#82): getComputedStyle's used width / height of the fullscreen element read
    // this layout (turbo-dom#316). A stack change bumps the tree version, so no cached
    // layout outlives it.
    let fullscreen = if tree.fullscreen_stack().is_empty() {
        ""
    } else {
        turbo_dom_layout::style::fullscreen_user_agent_css()
    };
    let css = format!(
        "{} {} slot{{display:contents}} {fullscreen}",
        turbo_dom_layout::style::user_agent_css(),
        crate::live_layout::LIVE_UA
    );
    let parsed = StyleSheet::parse(&css, ParserOptions::default())
        .map_err(|error| format!("native UA CSS parse failed: {error:?}"))?;
    let mut roots = graph.depths.keys().copied().collect::<Vec<_>>();
    // Equivalent captures must retain the same UA cascade order and cache key.
    roots.sort_unstable_by_key(|root| (graph.depths[root], *root));
    let mut result = Vec::new();
    for root in roots {
        for rule in &parsed.rules.0 {
            let text = lightningcss::traits::ToCss::to_css_string(
                rule,
                lightningcss::stylesheet::PrinterOptions::default(),
            )
            .map_err(|error| format!("native UA CSS serialization failed: {error:?}"))?;
            let snapshot = RuleSnapshot {
                kind: 1,
                css_text: text.clone(),
                prelude: None,
                rules: None,
                media: String::new(),
                style_sheet: None,
                scoped: None,
            };
            result.push(crate::live_stylesheets::ActiveRule {
                text,
                scoped: Some(prepare_rule(tree, document, root, graph, &snapshot)?),
                resource: None,
                containers: Vec::new(),
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
thread_local! {static ORDINARY_VALIDATIONS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

#[cfg(test)]
thread_local! {static ORDINARY_MATCHES:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

#[cfg(test)]
thread_local! {static BATCH_QUERIES:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

#[cfg(test)]
thread_local! { static RULE_COMPILATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(tree: &Tree, root: Handle, graph: &Composition, css: &str) -> ScopedRule {
        prepared_document(tree, 1, root, graph, css)
    }
    fn prepared_document(
        tree: &Tree,
        document: u32,
        root: Handle,
        graph: &Composition,
        css: &str,
    ) -> ScopedRule {
        prepare_rule(
            tree,
            document,
            root,
            graph,
            &RuleSnapshot {
                kind: 1,
                css_text: css.into(),
                prelude: None,
                rules: None,
                media: String::new(),
                style_sheet: None,
                scoped: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn ua_rule_scope_order_is_independent_of_hash_iteration() {
        let mut tree = Tree::parse("<body><input><div id=a></div><div id=b></div><div id=c></div>");
        for id in ["a", "b", "c"] {
            let host = tree.get_element_by_id(id).unwrap();
            let shadow = tree.attach_shadow(host);
            let child = tree.create_element("input");
            tree.set_attribute(child, "type", "checkbox");
            tree.append_child(shadow, child);
        }
        let mut graph = Composition::scopes(&tree).unwrap();
        let count = graph.depths.len();
        // Make an iteration-order dependency observable with every hash seed.
        for (index, depth) in graph.depths.values_mut().enumerate() {
            *depth = (count - index) as _;
        }
        let mut expected = Vec::new();
        for rank in 1..=count {
            let root = *graph
                .depths
                .iter()
                .find(|(_, depth)| **depth as usize == rank)
                .unwrap()
                .0;
            let mut single = graph.clone();
            single.depths.retain(|&scope, _| scope == root);
            expected.extend(ua_rules(&tree, 1, &single).unwrap());
        }
        assert_eq!(ua_rules(&tree, 1, &graph).unwrap(), expected);
        let entries = graph
            .depths
            .iter()
            .map(|(&root, &depth)| (root, depth))
            .collect::<Vec<_>>();
        graph.depths = entries.into_iter().rev().collect();
        assert_eq!(ua_rules(&tree, 1, &graph).unwrap(), expected);
        let natural = Composition::scopes(&tree).unwrap();
        let mut expected = Vec::new();
        let mut roots = vec![tree.root()];
        roots.extend(["a", "b", "c"].map(|id| {
            tree.shadow_root(tree.get_element_by_id(id).unwrap())
                .unwrap()
        }));
        for root in roots {
            let mut single = natural.clone();
            single.depths.retain(|&scope, _| scope == root);
            expected.extend(ua_rules(&tree, 1, &single).unwrap());
        }
        assert_eq!(ua_rules(&tree, 1, &natural).unwrap(), expected);
    }

    #[test]
    fn positive_functional_candidates_bound_work_without_caching_live_results() {
        let mut tree = Tree::parse(&format!("<body>{}<input id=a type=checkbox class=pick><textarea class=pick></textarea><button></button>", "<style></style>".repeat(1000)));
        let input = tree.get_element_by_id("a").unwrap();
        for checked in [false, true, false] {
            tree.set_form_property(
                input,
                turbo_dom_parser::rtdom::tree::FormProp::Checked,
                checked,
            );
            let graph = Composition::scopes(&tree).unwrap();
            for (css, bounded) in [
                (":is(input,textarea):checked{width:3px}", true),
                (":-webkit-any(input,textarea):checked{width:3px}", true),
                (":where(input,textarea):not(.missing){width:3px}", true),
                (
                    ":is(:where(input,textarea),button):not(.missing){width:3px}",
                    true,
                ),
                (":is(input,:not(.missing)):checked{width:3px}", false),
                (":is(:scope,.missing){width:3px}", false),
            ] {
                let expected = legacy_targets(&tree, tree.root(), &graph, css);
                ORDINARY_MATCHES.with(|count| count.set(0));
                let actual = prepared(&tree, tree.root(), &graph, css).targets;
                assert_eq!(actual, expected);
                if bounded {
                    assert!(ORDINARY_MATCHES.with(|count| count.get()) <= 3);
                }
            }
        }
    }

    #[test]
    fn ordinary_pseudo_element_rules_do_not_scan_element_candidates() {
        let mut tree = Tree::parse(&format!(
            "<body>{}<input id=pick class='pick foo::bar' data-label='::before' type=checkbox checked>",
            "<style></style>".repeat(1000)
        ));
        let pick = tree.get_element_by_id("pick").unwrap();
        for checked in [false, true, false] {
            tree.set_form_property(
                pick,
                turbo_dom_parser::rtdom::tree::FormProp::Checked,
                checked,
            );
            let graph = Composition::scopes(&tree).unwrap();
            for (css, matches_pick) in [
                ("*::before,*::after,input:checked::before,.pick:hover::-webkit-input-placeholder{width:3px}", false),
                (".pick,*::before,.pick::after{width:3px}", true),
                ("[data-label='::before']{width:3px}", true),
                (r".foo\:\:bar:checked{width:3px}", checked),
                (":is(.pick,.missing){width:3px}", true),
            ] {
                let expected = legacy_targets(&tree, tree.root(), &graph, css);
                ORDINARY_MATCHES.with(|count| count.set(0));
                let actual = prepared(&tree, tree.root(), &graph, css).targets;
                assert_eq!(actual, expected);
                assert_eq!(actual.iter().any(|target| target.source == source_id(1, pick)), matches_pick);
                if css.starts_with("*::before") {
                    assert!(actual.is_empty());
                    assert_eq!(ORDINARY_MATCHES.with(|count| count.get()), 0);
                }
            }
        }
    }

    #[test]
    fn nested_rules_keep_scoped_targets_and_conditional_groups() {
        let tree = Tree::parse("<body><section class=parent><input id=inside class=child></section><input id=outside class=child></body>");
        let graph = Composition::scopes(&tree).unwrap();
        let mut sheet: SheetSnapshot = serde_json::from_value(serde_json::json!({"rules":[{"type":1,"cssText":".parent { width:10px; & > .child { width:20px; } @media screen { & > .child { height:5px; } } }"}]})).unwrap();
        prepare_sheet(&tree, 1, tree.root(), &graph, &mut sheet).unwrap();
        assert_eq!(sheet.rules.len(), 3);
        assert_eq!(
            sheet.rules[1]
                .scoped
                .as_ref()
                .unwrap()
                .targets
                .iter()
                .map(|target| target.source)
                .collect::<Vec<_>>(),
            vec![source_id(1, tree.get_element_by_id("inside").unwrap())]
        );
        assert_eq!(sheet.rules[2].kind, 4);
        assert!(sheet.rules[2]
            .prelude
            .as_deref()
            .unwrap()
            .starts_with("@media screen"));
        assert_eq!(
            sheet.rules[2].rules.as_ref().unwrap()[0]
                .scoped
                .as_ref()
                .unwrap()
                .targets
                .len(),
            1
        );
    }

    #[test]
    fn nested_selector_lists_keep_max_specificity_and_declaration_order() {
        let tree =
            Tree::parse("<body><div class=parent><span id=child class=child></span></div></body>");
        let graph = Composition::scopes(&tree).unwrap();
        let mut sheet: SheetSnapshot = serde_json::from_value(serde_json::json!({"rules":[{"type":1,"cssText":".parent, #absent { & > .child { width:20px; } }"},{"type":1,"cssText":".parent { width:10px; & { width:20px; } width:30px; }"}]})).unwrap();
        prepare_sheet(&tree, 1, tree.root(), &graph, &mut sheet).unwrap();
        assert_eq!(
            sheet.rules[0].scoped.as_ref().unwrap().targets[0].specificity,
            (1, 1, 0)
        );
        let values = sheet.rules[1..]
            .iter()
            .flat_map(|rule| &rule.scoped.as_ref().unwrap().declarations)
            .map(|decl| decl.value.as_str())
            .collect::<Vec<_>>();
        assert_eq!(values, ["10px", "20px", "30px"]);
    }

    #[test]
    fn nested_shadow_rules_do_not_escape_their_original_tree_scope() {
        let mut tree =
            Tree::parse("<body><div id=host></div><input class=child id=outside></body>");
        let host = tree.get_element_by_id("host").unwrap();
        let shadow = tree.attach_shadow(host);
        let child = tree.create_element("input");
        tree.set_attribute(child, "class", "child");
        tree.append_child(shadow, child);
        let graph = Composition::scopes(&tree).unwrap();
        let mut sheet: SheetSnapshot = serde_json::from_value(serde_json::json!({"rules":[{"type":1,"cssText":":host { display:block; @media print { display:none; } & > .child { width:20px; } }"}]})).unwrap();
        prepare_sheet(&tree, 1, shadow, &graph, &mut sheet).unwrap();
        assert_eq!(sheet.rules.len(), 3);
        assert_eq!(
            sheet.rules[0].scoped.as_ref().unwrap().targets[0].source,
            source_id(1, host)
        );
        assert_eq!(
            sheet.rules[2]
                .scoped
                .as_ref()
                .unwrap()
                .targets
                .iter()
                .map(|target| target.source)
                .collect::<Vec<_>>(),
            vec![source_id(1, child)]
        );
        assert_eq!(sheet.rules[1].prelude.as_deref(), Some("@media print"));
    }

    #[test]
    fn nested_supports_and_container_conditions_remain_grouped() {
        let tree = Tree::parse("<body><div class=parent><span class=child></span></div></body>");
        let graph = Composition::scopes(&tree).unwrap();
        let mut sheet: SheetSnapshot = serde_json::from_value(serde_json::json!({"rules":[{"type":1,"cssText":".parent { @supports (display:grid) { & .child { width:20px; } } @container wide (min-width: 10px) { & .child { height:4px; } } }"}]})).unwrap();
        prepare_sheet(&tree, 1, tree.root(), &graph, &mut sheet).unwrap();
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rules[0].kind, 12);
        assert!(sheet.rules[0]
            .prelude
            .as_deref()
            .unwrap()
            .starts_with("@supports"));
        assert!(crate::live_stylesheets::is_container(&sheet.rules[1]));
        assert_eq!(
            sheet.rules[0].rules.as_ref().unwrap()[0]
                .scoped
                .as_ref()
                .unwrap()
                .targets
                .len(),
            1
        );
        assert_eq!(
            sheet.rules[1].rules.as_ref().unwrap()[0]
                .scoped
                .as_ref()
                .unwrap()
                .targets
                .len(),
            1
        );
    }

    fn legacy_targets(tree: &Tree, root: Handle, graph: &Composition, css: &str) -> Vec<Target> {
        let parsed = StyleSheet::parse(css, ParserOptions::default()).unwrap();
        let CssRule::Style(rule) = &parsed.rules.0[0] else {
            panic!("style expected")
        };
        let sources = selector_sources(css).unwrap();
        let matcher = Matcher { tree, root, graph };
        let mut targets = Vec::new();
        for &node in graph.scopes.keys() {
            if tree.node_type_id(node) != 1 {
                continue;
            }
            let mut specificity = None;
            for (selector, source) in rule.selectors.0.iter().zip(&sources) {
                let matches = if shadow_selector(selector) {
                    matcher.matches(selector, node, false).unwrap()
                } else {
                    graph.scopes.get(&node) == Some(&root)
                        && tree.matches_checked(node, source).unwrap()
                };
                if matches {
                    specificity = Some(specificity.unwrap_or(0).max(selector.specificity()));
                }
            }
            if let Some(value) = specificity {
                targets.push(Target {
                    source: source_id(1, node),
                    specificity: (value >> 20, (value >> 10) & 1023, value & 1023),
                });
            }
        }
        targets.sort_by_key(|target| target.source.0);
        targets
    }

    #[test]
    fn dynamic_type_candidates_bound_work_and_keep_live_and_namespace_matches() {
        use turbo_dom_parser::rtdom::tree::FormProp;
        let html = format!(
            "<body>{}<section class=scope><input id=a type=checkbox><input id=b type=checkbox></section><svg><a id=svg-first></a><a id=svg-second></a><foreignObject><a id=html-link></a></foreignObject></svg>",
            "<i></i>".repeat(1000)
        );
        let mut tree = Tree::parse(&html);
        let a = tree.get_element_by_id("a").unwrap();
        let b = tree.get_element_by_id("b").unwrap();
        let css = ".scope > input:checked,svg > a:nth-of-type(2),foreignObject > a:first-of-type{width:10px}";
        for checked in [false, true, false] {
            tree.set_form_property(a, FormProp::Checked, checked);
            tree.set_form_property(b, FormProp::Checked, !checked);
            let graph = Composition::scopes(&tree).unwrap();
            let expected = legacy_targets(&tree, tree.root(), &graph, css);
            ORDINARY_MATCHES.with(|count| count.set(0));
            let actual = prepared(&tree, tree.root(), &graph, css).targets;
            assert_eq!(actual, expected);
            assert_eq!(actual.len(), 3);
            assert!(actual
                .iter()
                .any(|target| target.source == source_id(1, if checked { a } else { b })));
            assert!(
                ORDINARY_MATCHES.with(|count| count.get()) <= 8,
                "unrelated elements reached dynamic matching: {}",
                ORDINARY_MATCHES.with(|count| count.get())
            );
        }
    }

    #[test]
    fn dynamic_type_candidates_keep_more_selective_class_buckets() {
        let html = format!(
            "<body>{}<input class=pick type=checkbox checked><input class=pick type=checkbox>",
            "<input>".repeat(1000)
        );
        let tree = Tree::parse(&html);
        let graph = Composition::scopes(&tree).unwrap();
        let css = "input.pick:checked{width:10px}";
        let expected = legacy_targets(&tree, tree.root(), &graph, css);
        ORDINARY_MATCHES.with(|count| count.set(0));
        let actual = prepared(&tree, tree.root(), &graph, css).targets;
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 1);
        assert!(ORDINARY_MATCHES.with(|count| count.get()) <= 2);
    }

    #[test]
    fn dynamic_selector_candidates_only_match_the_rightmost_class() {
        use turbo_dom_parser::rtdom::tree::FormProp;
        let html = format!("<body>{}<section class=scope><input id=a type=checkbox class='pick:target'><input id=b type=checkbox class='pick:target'></section><input id=outside type=checkbox class='pick:target' checked>", "<i></i>".repeat(1000));
        let mut tree = Tree::parse(&html);
        let a = tree.get_element_by_id("a").unwrap();
        let b = tree.get_element_by_id("b").unwrap();
        let css = r#".scope > .pick\:target:checked{width:10px}"#;
        for checked in [false, true, false] {
            tree.set_form_property(a, FormProp::Checked, checked);
            tree.set_form_property(b, FormProp::Checked, !checked);
            let graph = Composition::scopes(&tree).unwrap();
            let expected = legacy_targets(&tree, tree.root(), &graph, css);
            ORDINARY_MATCHES.with(|count| count.set(0));
            let actual = prepared(&tree, tree.root(), &graph, css).targets;
            assert_eq!(actual, expected);
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0].source, source_id(1, if checked { a } else { b }));
            assert!(
                ORDINARY_MATCHES.with(|count| count.get()) <= 3,
                "matching calls: {}",
                ORDINARY_MATCHES.with(|count| count.get())
            );
        }
    }

    #[test]
    fn dynamic_id_candidates_keep_duplicate_ids_and_live_state() {
        use turbo_dom_parser::rtdom::tree::FormProp;
        let mut tree = Tree::parse(
            "<body><input id=duplicate type=checkbox><input id=duplicate type=checkbox>",
        );
        let controls = tree.query_selector_all("input");
        let first = controls[0];
        let second = controls[1];
        for checked in [false, true, false] {
            tree.set_form_property(first, FormProp::Checked, checked);
            tree.set_form_property(second, FormProp::Checked, !checked);
            let graph = Composition::scopes(&tree).unwrap();
            let css = "#duplicate:checked{width:10px}";
            let expected = legacy_targets(&tree, tree.root(), &graph, css);
            ORDINARY_MATCHES.with(|count| count.set(0));
            let actual = prepared(&tree, tree.root(), &graph, css).targets;
            assert_eq!(actual, expected);
            assert_eq!(actual.len(), 1);
            assert!(
                ORDINARY_MATCHES.with(|count| count.get()) <= 2,
                "matching calls: {}",
                ORDINARY_MATCHES.with(|count| count.get())
            );
        }
    }

    #[test]
    fn document_selector_batches_preserve_legacy_targets_specificity_and_mutations() {
        let mut tree = Tree::parse("<body><div id=x class=pick></div><input id=c></body>");
        let x = tree.get_element_by_id("x").unwrap();
        for css in [
            ".pick,#x,body > .pick{width:10px}",
            ":root,:scope,body .pick{height:3px}",
        ] {
            let graph = Composition::scopes(&tree).unwrap();
            let expected = legacy_targets(&tree, tree.root(), &graph, css);
            BATCH_QUERIES.with(|count| count.set(0));
            assert_eq!(prepared(&tree, tree.root(), &graph, css).targets, expected);
            let expected_queries = if css.starts_with(".pick") { 3 } else { 1 };
            assert_eq!(BATCH_QUERIES.with(|count| count.get()), expected_queries);
            assert_eq!(prepared(&tree, tree.root(), &graph, css).targets, expected);
        }
        tree.set_attribute(x, "class", "other");
        let graph = Composition::scopes(&tree).unwrap();
        assert!(prepared(&tree, tree.root(), &graph, ".pick{width:10px}")
            .targets
            .is_empty());
        let child = tree.create_element("div");
        tree.set_attribute(child, "class", "pick");
        tree.append_child(x, child);
        let graph = Composition::scopes(&tree).unwrap();
        for css in [
            ".pick{width:10px}",
            "#x > .pick{width:30px}",
            "#x{width:40px}",
        ] {
            assert_eq!(
                prepared(&tree, tree.root(), &graph, css).targets,
                legacy_targets(&tree, tree.root(), &graph, css)
            );
        }
    }

    #[test]
    fn shadow_and_dynamic_pseudos_keep_per_node_matching_and_scope_isolation() {
        let mut tree = Tree::parse("<body><div id=host><input id=light class=pick></div></body>");
        let host = tree.get_element_by_id("host").unwrap();
        let light = tree.get_element_by_id("light").unwrap();
        let shadow = tree.attach_shadow(host);
        let inner = tree.create_element("input");
        tree.set_attribute(inner, "class", "pick");
        tree.append_child(shadow, inner);
        for checked in [false, true, false] {
            if checked {
                tree.set_attribute(light, "checked", "");
            } else {
                tree.remove_attribute(light, "checked");
            }
            let graph = Composition::scopes(&tree).unwrap();
            for (root, css) in [
                (tree.root(), ".pick{width:10px}"),
                (shadow, ".pick{width:20px}"),
                (shadow, ":host{width:30px}"),
                (tree.root(), ":checked,:focus,:active,:scope{height:8px}"),
            ] {
                BATCH_QUERIES.with(|count| count.set(0));
                assert_eq!(
                    prepared(&tree, root, &graph, css).targets,
                    legacy_targets(&tree, root, &graph, css)
                );
                if root == shadow || css.starts_with(':') {
                    assert_eq!(BATCH_QUERIES.with(|count| count.get()), 0);
                }
            }
            assert_eq!(
                prepared(&tree, tree.root(), &graph, ".pick{width:1px}")
                    .targets
                    .len(),
                1
            );
            assert_eq!(
                prepared(&tree, shadow, &graph, ".pick{width:1px}")
                    .targets
                    .len(),
                1
            );
        }
    }

    #[test]
    fn selector_validation_is_bounded_without_caching_dynamic_matches_or_scopes() {
        use turbo_dom_parser::rtdom::tree::FormProp;
        let mut tree = Tree::parse(
            "<body><div id=host></div><input id=c type=checkbox><x-state id=s></x-state></body>",
        );
        let host = tree.get_element_by_id("host").unwrap();
        let checked = tree.get_element_by_id("c").unwrap();
        let state = tree.get_element_by_id("s").unwrap();
        for _ in 0..100 {
            let element = tree.create_element("div");
            tree.append_child(host, element);
        }
        let shadow = tree.attach_shadow(host);
        let inner = tree.create_element("input");
        tree.set_attribute(inner, "type", "checkbox");
        tree.set_form_property(inner, FormProp::Checked, true);
        tree.append_child(shadow, inner);
        let css = "input:checked,x-state:state(active),div:nth-child(2n){width:10px}";
        for present in [false, true, false] {
            tree.set_form_property(checked, FormProp::Checked, present);
            tree.set_custom_state_utf16(
                state,
                &"active".encode_utf16().collect::<Vec<_>>(),
                present,
            );
            let graph = Composition::scopes(&tree).unwrap();
            for root in [tree.root(), shadow] {
                let expected = legacy_targets(&tree, root, &graph, css);
                ORDINARY_VALIDATIONS.with(|count| count.set(0));
                let actual = prepared(&tree, root, &graph, css).targets;
                assert_eq!(actual, expected);
                assert_eq!(ORDINARY_VALIDATIONS.with(|count| count.get()), 3);
                assert_eq!(
                    actual
                        .iter()
                        .any(|target| target.source == source_id(1, inner)),
                    root == shadow
                );
                if root == tree.root() {
                    for node in [checked, state] {
                        assert_eq!(
                            actual
                                .iter()
                                .any(|target| target.source == source_id(1, node)),
                            present
                        );
                    }
                }
            }
        }
        let mut empty = Composition::scopes(&tree).unwrap();
        empty.scopes.clear();
        ORDINARY_VALIDATIONS.with(|count| count.set(0));
        assert_eq!(
            prepared(&tree, tree.root(), &empty, css).targets,
            Vec::<Target>::new()
        );
        assert_eq!(ORDINARY_VALIDATIONS.with(|count| count.get()), 0);
        let mut validated = false;
        assert!(matches_validated(&tree, checked, "input[", &mut validated)
            .unwrap_err()
            .contains("native selector match failed"));
        assert!(!validated);
    }

    /// turbo-surf#117: a rule whose selector list Chrome drops (an unknown pseudo-class)
    /// matches nothing instead of failing the page; Google's view-transition rules are
    /// valid and never match an element; the rules around them keep matching.
    #[test]
    fn invalid_selector_rule_is_skipped_not_page_fatal() {
        let tree = Tree::parse("<body><div id=a class=pick></div></body>");
        let a = tree.get_element_by_id("a").unwrap();
        let graph = Composition::scopes(&tree).unwrap();
        let root = tree.root();
        let targets = |css: &str| prepared(&tree, root, &graph, css).targets;
        assert_eq!(targets("html:bogus .pick{width:1px}"), Vec::<Target>::new());
        assert_eq!(
            targets(".pick, html:bogus{width:1px}"),
            Vec::<Target>::new()
        );
        assert_eq!(
            targets("html:active-view-transition-type(aimc)::view-transition-new(sb):only-child{width:1px}"),
            Vec::<Target>::new()
        );
        let kept = targets(".pick, html:active-view-transition-type(aimc) .pick{width:1px}");
        assert_eq!(
            kept.iter().map(|t| t.source).collect::<Vec<_>>(),
            vec![source_id(1, a)]
        );
    }

    #[test]
    fn original_scopes_and_slot_assignment_are_distinct_from_flat_parentage() {
        let mut tree =
            Tree::parse("<body><div id=host><span id=light class=pick></span></div></body>");
        let host = tree.get_element_by_id("host").unwrap();
        let light = tree.get_element_by_id("light").unwrap();
        let shadow = tree.attach_shadow(host);
        let inner = tree.create_element("div");
        tree.set_attribute(inner, "class", "pick");
        tree.append_child(shadow, inner);
        let slot = tree.create_element("slot");
        tree.append_child(shadow, slot);
        let fallback = tree.create_element("span");
        tree.set_attribute(fallback, "class", "pick");
        tree.append_child(slot, fallback);
        let mut graph = Composition::scopes(&tree).unwrap();
        graph.parents.insert(light, slot);
        graph.parents.insert(fallback, slot);
        let matches = |root, selector: &str| {
            let rule = RuleSnapshot {
                kind: 1,
                css_text: format!("{selector} {{ width: 40px }}"),
                prelude: None,
                rules: None,
                media: String::new(),
                style_sheet: None,
                scoped: None,
            };
            prepare_rule(&tree, 1, root, &graph, &rule)
                .unwrap()
                .targets
                .into_iter()
                .map(|target| target.source)
                .collect::<Vec<_>>()
        };
        assert_eq!(matches(tree.root(), ".pick"), vec![source_id(1, light)]);
        assert_eq!(matches(shadow, ":host"), vec![source_id(1, host)]);
        assert_eq!(matches(shadow, ":is(:host)"), vec![source_id(1, host)]);
        assert_eq!(matches(shadow, ":host > .pick"), vec![source_id(1, inner)]);
        assert_eq!(
            matches(shadow, "slot::slotted(.pick)"),
            vec![source_id(1, light)]
        );
        assert_eq!(
            matches(shadow, "slot > .pick"),
            vec![source_id(1, fallback)]
        );
        assert!(matches(shadow, "#host .pick").is_empty());
    }
    #[test]
    fn repeated_css_compiles_once_but_rematches_live_state_and_document_identity() {
        use turbo_dom_parser::rtdom::tree::FormProp;
        let mut tree = Tree::parse("<body><input id=pick class=compileplan-live type=checkbox>");
        let node = tree.get_element_by_id("pick").unwrap();
        let css = ".compileplan-live:checked{width:77px}";
        RULE_COMPILATIONS.with(|count| count.set(0));
        for index in 0..20 {
            let checked = index % 2 == 0;
            tree.set_form_property(node, FormProp::Checked, checked);
            let graph = Composition::scopes(&tree).unwrap();
            let rule = prepared(&tree, tree.root(), &graph, css);
            assert_eq!(rule.targets.len(), usize::from(checked));
            assert_eq!(rule.declarations[0].value, "77px");
        }
        assert_eq!(RULE_COMPILATIONS.with(|count| count.get()), 1);
        let other =
            Tree::parse("<body><input id=other class=compileplan-live type=checkbox checked>");
        let graph = Composition::scopes(&other).unwrap();
        let rule = prepared_document(&other, 9, other.root(), &graph, css);
        assert_eq!(
            rule.targets[0].source,
            source_id(9, other.get_element_by_id("other").unwrap())
        );
        assert_eq!(RULE_COMPILATIONS.with(|count| count.get()), 1);
        let rule = prepared(
            &other,
            other.root(),
            &graph,
            ".compileplan-live:checked{width:88px}",
        );
        assert_eq!(rule.declarations[0].value, "88px");
        assert_eq!(RULE_COMPILATIONS.with(|count| count.get()), 2);
    }
}
