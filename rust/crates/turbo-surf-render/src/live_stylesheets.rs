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
        append_rules(&sheet.rules, conditions, result)?;
    }
    Ok(())
}

fn append_rules(
    rules: &[RuleSnapshot],
    conditions: &impl Conditions,
    result: &mut Vec<ActiveRule>,
) -> Result<(), String> {
    for rule in rules {
        match rule.kind {
            3 if conditions.media(&rule.media)? => {
                if let Some(sheet) = &rule.style_sheet {
                    append_sheet(sheet, conditions, result)?;
                }
            }
            3 => {}
            4 | 12 => {
                let prefix = if rule.kind == 4 {
                    "@media"
                } else {
                    "@supports"
                };
                let prelude = rule
                    .prelude
                    .as_deref()
                    .and_then(|text| text.strip_prefix(prefix))
                    .ok_or("native CSS grouping rule has no trusted prelude")?;
                let applies = if rule.kind == 4 {
                    conditions.media(prelude.trim())?
                } else {
                    conditions.supports(prelude.trim())?
                };
                if applies {
                    append_rules(
                        rule.rules
                            .as_deref()
                            .ok_or("native CSS grouping rule has no private child list")?,
                        conditions,
                        result,
                    )?;
                }
            }
            1 => result.push(ActiveRule {
                text: rule.css_text.clone(),
                scoped: rule.scoped.clone(),
                resource: None,
            }),
            5 | 7 => result.push(ActiveRule {
                text: rule.css_text.clone(),
                scoped: None,
                resource: Some(crate::live_layout_resources::definition(&rule.css_text)?),
            }),
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
}
