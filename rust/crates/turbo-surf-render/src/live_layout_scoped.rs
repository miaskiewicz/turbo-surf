//! Match selectors before slot placement changes physical ancestry. Snapshot
//! targets retain native identity and the original encapsulation depth.
use crate::live_layout::source_id;
use crate::live_layout_composed::Composition;
use crate::live_stylesheets::{RuleSnapshot, SheetSnapshot};
use cssparser::ToCss;
use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::{Property, PropertyId};
use lightningcss::rules::CssRule;
use lightningcss::selector::{Combinator, Component, Selector};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
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
    for rule in &mut sheet.rules {
        if rule.kind == 1 {
            rule.scoped = Some(prepare_rule(tree, document, root, graph, rule)?);
        }
        // Keyframe percentages are animation steps, never DOM selectors.
        if let Some(rules) = rule.rules.as_mut().filter(|_| matches!(rule.kind, 4 | 12)) {
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
    let parsed = StyleSheet::parse(&rule.css_text, ParserOptions::default())
        .map_err(|error| format!("native scoped CSS parse failed: {error:?}"))?;
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
    let matcher = Matcher { tree, root, graph };
    let mut specificity_by_node = std::collections::HashMap::<Handle, u32>::new();
    for (selector, source) in style.selectors.0.iter().zip(&sources) {
        // Native query results are version-cached. Context-sensitive and dynamic
        // pseudos retain per-node matching: their state need not bump tree.version.
        let batched = root == tree.root() && !source.contains(':');
        if batched {
            #[cfg(test)]
            BATCH_QUERIES.with(|count| count.set(count.get() + 1));
            let nodes = tree
                .query_selector_all_checked(source)
                .map_err(|error| format!("native selector match failed: {error:?}"))?;
            for &node in nodes.iter() {
                if graph.scopes.get(&node) == Some(&root) {
                    let value = specificity_by_node.entry(node).or_default();
                    *value = (*value).max(selector.specificity());
                }
            }
        } else {
            let shadow = shadow_selector(selector);
            let mut validated = false;
            for &node in graph.scopes.keys() {
                if tree.node_type_id(node) != 1 {
                    continue;
                }
                let matches = if shadow {
                    matcher.matches(selector, node, false)?
                } else {
                    graph.scopes.get(&node) == Some(&root)
                        && matches_validated(tree, node, source, &mut validated)?
                };
                if matches {
                    let value = specificity_by_node.entry(node).or_default();
                    *value = (*value).max(selector.specificity());
                }
            }
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
        depth: *graph
            .depths
            .get(&root)
            .ok_or("native CSS scope depth missing")?,
        targets,
    })
}

fn matches_validated(
    tree: &Tree,
    node: Handle,
    source: &str,
    validated: &mut bool,
) -> Result<bool, String> {
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
        let mut host = false;
        let mut slotted = false;
        for part in parts {
            match part {
                Component::Host(selector) => {
                    host = true;
                    if self.graph.hosts.get(&self.root) != Some(&node) {
                        return Ok(false);
                    }
                    if let Some(selector) = selector {
                        if !self.matches(selector, node, true)? {
                            return Ok(false);
                        }
                    }
                }
                Component::Slotted(selector) => {
                    slotted = true;
                    let assigned = self.graph.parents.get(&node).is_some_and(|slot| {
                        self.tree.local_name(*slot) == Some("slot")
                            && self.tree.parent(node) != Some(*slot)
                            && self.graph.scopes.get(slot) == Some(&self.root)
                    });
                    if !assigned || !self.matches(selector, node, true)? {
                        return Ok(false);
                    }
                }
                Component::Is(selectors)
                | Component::Where(selectors)
                | Component::Negation(selectors) => {
                    let mut any = false;
                    for selector in selectors {
                        any |= self.matches(selector, node, argument)?;
                    }
                    let negation = matches!(part, Component::Negation(_));
                    if any == negation {
                        return Ok(false);
                    }
                    // A featureless host may participate through :is(:host)
                    // or :where(:host), but not through an ordinary negation.
                    host |= !negation && any && self.graph.hosts.get(&self.root) == Some(&node);
                }
                _ => part
                    .to_css(&mut ordinary)
                    .map_err(|_| "native selector serialization failed")?,
            }
        }
        if !argument && self.graph.scopes.get(&node) != Some(&self.root) && !host && !slotted {
            return Ok(false);
        }
        if ordinary.is_empty() {
            return Ok(true);
        }
        self.tree
            .matches_checked(node, &ordinary)
            .map_err(|error| format!("native selector match failed: {error:?}"))
    }
}

/// The UA stylesheet (turbo-dom-layout's + surf's live supplement + `slot`) matched
/// in every tree scope of `graph`.
pub(crate) fn ua_rules(
    tree: &Tree,
    document: u32,
    graph: &Composition,
) -> Result<Vec<crate::live_stylesheets::ActiveRule>, String> {
    let css = format!(
        "{} {} slot{{display:contents}}",
        turbo_dom_layout::style::user_agent_css(),
        crate::live_layout::LIVE_UA
    );
    let parsed = StyleSheet::parse(&css, ParserOptions::default())
        .map_err(|error| format!("native UA CSS parse failed: {error:?}"))?;
    let mut result = Vec::new();
    for &root in graph.depths.keys() {
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
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
thread_local! {static ORDINARY_VALIDATIONS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

#[cfg(test)]
thread_local! {static BATCH_QUERIES:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(tree: &Tree, root: Handle, graph: &Composition, css: &str) -> ScopedRule {
        prepare_rule(
            tree,
            1,
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
}
