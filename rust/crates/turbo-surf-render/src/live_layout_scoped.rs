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
use turbo_html2pdf_core::layout_snapshot::SourceId;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Target {
    pub(crate) source: SourceId,
    pub(crate) specificity: (u32, u32, u32),
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScopedRule {
    pub(crate) depth: u32,
    pub(crate) declarations: Vec<turbo_html2pdf_core::style::Declaration>,
    pub(crate) targets: Vec<Target>,
}

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
    let mut targets = Vec::new();
    for &node in graph.scopes.keys() {
        if tree.node_type_id(node) != 1 {
            continue;
        }
        let mut specificity = None;
        for (selector, source) in style.selectors.0.iter().zip(&sources) {
            let matches = if shadow_selector(selector) {
                matcher.matches(selector, node, false)?
            } else {
                graph.scopes.get(&node) == Some(&root)
                    && tree
                        .matches_checked(node, source)
                        .map_err(|error| format!("native selector match failed: {error:?}"))?
            };
            if matches {
                specificity = Some(specificity.unwrap_or(0).max(selector.specificity()));
            }
        }
        if let Some(value) = specificity {
            targets.push(Target {
                source: source_id(document, node),
                specificity: (value >> 20, (value >> 10) & 1023, value & 1023),
            });
        }
    }
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
) -> Result<Vec<turbo_html2pdf_core::style::Declaration>, String> {
    use lightningcss::stylesheet::PrinterOptions;
    let mut result = Vec::new();
    for (important, properties) in [
        (false, &block.declarations),
        (true, &block.important_declarations),
    ] {
        for property in properties {
            // The producer inherits font longhands. Expand the parser's typed
            // shorthand before that seam, preserving resets and source order.
            let expanded = if matches!(property, Property::Font(_) | Property::Animation(..)) {
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
                    result.push(turbo_html2pdf_core::style::Declaration {
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
                result.extend(turbo_html2pdf_core::style::parse_declarations(&text));
            }
        }
    }
    Ok(result)
}

fn inline_declarations(text: &str) -> Result<Vec<turbo_html2pdf_core::style::Declaration>, String> {
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

pub(crate) fn ua_rules(
    tree: &Tree,
    document: u32,
    graph: &Composition,
) -> Result<Vec<crate::live_stylesheets::ActiveRule>, String> {
    let css = format!(
        "{} {} slot{{display:contents}}",
        turbo_html2pdf_core::style::user_agent_css(),
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

pub(crate) fn style_composed_tree(
    input: &crate::live_layout::LiveTreeInput,
    rules: &[crate::live_stylesheets::ActiveRule],
) -> Result<Vec<turbo_html2pdf_core::StyledNode>, String> {
    use turbo_html2pdf_core::style::{CascadeOrigin, MatchedDeclaration};
    let graph = input
        .composition
        .as_ref()
        .ok_or("native composed style graph unavailable")?;
    let mut declarations = std::collections::HashMap::<SourceId, Vec<MatchedDeclaration>>::new();
    let mut count = 0usize;
    for (origin, rules) in [
        (CascadeOrigin::UserAgent, graph.ua_rules.as_slice()),
        (CascadeOrigin::Author, rules),
    ] {
        for (order, rule) in rules.iter().enumerate() {
            let scoped = rule
                .scoped
                .as_ref()
                .ok_or("native stylesheet scope provenance missing")?;
            for target in &scoped.targets {
                for declaration in &scoped.declarations {
                    count += 1;
                    if count > 500_000 {
                        return Err("native scoped declaration budget exceeded".into());
                    }
                    declarations
                        .entry(target.source)
                        .or_default()
                        .push(MatchedDeclaration {
                            declaration: declaration.clone(),
                            origin,
                            scope_depth: scoped.depth,
                            inline: false,
                            specificity: target.specificity,
                            order,
                        });
                }
            }
        }
    }
    let identities = input
        .identities
        .iter()
        .map(|source| (source.path.clone(), source.id))
        .collect::<std::collections::HashMap<_, _>>();
    struct Styler<'a> {
        graph: &'a Composition,
        identities: &'a std::collections::HashMap<Vec<usize>, SourceId>,
        declarations: &'a std::collections::HashMap<SourceId, Vec<MatchedDeclaration>>,
    }
    impl Styler<'_> {
        fn nodes(
            &self,
            nodes: &[turbo_html2pdf_core::Node],
            path: &[usize],
            parent: &turbo_html2pdf_core::ComputedStyle,
        ) -> Result<Vec<turbo_html2pdf_core::StyledNode>, String> {
            use turbo_html2pdf_core::{Node, StyledNode};
            let mut result = Vec::with_capacity(nodes.len());
            for (index, node) in nodes.iter().enumerate() {
                let mut path = path.to_vec();
                path.push(index);
                match node {
                    Node::Text(text) => result.push(StyledNode::Text(text.clone())),
                    Node::Element(element) => {
                        let source = *self
                            .identities
                            .get(&path)
                            .ok_or("native composed source identity unavailable")?;
                        let handle = Handle::from_raw(source.0 as u32);
                        let root = self
                            .graph
                            .scopes
                            .get(&handle)
                            .ok_or("native composed source scope unavailable")?;
                        let depth = *self
                            .graph
                            .depths
                            .get(root)
                            .ok_or("native composed source depth unavailable")?;
                        let mut declarations = Vec::new();
                        for declaration in
                            turbo_html2pdf_core::style::presentational_declarations(element)
                        {
                            declarations.push(MatchedDeclaration {
                                declaration,
                                origin: CascadeOrigin::Author,
                                scope_depth: depth,
                                inline: false,
                                specificity: (0, 0, 0),
                                order: 0,
                            });
                        }
                        declarations.extend(
                            self.declarations
                                .get(&source)
                                .into_iter()
                                .flatten()
                                .cloned()
                                .map(|mut d| {
                                    d.order += 1;
                                    d
                                }),
                        );
                        if let Some(style) = element
                            .attrs
                            .iter()
                            .find(|attribute| attribute.name == "style")
                        {
                            for declaration in inline_declarations(&style.value)? {
                                declarations.push(MatchedDeclaration {
                                    declaration,
                                    origin: CascadeOrigin::Author,
                                    scope_depth: depth,
                                    inline: true,
                                    specificity: (0, 0, 0),
                                    order: usize::MAX,
                                });
                            }
                        }
                        let style = turbo_html2pdf_core::style::resolve_matched_style(
                            parent,
                            &declarations,
                        );
                        let children = self.nodes(&element.children, &path, &style)?;
                        result.push(StyledNode::Element(
                            turbo_html2pdf_core::style::StyledElement {
                                tag: element.tag.clone(),
                                attrs: element.attrs.clone(),
                                style,
                                children,
                            },
                        ));
                    }
                }
            }
            Ok(result)
        }
    }
    Styler {
        graph,
        identities: &identities,
        declarations: &declarations,
    }
    .nodes(
        &input.nodes,
        &[],
        &turbo_html2pdf_core::ComputedStyle::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
