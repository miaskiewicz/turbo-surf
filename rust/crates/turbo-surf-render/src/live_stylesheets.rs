//! Apply the private CSSHost graph in true sheet/rule order. Policy and condition
//! evaluation belong to the host, rather than a page-replaceable JS getter.
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SheetSnapshot {
    #[serde(default)]
    pub(crate) cycle: bool,
    #[serde(default)]
    pub(crate) disabled: bool,
    #[serde(default)]
    pub(crate) media: String,
    #[serde(default)]
    pub(crate) rules: Vec<RuleSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuleSnapshot {
    #[serde(skip)]
    pub(crate) scoped: Option<crate::live_layout_scoped::ScopedRule>,
    #[serde(rename = "type")]
    pub(crate) kind: u32,
    pub(crate) css_text: String,
    #[serde(default)]
    pub(crate) prelude: Option<String>,
    #[serde(default)]
    pub(crate) rules: Option<Vec<RuleSnapshot>>,
    #[serde(default)]
    pub(crate) media: String,
    #[serde(default)]
    pub(crate) style_sheet: Option<SheetSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveRule {
    pub(crate) text: String,
    pub(crate) scoped: Option<crate::live_layout_scoped::ScopedRule>,
    pub(crate) resource: Option<crate::live_layout_resources::Definition>,
    /// Enclosing `@container` queries (outermost first); all must hold per target.
    pub(crate) containers: Vec<crate::live_container_queries::Query>,
}

pub(crate) trait Conditions {
    fn media(&self, query: &str) -> Result<bool, String>;
    fn supports(&self, condition: &str) -> Result<bool, String>;
}

/// Qualified CSS rules are kept separate so a grouping block before a following
/// ordinary rule cannot be moved after it by a parser's at-rule side list.
#[cfg(test)]
pub(crate) fn active_rules(
    sheets: &[SheetSnapshot],
    conditions: &impl Conditions,
) -> Result<Vec<String>, String> {
    Ok(active_rule_snapshots(sheets, conditions)?
        .into_iter()
        .map(|rule| rule.text)
        .collect())
}

pub(crate) fn active_rule_snapshots(
    sheets: &[SheetSnapshot],
    conditions: &impl Conditions,
) -> Result<Vec<ActiveRule>, String> {
    let mut result = Vec::new();
    for sheet in sheets {
        append_sheet(sheet, conditions, &mut result)?;
    }
    Ok(result)
}

fn append_sheet(
    sheet: &SheetSnapshot,
    conditions: &impl Conditions,
    result: &mut Vec<ActiveRule>,
) -> Result<(), String> {
    if !sheet.cycle && !sheet.disabled && conditions.media(&sheet.media)? {
        append_rules(&sheet.rules, conditions, &[], result)?;
    }
    Ok(())
}

fn append_rules(
    rules: &[RuleSnapshot],
    conditions: &impl Conditions,
    containers: &[crate::live_container_queries::Query],
    result: &mut Vec<ActiveRule>,
) -> Result<(), String> {
    for rule in rules {
        match rule.kind {
            0 if is_container(rule) => append_container(rule, conditions, containers, result)?,
            3 => append_import(rule, conditions, result)?,
            4 | 12 => append_grouping(rule, conditions, containers, result)?,
            1 => result.push(ActiveRule {
                text: rule.css_text.clone(),
                scoped: rule.scoped.clone(),
                resource: None,
                containers: containers.to_vec(),
            }),
            0 | 5 | 7 => result.push(resource_rule(rule)?),
            _ => {
                return Err(format!(
                    "native live layout does not support CSS rule type {}",
                    rule.kind
                ))
            }
        }
    }
    Ok(())
}

/// An `@container` block: its children apply under one more enclosing query.
fn append_container(
    rule: &RuleSnapshot,
    conditions: &impl Conditions,
    containers: &[crate::live_container_queries::Query],
    result: &mut Vec<ActiveRule>,
) -> Result<(), String> {
    let prelude = rule
        .prelude
        .as_deref()
        .ok_or("native CSS container rule has no trusted prelude")?;
    let mut nested = containers.to_vec();
    nested.push(crate::live_container_queries::parse(prelude)?);
    append_rules(
        rule.rules
            .as_deref()
            .ok_or("native CSS container rule has no private child list")?,
        conditions,
        &nested,
        result,
    )
}

/// An `@import` whose media list holds contributes its sheet in place.
fn append_import(
    rule: &RuleSnapshot,
    conditions: &impl Conditions,
    result: &mut Vec<ActiveRule>,
) -> Result<(), String> {
    if conditions.media(&rule.media)? {
        if let Some(sheet) = &rule.style_sheet {
            append_sheet(sheet, conditions, result)?;
        }
    }
    Ok(())
}

/// An `@media` (4) or `@supports` (12) block whose condition holds.
fn append_grouping(
    rule: &RuleSnapshot,
    conditions: &impl Conditions,
    containers: &[crate::live_container_queries::Query],
    result: &mut Vec<ActiveRule>,
) -> Result<(), String> {
    let media = rule.kind == 4;
    let prefix = if media { "@media" } else { "@supports" };
    let prelude = rule
        .prelude
        .as_deref()
        .and_then(|text| text.strip_prefix(prefix))
        .ok_or("native CSS grouping rule has no trusted prelude")?
        .trim();
    let applies = if media {
        conditions.media(prelude)?
    } else {
        conditions.supports(prelude)?
    };
    if !applies {
        return Ok(());
    }
    append_rules(
        rule.rules
            .as_deref()
            .ok_or("native CSS grouping rule has no private child list")?,
        conditions,
        containers,
        result,
    )
}

/// A definition rule (`@font-face` 5, `@keyframes` 7, or an unnamed-type 0 rule such
/// as `@property` / `@view-transition`) that layout admits as a resource.
fn resource_rule(rule: &RuleSnapshot) -> Result<ActiveRule, String> {
    let definition = crate::live_layout_resources::definition(&rule.css_text);
    let resource = if rule.kind == 0 {
        definition.map_err(|error| {
            format!("native live layout does not support CSS rule type 0: {error}")
        })?
    } else {
        definition?
    };
    Ok(ActiveRule {
        text: rule.css_text.clone(),
        scoped: None,
        resource: Some(resource),
        containers: Vec::new(),
    })
}

/// A CSSOM `@container` block: an unnamed-type (0) rule with a child list.
pub(crate) fn is_container(rule: &RuleSnapshot) -> bool {
    rule.kind == 0
        && rule.rules.is_some()
        && rule
            .prelude
            .as_deref()
            .is_some_and(|prelude| prelude.starts_with("@container"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestConditions;
    impl Conditions for TestConditions {
        fn media(&self, query: &str) -> Result<bool, String> {
            match query {
                "" | "screen" => Ok(true),
                "print" => Ok(false),
                _ => Err("unimplemented media feature".into()),
            }
        }
        fn supports(&self, condition: &str) -> Result<bool, String> {
            match condition {
                "(display: block)" => Ok(true),
                "(unknown: invalid)" => Ok(false),
                _ => Err("unimplemented supports feature".into()),
            }
        }
    }
    fn sheet(value: serde_json::Value) -> SheetSnapshot {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn real_sheet_and_import_order_is_retained_across_groups_and_disabled_sheets() {
        let live = sheet(
            serde_json::json!({"href":"https://layout.test/main.css","originClean":true,"media":"screen","rules":[
            {"type":3,"cssText":"@import url(child.css);","media":"screen","styleSheet":{"originClean":false,"href":"https://other.test/child.css","rules":[{"type":1,"cssText":".x{width:10px}"}]}},
            {"type":4,"cssText":"@media screen {.x{width:20px}}","prelude":"@media screen","rules":[{"type":1,"cssText":".x{width:20px}"}]},
            {"type":1,"cssText":".x{width:30px}"}]}),
        );
        let disabled = sheet(
            serde_json::json!({"disabled":true,"rules":[{"type":1,"cssText":".x{width:40px}"}]}),
        );
        assert_eq!(
            active_rules(&[live, disabled], &TestConditions).unwrap(),
            [".x{width:10px}", ".x{width:20px}", ".x{width:30px}"]
        );
    }
    #[test]
    fn inactive_and_cyclic_imports_never_apply_and_unimplemented_conditions_are_explicit() {
        let cyclic = sheet(serde_json::json!({"cycle":true}));
        let inactive = sheet(
            serde_json::json!({"media":"print","rules":[{"type":1,"cssText":".x{width:10px}"}]}),
        );
        assert!(active_rules(&[cyclic, inactive], &TestConditions)
            .unwrap()
            .is_empty());
        let unknown = sheet(serde_json::json!({"media":"(hover: hover)","rules":[]}));
        assert_eq!(
            active_rules(&[unknown], &TestConditions).unwrap_err(),
            "unimplemented media feature"
        );
    }
    #[test]
    fn actual_conditional_children_and_missing_private_provenance_are_distinct() {
        let supported = sheet(
            serde_json::json!({"rules":[{"type":12,"cssText":"@supports (display: block){.x{width:10px}}","prelude":"@supports (display: block)","rules":[{"type":1,"cssText":".x{width:10px}"}]}]}),
        );
        assert_eq!(
            active_rules(&[supported], &TestConditions).unwrap(),
            [".x{width:10px}"]
        );
        let malformed =
            sheet(serde_json::json!({"rules":[{"type":4,"cssText":"@media screen{}"}]}));
        assert!(active_rules(&[malformed], &TestConditions)
            .unwrap_err()
            .contains("trusted prelude"));
        let unknown = sheet(serde_json::json!({"rules":[{"type":0,"cssText":"@layer x{}"}]}));
        assert!(active_rules(&[unknown], &TestConditions)
            .unwrap_err()
            .contains("rule type 0"));
    }
    #[test]
    fn imports_with_unmatched_media_are_skipped_and_unhandled_rule_types_are_refused() {
        let print_import = sheet(serde_json::json!({"rules":[
            {"type":3,"cssText":"@import url(p.css) print;","media":"print","styleSheet":{"rules":[{"type":1,"cssText":".x{width:10px}"}]}},
            {"type":3,"cssText":"@import url(gone.css);","media":""},
            {"type":1,"cssText":".x{width:20px}"}]}));
        assert_eq!(
            active_rules(&[print_import], &TestConditions).unwrap(),
            [".x{width:20px}"]
        );
        for kind in [2, 6, 10, 11] {
            let rule = sheet(serde_json::json!({"rules":[{"type":kind,"cssText":"@x{}"}]}));
            assert_eq!(
                active_rules(&[rule], &TestConditions).unwrap_err(),
                format!("native live layout does not support CSS rule type {kind}")
            );
        }
    }
}
