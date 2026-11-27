//! Raw CSS is independent of selector targets. Keep only bounded snapshots and weak owners.
use crate::live_stylesheets::{RuleSnapshot, SheetSnapshot};
use deno_core::v8;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_048;

#[derive(Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct SheetState {
    pub(crate) cacheable: bool,
    disabled: bool,
    media: String,
}

struct Entry {
    owner: v8::Weak<v8::Object>,
    epoch: u64,
    state: SheetState,
    snapshot: SheetSnapshot,
}

struct Store<T> {
    entries: HashMap<i32, (usize, T)>,
    order: VecDeque<i32>,
    bytes: usize,
}
impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
        }
    }
}
impl<T> Store<T> {
    fn remove(&mut self, key: i32) {
        if let Some((bytes, _)) = self.entries.remove(&key) {
            self.bytes -= bytes;
            self.order.retain(|&old| old != key);
        }
    }
    fn insert(&mut self, key: i32, bytes: usize, value: T) -> bool {
        if bytes > MAX_BYTES {
            return false;
        }
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
        self.entries.insert(key, (bytes, value));
        true
    }
}

#[derive(Clone, Default)]
struct Snapshots(Rc<RefCell<Store<Rc<Entry>>>>);

pub(crate) fn read(
    scope: &mut v8::PinScope,
    sheet: v8::Local<v8::Object>,
    epoch: u64,
    state: &SheetState,
) -> Option<SheetSnapshot> {
    let cache = scope.get_slot::<Snapshots>()?.clone();
    let key = sheet.get_identity_hash().get();
    let entry = cache
        .0
        .borrow()
        .entries
        .get(&key)
        .map(|(_, entry)| entry.clone())?;
    // Identity hashes may collide, and retired documents must remain collectible.
    if entry
        .owner
        .to_local(scope)
        .is_none_or(|owner| !owner.strict_equals(sheet.into()))
    {
        cache.0.borrow_mut().remove(key);
        return None;
    }
    (entry.epoch == epoch && entry.state == *state).then(|| entry.snapshot.clone())
}

pub(crate) fn remember(
    scope: &mut v8::PinScope,
    sheet: v8::Local<v8::Object>,
    epoch: u64,
    state: &SheetState,
    snapshot: &SheetSnapshot,
) {
    let Some(bytes) = snapshot_bytes(snapshot)
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Entry>()))
        .and_then(|bytes| bytes.checked_add(state.media.capacity()))
        .filter(|&bytes| bytes <= MAX_BYTES)
    else {
        return;
    };
    let key = sheet.get_identity_hash().get();
    let entry = Rc::new(Entry {
        owner: v8::Weak::new(scope, sheet),
        epoch,
        state: state.clone(),
        snapshot: snapshot.clone(),
    });
    if scope.get_slot::<Snapshots>().is_none() {
        scope.set_slot(Snapshots::default());
    }
    let cache = scope.get_slot::<Snapshots>().unwrap().clone();
    cache.0.borrow_mut().insert(key, bytes, entry);
}

fn charge(bytes: &mut usize, amount: usize) -> Option<()> {
    *bytes = bytes.checked_add(amount)?;
    (*bytes <= MAX_BYTES).then_some(())
}
fn snapshot_bytes(sheet: &SheetSnapshot) -> Option<usize> {
    let mut bytes = std::mem::size_of::<SheetSnapshot>();
    charge(&mut bytes, sheet.media.capacity())?;
    let mut pending = vec![&sheet.rules];
    while let Some(rules) = pending.pop() {
        charge(
            &mut bytes,
            rules
                .capacity()
                .checked_mul(std::mem::size_of::<RuleSnapshot>())?,
        )?;
        for rule in rules {
            // A prepared rule carries node identities and is never a reusable raw sheet.
            if rule.scoped.is_some() {
                return None;
            }
            charge(&mut bytes, rule.css_text.capacity())?;
            charge(&mut bytes, rule.media.capacity())?;
            if let Some(prelude) = &rule.prelude {
                charge(&mut bytes, prelude.capacity())?;
            }
            if let Some(children) = &rule.rules {
                pending.push(children);
            }
            if let Some(child) = &rule.style_sheet {
                charge(&mut bytes, std::mem::size_of::<SheetSnapshot>())?;
                charge(&mut bytes, child.media.capacity())?;
                pending.push(&child.rules);
            }
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_storage_evicts_old_values_and_preserves_replacement_accounting() {
        let mut store = Store::default();
        assert!(store.insert(1, MAX_BYTES / 2, "first"));
        assert!(store.insert(2, MAX_BYTES / 2, "second"));
        assert!(store.insert(3, MAX_BYTES / 2, "third"));
        assert!(!store.entries.contains_key(&1));
        assert_eq!(store.bytes, MAX_BYTES);
        assert!(store.insert(2, 1, "replacement"));
        assert_eq!(store.bytes, MAX_BYTES / 2 + 1);
        assert_eq!(store.order.len(), 2);
        assert!(!store.insert(2, MAX_BYTES + 1, "oversized"));
        assert_eq!(store.entries[&2].1, "replacement");
        store.remove(2);
        store.remove(2);
        assert_eq!(store.bytes, MAX_BYTES / 2);
    }

    #[test]
    fn snapshot_entry_count_is_bounded_even_for_empty_payloads() {
        let mut store = Store::default();
        for key in 0..=i32::try_from(MAX_ENTRIES).unwrap() {
            assert!(store.insert(key, 0, key));
        }
        assert_eq!(store.entries.len(), MAX_ENTRIES);
        assert_eq!(store.order.len(), MAX_ENTRIES);
        assert!(!store.entries.contains_key(&0));
    }

    #[test]
    fn raw_snapshot_accounting_counts_nested_data_and_rejects_prepared_targets() {
        let mut sheet: SheetSnapshot = serde_json::from_value(serde_json::json!({"media":"screen","rules":[{"type":4,"cssText":"@media all{}","prelude":"@media all","rules":[{"type":1,"cssText":".x{width:1px}"}],"styleSheet":{"rules":[{"type":1,"cssText":".y{height:2px}"}]}}]})).unwrap();
        let bytes = snapshot_bytes(&sheet).unwrap();
        assert!(
            bytes > std::mem::size_of::<SheetSnapshot>() + 3 * std::mem::size_of::<RuleSnapshot>()
        );
        sheet.rules[0].scoped = Some(crate::live_layout_scoped::ScopedRule {
            declarations: Vec::new(),
            depth: 0,
            targets: Vec::new(),
        });
        assert_eq!(snapshot_bytes(&sheet), None);
        sheet.rules[0].scoped = None;
        sheet.media = "x".repeat(MAX_BYTES);
        assert_eq!(snapshot_bytes(&sheet), None);
    }
}
