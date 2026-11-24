//! Retain only context-free CSS plans; every read still matches the current tree.
use super::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

type Declaration = turbo_dom_layout::style::Declaration;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_048;
const MAX_SOURCE: usize = 64 * 1024;

struct SelectorPlan {
    source: String,
    seed: Option<String>,
    specificity: u32,
}
struct Plan {
    declarations: Vec<Declaration>,
    selectors: Vec<SelectorPlan>,
}
struct Entry {
    source: String,
    // Shadow selectors keep the existing full matcher and its borrowed AST.
    plan: Option<Plan>,
}
#[derive(Default)]
struct Store {
    entries: HashMap<u64, (usize, Rc<Entry>)>,
    order: VecDeque<u64>,
    bytes: usize,
}
impl Store {
    fn get(&self, key: u64, source: &str) -> Option<Rc<Entry>> {
        self.entries
            .get(&key)
            .filter(|(_, entry)| entry.source == source)
            .map(|(_, entry)| entry.clone())
    }
    fn remove(&mut self, key: u64) {
        if let Some((bytes, _)) = self.entries.remove(&key) {
            self.bytes -= bytes;
            self.order.retain(|&old| old != key);
        }
    }
    fn insert(&mut self, key: u64, entry: Rc<Entry>) -> bool {
        let Some(bytes) = retained_bytes(&entry) else {
            return false;
        };
        self.remove(key);
        while self.entries.len() >= MAX_ENTRIES || self.bytes.saturating_add(bytes) > MAX_BYTES {
            let Some(old) = self.order.pop_front() else {
                return false;
            };
            if let Some((bytes, _)) = self.entries.remove(&old) {
                self.bytes -= bytes;
            }
        }
        self.bytes += bytes;
        self.order.push_back(key);
        self.entries.insert(key, (bytes, entry));
        true
    }
}
thread_local! { static CACHE: RefCell<Store> = RefCell::new(Store::default()); }

pub(super) fn prepare(
    tree: &Tree,
    document: u32,
    root: Handle,
    graph: &Composition,
    source: &str,
) -> Result<Option<ScopedRule>, String> {
    // These are retention limits. Larger rules still use the ordinary parser.
    if source.len() > MAX_SOURCE {
        return Ok(None);
    }
    let depth = *graph
        .depths
        .get(&root)
        .ok_or("native CSS scope depth missing")?;
    let entry = cached(source)?;
    let Some(plan) = &entry.plan else {
        return Ok(None);
    };
    let matcher = Matcher { tree, root, graph };
    let mut specificity = HashMap::<Handle, u32>::new();
    for selector in &plan.selectors {
        for node in matcher.ordinary_nodes(&selector.source, selector.seed.as_deref())? {
            let current = specificity.entry(node).or_default();
            *current = (*current).max(selector.specificity);
        }
    }
    let mut targets = specificity
        .into_iter()
        .map(|(node, value)| Target {
            source: source_id(document, node),
            specificity: (value >> 20, (value >> 10) & 1023, value & 1023),
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| target.source.0);
    Ok(Some(ScopedRule {
        declarations: plan.declarations.clone(),
        depth,
        targets,
    }))
}
fn cached(source: &str) -> Result<Rc<Entry>, String> {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    let key = hash.finish();
    if let Some(entry) = CACHE.with(|cache| cache.borrow().get(key, source)) {
        return Ok(entry);
    }
    let entry = Rc::new(Entry {
        source: source.to_owned(),
        plan: compile(source)?,
    });
    CACHE.with(|cache| cache.borrow_mut().insert(key, entry.clone()));
    Ok(entry)
}
fn compile(source: &str) -> Result<Option<Plan>, String> {
    #[cfg(test)]
    RULE_COMPILATIONS.with(|count| count.set(count.get() + 1));
    let Ok(parsed) = StyleSheet::parse(source, ParserOptions::default()) else {
        return Ok(Some(Plan {
            declarations: Vec::new(),
            selectors: Vec::new(),
        }));
    };
    let [CssRule::Style(style)] = parsed.rules.0.as_slice() else {
        return Err("native scoped CSS requires one qualified rule".into());
    };
    if !style.rules.0.is_empty() {
        return Err("native scoped nested rules are unsupported".into());
    }
    let declarations = declarations(&style.declarations)?;
    let sources = selector_sources(source)?;
    if sources.len() != style.selectors.0.len() {
        return Err("native selector source provenance mismatch".into());
    }
    if parse_for_cascade(&sources.join(",")).is_err() {
        return Ok(Some(Plan {
            declarations,
            selectors: Vec::new(),
        }));
    }
    if style.selectors.0.iter().any(shadow_selector) {
        return Ok(None);
    }
    let selectors = style
        .selectors
        .0
        .iter()
        .zip(sources)
        .filter(|(selector, _)| ordinary_element_selector(selector))
        .map(|(selector, source)| SelectorPlan {
            source,
            seed: candidate_seed(selector),
            specificity: selector.specificity(),
        })
        .collect();
    Ok(Some(Plan {
        declarations,
        selectors,
    }))
}
fn charge(bytes: &mut usize, amount: usize) -> Option<()> {
    *bytes = bytes.checked_add(amount)?;
    (*bytes <= MAX_BYTES).then_some(())
}
fn retained_bytes(entry: &Entry) -> Option<usize> {
    if entry.source.len() > MAX_SOURCE {
        return None;
    }
    let mut bytes = std::mem::size_of::<Entry>() + 2 * std::mem::size_of::<usize>();
    charge(&mut bytes, entry.source.capacity())?;
    let Some(plan) = &entry.plan else {
        return Some(bytes);
    };
    charge(
        &mut bytes,
        plan.declarations
            .capacity()
            .checked_mul(std::mem::size_of::<Declaration>())?,
    )?;
    for declaration in &plan.declarations {
        charge(&mut bytes, declaration.property.capacity())?;
        charge(&mut bytes, declaration.value.capacity())?;
    }
    charge(
        &mut bytes,
        plan.selectors
            .capacity()
            .checked_mul(std::mem::size_of::<SelectorPlan>())?,
    )?;
    for selector in &plan.selectors {
        charge(&mut bytes, selector.source.capacity())?;
        if let Some(seed) = &selector.seed {
            charge(&mut bytes, seed.capacity())?;
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(source: &str) -> Rc<Entry> {
        Rc::new(Entry {
            source: source.into(),
            plan: Some(Plan {
                declarations: Vec::new(),
                selectors: Vec::new(),
            }),
        })
    }
    #[test]
    fn source_collisions_and_oversized_replacements_cannot_reuse_another_plan() {
        let mut store = Store::default();
        assert!(store.insert(7, entry("a")));
        assert!(store.get(7, "a").is_some());
        assert!(store.insert(7, entry("b")));
        assert!(store.get(7, "a").is_none());
        assert!(store.get(7, "b").is_some());
        assert_eq!(store.order.len(), 1);
        let oversized = Entry {
            source: "b".into(),
            plan: Some(Plan {
                declarations: vec![Declaration {
                    property: "width".into(),
                    value: String::with_capacity(MAX_BYTES),
                    important: false,
                }],
                selectors: Vec::new(),
            }),
        };
        assert!(!store.insert(7, Rc::new(oversized)));
        assert!(store.get(7, "b").is_some());
        assert!(!store.insert(7, entry(&"x".repeat(MAX_SOURCE + 1))));
        assert!(store.get(7, "b").is_some());
    }
    #[test]
    fn entry_count_and_retained_payload_are_bounded() {
        let mut store = Store::default();
        for key in 0..=MAX_ENTRIES as u64 {
            assert!(store.insert(key, entry(&key.to_string())));
        }
        assert_eq!(store.entries.len(), MAX_ENTRIES);
        assert!(store.get(0, "0").is_none());
        assert!(store.bytes <= MAX_BYTES);
        for key in 0..=MAX_ENTRIES as u64 {
            store.remove(key);
        }
        assert_eq!(store.bytes, 0);
        assert!(store.order.is_empty());
    }
    #[test]
    fn payload_pressure_evicts_the_oldest_plan_before_exceeding_retention_budget() {
        let mut store = Store::default();
        for key in 1..=3 {
            let plan = Entry {
                source: key.to_string(),
                plan: Some(Plan {
                    declarations: vec![Declaration {
                        property: "width".into(),
                        value: String::with_capacity(MAX_BYTES / 3),
                        important: false,
                    }],
                    selectors: Vec::new(),
                }),
            };
            assert!(store.insert(key, Rc::new(plan)));
            assert!(store.bytes <= MAX_BYTES);
        }
        assert_eq!(store.entries.len(), 2);
        assert!(store.get(1, "1").is_none());
        assert!(store.get(2, "2").is_some());
        assert!(store.get(3, "3").is_some());
    }
    #[test]
    fn large_rules_keep_uncached_support_and_current_targets() {
        let tree = Tree::parse("<body><input class=large>");
        let graph = Composition::scopes(&tree).unwrap();
        let rule = RuleSnapshot {
            kind: 1,
            css_text: format!(
                ".large{{--payload:{};width:7px}}",
                "x".repeat(MAX_SOURCE + 1)
            ),
            prelude: None,
            rules: None,
            media: String::new(),
            style_sheet: None,
            scoped: None,
        };
        RULE_COMPILATIONS.with(|count| count.set(0));
        for _ in 0..2 {
            let prepared = prepare_rule(&tree, 1, tree.root(), &graph, &rule).unwrap();
            assert_eq!(prepared.targets.len(), 1);
            assert_eq!(
                prepared
                    .declarations
                    .iter()
                    .find(|value| value.property == "width")
                    .unwrap()
                    .value,
                "7px"
            );
        }
        assert_eq!(RULE_COMPILATIONS.with(|count| count.get()), 2);
    }
}
