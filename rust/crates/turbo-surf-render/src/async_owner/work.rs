//! Native Promise work selected through authenticated weak document leases.
use deno_core::{v8, AsyncOpEvent, AsyncOpObservation};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OwnerId(u64);

impl OwnerId {
    pub(super) fn registered(native_id: u64) -> Self {
        Self(native_id)
    }
}

#[derive(Clone, Copy)]
pub enum PromiseTurn {
    Init,
    Before,
    After,
    Resolve,
}

#[derive(Clone, Copy)]
enum TerminalPromiseState {
    Fulfilled,
    Rejected,
}

impl TerminalPromiseState {
    fn observed(state: &v8::PromiseState) -> Option<Self> {
        match state {
            v8::PromiseState::Pending => None,
            v8::PromiseState::Fulfilled => Some(Self::Fulfilled),
            v8::PromiseState::Rejected => Some(Self::Rejected),
        }
    }

    fn native(self) -> v8::PromiseState {
        match self {
            Self::Fulfilled => v8::PromiseState::Fulfilled,
            Self::Rejected => v8::PromiseState::Rejected,
        }
    }
}

struct Node {
    promise: v8::Weak<v8::Promise>,
    owner: Option<OwnerId>,
    parent: Option<u32>,
    reacted: bool,
    retained_for_prune: bool,
    terminal: Cell<Option<TerminalPromiseState>>,
    #[cfg(test)]
    native_state_observations: Cell<usize>,
}

impl Node {
    fn state(&self, scope: &mut v8::PinScope) -> Option<v8::PromiseState> {
        if let Some(terminal) = self.terminal.get() {
            return Some(terminal.native());
        }
        if let Some(promise) = self.promise.to_local(scope) {
            return Some(self.observe_native_state(promise));
        }
        // Queued reactions may outlive their collected parent. Only an actual
        // native terminal observation survives; pending/unseen parents stay unknown.
        self.terminal.get().map(TerminalPromiseState::native)
    }

    fn observe_native_state(&self, promise: v8::Local<v8::Promise>) -> v8::PromiseState {
        if let Some(terminal) = self.terminal.get() {
            return terminal.native();
        }
        #[cfg(test)]
        self.native_state_observations
            .set(self.native_state_observations.get() + 1);
        let state = promise.state();
        self.observe_terminal(&state);
        state
    }

    fn observe_terminal(&self, state: &v8::PromiseState) {
        if let Some(terminal) = TerminalPromiseState::observed(state) {
            self.terminal.set(Some(terminal));
        }
    }
}

struct NativeOp {
    node: u32,
    owner: Option<OwnerId>,
    refed: bool,
}

#[derive(Default)]
pub struct WorkSnapshot {
    pub owners: HashSet<OwnerId>,
    pub selected_ops: usize,
    pub unknown_ops: usize,
    pub runnable_reactions: usize,
    pub inconsistent_owners: bool,
    pub unknown_reaction_parents: usize,
    pub collected_root_reaction_parents: usize,
    pub tracking_error: Option<&'static str>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
struct PruneMeasurements {
    calls: usize,
    candidates: usize,
    ancestors: usize,
    retained_candidates: usize,
    peak_candidates: usize,
    live_after_prune: usize,
    peak_nodes: usize,
    bulk_membership_lookups: usize,
}

pub struct PromiseWork {
    key: v8::Global<v8::Private>,
    sequence: u32,
    nodes: HashMap<u32, Node>,
    ops: HashMap<(i32, u64), NativeOp>,
    inconsistent_owners: bool,
    tracking_error: Option<&'static str>,
    maintenance: usize,
    next_maintenance: usize,
    #[cfg(test)]
    prune_measurements: PruneMeasurements,
}

impl PromiseWork {
    pub fn new(scope: &mut v8::PinScope) -> Self {
        let key = v8::Private::new(scope, None);
        Self {
            key: v8::Global::new(scope, key),
            sequence: 0,
            nodes: HashMap::new(),
            ops: HashMap::new(),
            inconsistent_owners: false,
            tracking_error: None,
            maintenance: 0,
            next_maintenance: 1024,
            #[cfg(test)]
            prune_measurements: PruneMeasurements::default(),
        }
    }

    fn node<'s>(
        &mut self,
        scope: &mut v8::PinScope<'s, '_>,
        promise: v8::Local<'s, v8::Promise>,
    ) -> Option<u32> {
        if self.tracking_error.is_some() {
            return None;
        }
        let key = v8::Local::new(scope, &self.key);
        let existing = promise
            .get_private(scope, key)
            .filter(|value| value.is_uint32())
            .and_then(|value| value.uint32_value(scope));
        let id = match existing {
            Some(id) => id,
            None => {
                let Some(next) = self.sequence.checked_add(1) else {
                    self.tracking_error = Some("Native Promise identity capacity exceeded");
                    return None;
                };
                self.sequence = next;
                let value = v8::Integer::new_from_unsigned(scope, self.sequence);
                if promise.set_private(scope, key, value.into()) != Some(true) {
                    self.tracking_error = Some("Native Promise identity installation failed");
                    return None;
                }
                self.sequence
            }
        };
        let node = self.nodes.entry(id).or_insert_with(|| Node {
            promise: v8::Weak::new(scope, promise),
            owner: None,
            parent: None,
            reacted: false,
            retained_for_prune: false,
            terminal: Cell::new(None),
            #[cfg(test)]
            native_state_observations: Cell::new(0),
        });
        node.observe_native_state(promise);
        #[cfg(test)]
        {
            self.prune_measurements.peak_nodes =
                self.prune_measurements.peak_nodes.max(self.nodes.len());
        }
        Some(id)
    }

    pub fn promise_turn<'s>(
        &mut self,
        scope: &mut v8::PinScope<'s, '_>,
        turn: PromiseTurn,
        promise: v8::Local<'s, v8::Promise>,
        parent: Option<v8::Local<'s, v8::Promise>>,
        owner: Option<OwnerId>,
    ) {
        let parent = parent.and_then(|parent| self.node(scope, parent));
        let Some(id) = self.node(scope, promise) else {
            return;
        };
        let Some(node) = self.nodes.get_mut(&id) else {
            self.tracking_error = Some("Native Promise identity unavailable");
            return;
        };
        if node.owner.is_none() {
            node.owner = owner;
        }
        match turn {
            PromiseTurn::Init => node.parent = parent,
            PromiseTurn::After => node.reacted = true,
            // Resolve hooks precede final native Promise state changes; never
            // use the hook alone to declare a Promise settled.
            PromiseTurn::Before | PromiseTurn::Resolve => {}
        }
        self.maintenance = self.maintenance.saturating_add(1);
        if self.maintenance >= self.next_maintenance {
            self.prune(scope);
        }
    }

    pub fn op_turn<'s>(
        &mut self,
        scope: &mut v8::PinScope<'s, '_>,
        event: AsyncOpObservation<'s>,
        owner: Option<OwnerId>,
    ) {
        let key = (event.promise_id, event.generation);
        match event.event {
            AsyncOpEvent::Registered => {
                let Some(promise) = event.promise else {
                    self.tracking_error = Some("Native async operation has no genuine Promise");
                    return;
                };
                let Some(node) = self.node(scope, promise) else {
                    return;
                };
                let Some(metadata) = self.nodes.get_mut(&node) else {
                    self.tracking_error = Some("Native async operation identity unavailable");
                    return;
                };
                if matches!((metadata.owner, owner), (Some(before), Some(now)) if before != now) {
                    self.inconsistent_owners = true;
                }
                if metadata.owner.is_none() {
                    metadata.owner = owner;
                }
                let owner = owner.or(metadata.owner);
                self.ops.insert(
                    key,
                    NativeOp {
                        node,
                        owner,
                        refed: true,
                    },
                );
            }
            AsyncOpEvent::Settled => {
                self.ops.remove(&key);
            }
            AsyncOpEvent::TrackingFailed => {
                self.tracking_error = Some("Core native async operation tracking failed");
            }
            AsyncOpEvent::Refed | AsyncOpEvent::Unrefed => {
                if let Some(op) = self.ops.get_mut(&key) {
                    op.refed = event.event == AsyncOpEvent::Refed;
                }
            }
        }
    }

    fn state(&self, scope: &mut v8::PinScope, node: u32) -> Option<v8::PromiseState> {
        self.nodes.get(&node)?.state(scope)
    }

    fn pending(&self, scope: &mut v8::PinScope, node: u32) -> bool {
        self.state(scope, node) == Some(v8::PromiseState::Pending)
    }

    fn settled(&self, scope: &mut v8::PinScope, node: u32) -> bool {
        matches!(
            self.state(scope, node),
            Some(v8::PromiseState::Fulfilled | v8::PromiseState::Rejected)
        )
    }

    fn dependency_owner(
        &self,
        scope: &mut v8::PinScope,
        owners: &HashSet<OwnerId>,
        id: u32,
        node: &Node,
    ) -> Option<OwnerId> {
        if node.reacted
            || !node.owner.is_some_and(|owner| owners.contains(&owner))
            || !self.pending(scope, id)
        {
            return None;
        }
        let parent = node.parent?;
        self.pending(scope, parent)
            .then(|| self.nodes.get(&parent)?.owner)
            .flatten()
    }

    fn dependency_scope(
        &self,
        scope: &mut v8::PinScope,
        roots: &HashSet<OwnerId>,
    ) -> HashSet<OwnerId> {
        let mut owners = roots.clone();
        loop {
            let added: Vec<_> = self
                .nodes
                .iter()
                .filter_map(|(id, node)| self.dependency_owner(scope, &owners, *id, node))
                .collect();
            let before = owners.len();
            owners.extend(added);
            if owners.len() == before {
                return owners;
            }
        }
    }

    pub fn snapshot(&self, scope: &mut v8::PinScope, roots: &HashSet<OwnerId>) -> WorkSnapshot {
        let owners = self.dependency_scope(scope, roots);
        let mut result = WorkSnapshot {
            owners,
            inconsistent_owners: self.inconsistent_owners,
            tracking_error: self.tracking_error,
            ..Default::default()
        };
        for op in self.ops.values().filter(|op| op.refed) {
            match op.owner {
                Some(owner) if result.owners.contains(&owner) => result.selected_ops += 1,
                None => result.unknown_ops += 1,
                _ => {}
            }
        }
        result.runnable_reactions = self
            .nodes
            .iter()
            .filter(|(id, node)| {
                !node.reacted
                    && node
                        .owner
                        .is_some_and(|owner| result.owners.contains(&owner))
                    && self.pending(scope, **id)
                    && node
                        .parent
                        .is_some_and(|parent| self.settled(scope, parent))
            })
            .count();
        result.unknown_reaction_parents = self
            .nodes
            .iter()
            .filter(|(id, node)| {
                !node.reacted
                    && node
                        .owner
                        .is_some_and(|owner| result.owners.contains(&owner))
                    && self.pending(scope, **id)
                    && node
                        .parent
                        .is_some_and(|parent| self.state(scope, parent).is_none())
            })
            .count();
        result.collected_root_reaction_parents = self
            .nodes
            .iter()
            .filter(|(id, node)| self.collected_root_parent(scope, roots, **id, node))
            .count();
        result
    }

    fn collected_root_parent(
        &self,
        scope: &mut v8::PinScope,
        roots: &HashSet<OwnerId>,
        id: u32,
        node: &Node,
    ) -> bool {
        if node.reacted
            || !node.owner.is_some_and(|owner| roots.contains(&owner))
            || !self.pending(scope, id)
        {
            return false;
        }
        let Some(parent) = node.parent.and_then(|parent| self.nodes.get(&parent)) else {
            return false;
        };
        parent.owner == node.owner
            && parent.promise.to_local(scope).is_none()
            && parent.terminal.get().is_none()
    }

    fn mark_for_prune(nodes: &mut HashMap<u32, Node>, id: u32, frontier: &mut Vec<u32>) {
        let Some(node) = nodes.get_mut(&id) else {
            return;
        };
        if !node.retained_for_prune {
            node.retained_for_prune = true;
            frontier.push(id);
        }
    }

    pub fn prune(&mut self, scope: &mut v8::PinScope) {
        self.maintenance = 0;
        let mut frontier = Vec::new();
        #[cfg(test)]
        let mut candidates = 0;
        for (id, node) in &mut self.nodes {
            #[cfg(test)]
            {
                candidates += 1;
            }
            // This mark belongs only to this complete maintenance pass. No JS
            // runs between clearing it and retaining the dependency closure.
            node.retained_for_prune = node.state(scope) == Some(v8::PromiseState::Pending);
            if node.retained_for_prune {
                frontier.push(*id);
            }
        }
        for op in self.ops.values() {
            Self::mark_for_prune(&mut self.nodes, op.node, &mut frontier);
        }
        #[cfg(test)]
        let mut ancestors = 0;
        while let Some(id) = frontier.pop() {
            #[cfg(test)]
            {
                ancestors += 1;
            }
            if let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) {
                Self::mark_for_prune(&mut self.nodes, parent, &mut frontier);
            }
        }
        #[cfg(test)]
        let retained_candidates = Cell::new(0);
        self.nodes.retain(|_, node| {
            #[cfg(test)]
            retained_candidates.set(retained_candidates.get() + 1);
            node.retained_for_prune
        });
        // Charge graph-wide maintenance in proportion to the live graph, while
        // keeping a small graph responsive to collected/settled nodes.
        self.next_maintenance = self.nodes.len().max(1024);
        #[cfg(test)]
        {
            self.prune_measurements.calls += 1;
            self.prune_measurements.peak_candidates =
                self.prune_measurements.peak_candidates.max(candidates);
            self.prune_measurements.live_after_prune = self.nodes.len();
            self.prune_measurements.candidates += candidates;
            self.prune_measurements.ancestors += ancestors;
            self.prune_measurements.retained_candidates += retained_candidates.get();
        }
    }
}

#[cfg(all(test, not(turbo_surf_native_proxy)))]
#[path = "work_terminal_cache_tests.rs"]
mod terminal_cache_tests;

#[cfg(all(test, not(turbo_surf_native_proxy)))]
#[path = "work_prune_scaling_tests.rs"]
mod prune_scaling_tests;
