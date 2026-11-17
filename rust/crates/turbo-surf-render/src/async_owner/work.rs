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
    terminal: Cell<Option<TerminalPromiseState>>,
}

impl Node {
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

pub struct PromiseWork {
    key: v8::Global<v8::Private>,
    sequence: u32,
    nodes: HashMap<u32, Node>,
    ops: HashMap<(i32, u64), NativeOp>,
    inconsistent_owners: bool,
    tracking_error: Option<&'static str>,
    maintenance: u16,
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
            terminal: Cell::new(None),
        });
        node.observe_terminal(&promise.state());
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
        self.maintenance += 1;
        if self.maintenance == 1024 {
            self.maintenance = 0;
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
        let node = self.nodes.get(&node)?;
        if let Some(promise) = node.promise.to_local(scope) {
            let state = promise.state();
            node.observe_terminal(&state);
            return Some(state);
        }
        // Queued reactions may outlive their collected parent. Only an actual
        // native terminal observation survives; pending/unseen parents stay unknown.
        node.terminal.get().map(TerminalPromiseState::native)
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

    pub fn prune(&mut self, scope: &mut v8::PinScope) {
        let mut keep: HashSet<u32> = self
            .nodes
            .keys()
            .copied()
            .filter(|id| self.pending(scope, *id))
            .collect();
        keep.extend(self.ops.values().map(|op| op.node));
        let mut frontier: Vec<_> = keep.iter().copied().collect();
        while let Some(id) = frontier.pop() {
            if let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) {
                if keep.insert(parent) {
                    frontier.push(parent);
                }
            }
        }
        self.nodes.retain(|id, _| keep.contains(id));
    }
}
