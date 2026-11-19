//! Native session-history identities and fail-atomic retention accounting.
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(super) struct Entry<S> {
    pub row: u64,
    pub group: Option<u64>,
    pub local: u32,
    pub url: String,
    pub key: String,
    pub navigation_id: String,
    pub scroll: String,
    pub state: Option<S>,
    pub bytes: usize,
}

pub(super) struct Draft<S> {
    pub group: Option<u64>,
    pub local: u32,
    pub url: String,
    pub key: String,
    pub navigation_id: String,
    pub scroll: String,
    pub state: Option<S>,
    pub bytes: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub entries: usize,
    pub url_bytes: usize,
    pub state_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Change {
    Push,
    Replace,
    Bind,
    Update,
}

pub(super) struct Plan<S> {
    revision: u64,
    target: u64,
    next_revision: u64,
    next_row: u64,
    at: usize,
    truncate: bool,
    entry: Entry<S>,
}

pub(super) struct History<S> {
    entries: Vec<Entry<S>>,
    index: usize,
    revision: u64,
    target: u64,
    next_row: u64,
    serials: BTreeMap<u64, u32>,
}

#[cfg(test)]
impl<S> Default for History<S> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: 0,
            revision: 0,
            target: 0,
            next_row: 1,
            serials: BTreeMap::new(),
        }
    }
}

impl<S> History<S> {
    pub fn new(target: u64) -> Self {
        Self {
            entries: Vec::new(),
            index: 0,
            revision: 0,
            target,
            next_row: 1,
            serials: BTreeMap::new(),
        }
    }
    pub fn entries(&self) -> &[Entry<S>] {
        &self.entries
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn used_bytes(&self) -> usize {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }

    pub fn serial(&self, group: u64) -> u32 {
        self.serials.get(&group).copied().unwrap_or(0)
    }

    pub fn current(&self) -> Option<&Entry<S>> {
        self.entries.get(self.index)
    }

    pub fn row_index(&self, row: u64) -> Option<usize> {
        self.entries.iter().position(|entry| entry.row == row)
    }

    pub fn adjacent(&self, delta: i32) -> Option<&Entry<S>> {
        let at = self.index.checked_add_signed(delta as isize)?;
        self.entries.get(at)
    }

    pub fn retained_bytes(&self, change: Change) -> usize {
        self.entries
            .iter()
            .enumerate()
            .filter(|(at, _)| match change {
                Change::Push => *at <= self.index,
                Change::Replace | Change::Bind | Change::Update => *at != self.index,
            })
            .map(|(_, entry)| entry.bytes)
            .sum()
    }

    pub fn prepare(
        &self,
        draft: Draft<S>,
        change: Change,
        limits: Limits,
    ) -> Result<Plan<S>, String> {
        self.validate_entry(&draft, change)?;
        self.plan(draft, change, limits)
    }

    fn validate_entry(&self, draft: &Draft<S>, change: Change) -> Result<(), String> {
        if draft.group.is_none() || draft.local == 0 {
            return Err("Native history entry identity is unavailable".into());
        }
        self.validate_identity(draft, change)?;
        if draft.key.len() > 128
            || draft.navigation_id.len() > 128
            || !matches!(draft.scroll.as_str(), "auto" | "manual")
        {
            return Err("Native history metadata budget exceeded".into());
        }
        Ok(())
    }

    fn plan(&self, draft: Draft<S>, change: Change, limits: Limits) -> Result<Plan<S>, String> {
        let at = if self.entries.is_empty() {
            0
        } else if matches!(change, Change::Push) {
            self.index + 1
        } else {
            self.index
        };
        let count = if matches!(change, Change::Push) || self.entries.is_empty() {
            at + 1
        } else {
            self.entries.len()
        };
        if count > limits.entries || draft.url.len() > limits.url_bytes {
            return Err("Native history entry or URL budget exceeded".into());
        }
        if self
            .retained_bytes(change)
            .checked_add(draft.bytes)
            .is_none_or(|bytes| bytes > limits.state_bytes)
        {
            return Err("Native history state byte budget exceeded".into());
        }
        let row = self
            .entries
            .get(at)
            .filter(|_| !matches!(change, Change::Push))
            .map_or(self.next_row, |entry| entry.row);
        let next_row = if row == self.next_row {
            self.next_row
                .checked_add(1)
                .ok_or("Native history row identity exhausted")?
        } else {
            self.next_row
        };
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or("Native history revision exhausted")?;
        Ok(Plan {
            revision: self.revision,
            target: self.target,
            next_revision,
            next_row,
            at,
            truncate: matches!(change, Change::Push),
            entry: Entry {
                row,
                group: draft.group,
                local: draft.local,
                url: draft.url,
                key: draft.key,
                navigation_id: draft.navigation_id,
                scroll: draft.scroll,
                state: draft.state,
                bytes: draft.bytes,
            },
        })
    }

    pub fn placeholder(
        &mut self,
        url: String,
        group: Option<u64>,
        change: Change,
        limits: Limits,
    ) -> Result<(), String> {
        let plan = self.plan(
            Draft {
                group,
                local: 0,
                url,
                key: String::new(),
                navigation_id: String::new(),
                scroll: "auto".into(),
                state: None,
                bytes: 0,
            },
            change,
            limits,
        )?;
        self.commit(plan)
    }

    fn validate_identity(&self, draft: &Draft<S>, change: Change) -> Result<(), String> {
        let Some(group) = draft.group else {
            return Ok(());
        };
        if draft.local == 0 {
            return Ok(());
        }
        if matches!(change, Change::Update) {
            if self
                .current()
                .is_some_and(|entry| entry.group == Some(group) && entry.local == draft.local)
            {
                return Ok(());
            }
            return Err("Native history state update does not own its current entry".into());
        }
        let bound = matches!(change, Change::Bind)
            && self
                .current()
                .is_some_and(|entry| entry.group == Some(group) && entry.local == 0);
        if !bound && draft.local <= self.serial(group) {
            return Err("Native history local identity was reused".into());
        }
        Ok(())
    }

    pub fn commit(&mut self, plan: Plan<S>) -> Result<(), String> {
        if self.target != plan.target || self.revision != plan.revision {
            return Err("Native history changed during state preparation".into());
        }
        if plan.truncate {
            self.entries.truncate(plan.at);
        }
        if let Some(group) = plan.entry.group {
            self.serials
                .entry(group)
                .and_modify(|serial| *serial = (*serial).max(plan.entry.local))
                .or_insert(plan.entry.local);
        }
        if plan.at == self.entries.len() {
            self.entries.push(plan.entry);
        } else {
            self.entries[plan.at] = plan.entry;
        }
        self.index = plan.at;
        self.revision = plan.next_revision;
        self.next_row = plan.next_row;
        self.serials
            .retain(|group, _| self.entries.iter().any(|entry| entry.group == Some(*group)));
        Ok(())
    }

    pub fn set_scroll(&mut self, group: u64, local: u32, value: &str) -> Result<(), String> {
        if !matches!(value, "auto" | "manual") {
            return Err("Native history scroll restoration mode is invalid".into());
        }
        let current = self
            .current()
            .ok_or("Native history has no current entry")?;
        if local == 0 || current.group != Some(group) || current.local != local {
            return Err("Native history scroll restoration has no owned current entry".into());
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or("Native history revision exhausted")?;
        let scroll = value.to_owned();
        self.entries[self.index].scroll = scroll;
        self.revision = revision;
        Ok(())
    }

    pub fn select(&mut self, row: u64) -> Result<(), String> {
        let at = self
            .row_index(row)
            .ok_or("Native history traversal target no longer exists")?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or("Native history revision exhausted")?;
        self.index = at;
        self.revision = revision;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> Limits {
        Limits {
            entries: 8,
            url_bytes: 128,
            state_bytes: 20,
        }
    }
    fn entry(group: u64, local: u32, url: &str, bytes: usize) -> Draft<Vec<u8>> {
        Draft {
            group: Some(group),
            local,
            url: url.into(),
            key: format!("{group}:{local}"),
            navigation_id: format!("id:{local}"),
            scroll: "auto".into(),
            state: Some(vec![0; bytes]),
            bytes,
        }
    }
    fn push(history: &mut History<Vec<u8>>, group: u64, local: u32, url: &str, bytes: usize) {
        let plan = history
            .prepare(entry(group, local, url, bytes), Change::Push, limits())
            .unwrap();
        history.commit(plan).unwrap();
    }
    #[test]
    fn repeated_urls_have_distinct_rows_and_groups() {
        let mut history = History::default();
        push(&mut history, 10, 1, "/same", 2);
        push(&mut history, 10, 2, "/same", 3);
        push(&mut history, 20, 1, "/same", 4);
        assert_ne!(history.entries[0].row, history.entries[1].row);
        assert_eq!(history.entries[0].group, history.entries[1].group);
        assert_ne!(history.entries[1].group, history.entries[2].group);
        assert_eq!(history.used_bytes(), 9);
    }
    #[test]
    fn replacement_preserves_forward_and_push_truncates_it_atomically() {
        let mut history = History::default();
        push(&mut history, 10, 1, "/one", 3);
        push(&mut history, 10, 2, "/two", 7);
        let forward = history.current().unwrap().row;
        let first = history.entries[0].row;
        history.select(first).unwrap();
        let plan = history
            .prepare(entry(10, 3, "/replacement", 4), Change::Replace, limits())
            .unwrap();
        history.commit(plan).unwrap();
        assert_eq!(history.entries[1].row, forward);
        assert_eq!(history.used_bytes(), 11);
        push(&mut history, 10, 4, "/branch", 5);
        assert!(history.row_index(forward).is_none());
        assert_eq!(history.used_bytes(), 9);
        assert_eq!(history.serial(10), 4);
    }
    #[test]
    fn failed_budget_and_reentrant_plan_leave_successful_state_untouched() {
        let mut history = History::default();
        push(&mut history, 10, 1, "/one", 2);
        let outer = history
            .prepare(entry(10, 3, "/outer", 2), Change::Push, limits())
            .unwrap();
        push(&mut history, 10, 2, "/inner", 3);
        assert!(history.commit(outer).is_err());
        assert_eq!(history.current().unwrap().url, "/inner");
        let revision = history.revision;
        assert!(history
            .prepare(entry(10, 4, "/too-large", 30), Change::Replace, limits())
            .is_err());
        assert_eq!(history.revision, revision);
        assert_eq!(history.index, 1);
        assert_eq!(history.used_bytes(), 5);
    }
    #[test]
    fn scroll_metadata_updates_without_cloning_state_or_changing_entry_identity() {
        struct Unclonable;
        let mut history = History::new(41);
        let plan = history
            .prepare(
                Draft {
                    group: Some(10),
                    local: 1,
                    url: "/one".into(),
                    key: "key".into(),
                    navigation_id: "id".into(),
                    scroll: "auto".into(),
                    state: Some(Unclonable),
                    bytes: 9,
                },
                Change::Push,
                limits(),
            )
            .unwrap();
        history.commit(plan).unwrap();
        let row = history.current().unwrap().row;
        let revision = history.revision();
        history.set_scroll(10, 1, "manual").unwrap();
        assert_eq!(history.current().unwrap().scroll, "manual");
        assert_eq!(history.current().unwrap().row, row);
        assert!(history.current().unwrap().state.is_some());
        assert_eq!(history.used_bytes(), 9);
        assert_eq!(history.index(), 0);
        assert_eq!(history.serial(10), 1);
        assert_eq!(history.revision(), revision + 1);
        for (group, local, value) in [(11, 1, "auto"), (10, 2, "auto"), (10, 1, "invalid")] {
            assert!(history.set_scroll(group, local, value).is_err());
        }
        assert_eq!(history.current().unwrap().scroll, "manual");
        assert_eq!(history.revision(), revision + 1);
    }
}
