//! Typed, geometry-only admission. Unsupported at-rules are never dropped by
//! number; declarations must prove no eligible container or layout dependency.
use crate::live_layout_resources::Definition;
use lightningcss::declaration::DeclarationBlock;
use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Uses {
    container: bool,
    edges: HashMap<String, HashSet<String>>,
}
impl Uses {
    fn declarations(&mut self, block: &DeclarationBlock) -> Result<(), String> {
        for property in block
            .declarations
            .iter()
            .chain(&block.important_declarations)
        {
            let name = property.property_id().name().to_owned();
            self.container |= matches!(
                name.as_str(),
                "container" | "container-name" | "container-type"
            );
            let text = property
                .to_css_string(false, PrinterOptions::default())
                .map_err(|_| "native layout cannot serialize at-rule dependency")?;
            for reference in references(&text)? {
                self.edges
                    .entry(reference)
                    .or_default()
                    .insert(name.clone());
            }
        }
        Ok(())
    }
    fn rules(&mut self, rules: &[CssRule]) -> Result<(), String> {
        for rule in rules {
            match rule {
                CssRule::Style(rule) => {
                    self.declarations(&rule.declarations)?;
                    self.rules(&rule.rules.0)?;
                }
                CssRule::Container(rule) => self.rules(&rule.rules.0)?,
                CssRule::Media(rule) => self.rules(&rule.rules.0)?,
                CssRule::Supports(rule) => self.rules(&rule.rules.0)?,
                CssRule::Keyframes(rule) => {
                    for frame in &rule.keyframes {
                        self.declarations(&frame.declarations)?;
                    }
                }
                CssRule::FontFace(_) | CssRule::Property(_) | CssRule::ViewTransition(_) => {}
                _ => {
                    return Err(
                        "native layout cannot prove unknown nested at-rule geometry neutral".into(),
                    )
                }
            }
        }
        Ok(())
    }
    fn color(&self, name: &str) -> Result<(), String> {
        let mut pending = vec![name.to_owned()];
        let mut seen = HashSet::new();
        while let Some(source) = pending.pop() {
            if !seen.insert(source.clone()) {
                continue;
            }
            for target in self.edges.get(&source).into_iter().flatten() {
                if target.starts_with("--") {
                    pending.push(target.clone());
                } else if !matches!(
                    target.as_str(),
                    "background" | "background-color" | "background-image" | "color"
                ) {
                    return Err(format!("native live layout registered color has unsupported geometry dependency: {name} -> {target}"));
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn validate(
    input: &crate::live_layout::LiveTreeInput,
    rules: &[crate::live_stylesheets::ActiveRule],
) -> Result<(), String> {
    let definitions = rules
        .iter()
        .filter_map(|rule| rule.resource.as_ref())
        .collect::<Vec<_>>();
    if !definitions.iter().any(|value| {
        matches!(
            value,
            Definition::Container(_) | Definition::RegisteredColor(_)
        )
    }) {
        return Ok(());
    }
    let mut uses = Uses::default();
    for rule in rules {
        let parsed = StyleSheet::parse(&rule.text, ParserOptions::default())
            .map_err(|_| "native layout cannot parse at-rule dependency source")?;
        uses.rules(&parsed.rules.0)?;
    }
    let mut pending = input.nodes.iter().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if let turbo_dom_layout::Node::Element(element) = node {
            for attr in element.attrs.iter().filter(|attr| attr.name == "style") {
                let block = DeclarationBlock::parse_string(&attr.value, ParserOptions::default())
                    .map_err(|_| "native layout cannot parse inline at-rule dependency")?;
                uses.declarations(&block)?;
            }
            pending.extend(&element.children);
        }
    }
    for definition in definitions {
        match definition {
            Definition::Container(name) if uses.container => {
                return Err(format!(
                    "native live layout cannot prove named container query inactive: {name}"
                ))
            }
            Definition::RegisteredColor(name) => uses.color(name)?,
            _ => {}
        }
    }
    Ok(())
}
fn references(source: &str) -> Result<HashSet<String>, String> {
    let mut input = cssparser::ParserInput::new(source);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut result = HashSet::new();
    scan(&mut parser, &mut result, 0)
        .map_err(|_| "native layout cannot resolve CSS variable dependency syntax")?;
    Ok(result)
}
fn scan<'i, 't>(
    input: &mut cssparser::Parser<'i, 't>,
    refs: &mut HashSet<String>,
    depth: usize,
) -> Result<(), cssparser::ParseError<'i, ()>> {
    if depth > 128 {
        return Err(input.new_custom_error(()));
    }
    while !input.is_exhausted() {
        let token = input.next_including_whitespace_and_comments()?.clone();
        match token {
            cssparser::Token::Function(name) => input.parse_nested_block(|input| {
                if name.eq_ignore_ascii_case("var") {
                    let value = input.expect_ident_cloned()?;
                    if !value.starts_with("--") {
                        return Err(input.new_custom_error(()));
                    }
                    refs.insert(value.to_string());
                }
                scan(input, refs, depth + 1)
            })?,
            cssparser::Token::ParenthesisBlock
            | cssparser::Token::SquareBracketBlock
            | cssparser::Token::CurlyBracketBlock => {
                input.parse_nested_block(|input| scan(input, refs, depth + 1))?
            }
            cssparser::Token::BadString(_) | cssparser::Token::BadUrl(_) => {
                return Err(input.new_custom_error(()))
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variable_graph_decodes_escapes_fallbacks_nested_functions_and_rejects_geometry() {
        let refs =
            references(r"background:radial-gradient(var(--p\61 int),var(--other,var(--fallback)))")
                .unwrap();
        assert_eq!(
            refs,
            HashSet::from([
                "--paint".to_string(),
                "--other".to_string(),
                "--fallback".to_string()
            ])
        );
        assert!(references(r#"content:"var(--literal)""#)
            .unwrap()
            .is_empty());
        let block = DeclarationBlock::parse_string(
            "--alias:var(--paint);background:var(--alias)",
            ParserOptions::default(),
        )
        .unwrap();
        let mut uses = Uses::default();
        uses.declarations(&block).unwrap();
        uses.color("--paint").unwrap();
        let block =
            DeclarationBlock::parse_string("width:var(--alias,10px)", ParserOptions::default())
                .unwrap();
        uses.declarations(&block).unwrap();
        assert!(uses.color("--paint").unwrap_err().contains("width"));
    }
    #[test]
    fn registered_geometry_neutrality_does_not_ignore_inline_important_or_shorthands() {
        for source in [
            "border:var(--paint,10px solid red)",
            "padding:var(--paint,10px)!important",
            "--alias:var(--paint);transform:translateX(var(--alias))",
        ] {
            let mut uses = Uses::default();
            uses.declarations(
                &DeclarationBlock::parse_string(source, ParserOptions::default()).unwrap(),
            )
            .unwrap();
            assert!(uses.color("--paint").is_err(), "{source}");
        }
    }
    /// Opt-in replay of an existing saved CSS capture; no scripts or network.
    #[test]
    fn captured_css_dependency_graph_can_be_replayed_offline() {
        use lightningcss::traits::ToCss;
        let Some(path) = std::env::var_os("TURBO_SURF_CAPTURED_CSS_REPLAY") else {
            return;
        };
        let source = std::fs::read_to_string(path).unwrap();
        let parsed = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
        let html = std::env::var_os("TURBO_SURF_CAPTURED_HTML_REPLAY")
            .map(|path| std::fs::read_to_string(path).unwrap())
            .unwrap_or_else(|| "<!doctype html><body>".into());
        let tree = turbo_dom_parser::rtdom::Tree::parse(&html);
        let input = crate::live_layout::capture_tree(&tree, 1).unwrap();
        let rules = parsed
            .rules
            .0
            .iter()
            .map(|rule| {
                let text = rule.to_css_string(PrinterOptions::default()).unwrap();
                let resource = if matches!(
                    rule,
                    CssRule::Container(_) | CssRule::Property(_) | CssRule::ViewTransition(_)
                ) {
                    Some(crate::live_layout_resources::definition(&text).unwrap())
                } else {
                    None
                };
                crate::live_stylesheets::ActiveRule {
                    text,
                    scoped: None,
                    resource,
                }
            })
            .collect::<Vec<_>>();
        validate(&input, &rules).unwrap();
    }
}
