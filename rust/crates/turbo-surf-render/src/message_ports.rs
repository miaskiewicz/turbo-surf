//! Isolate-owned port endpoints and one-shot ownership transfers.
use deno_core::{op2, v8, Extension, JsRuntime};
use deno_error::JsErrorBox;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::{Rc, Weak},
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PortToken {
    id: u32,
    generation: u32,
}
#[derive(Clone)]
pub(crate) struct PortTransfer {
    token: PortToken,
    endpoint: Weak<RefCell<Endpoint>>,
    registry: Weak<RefCell<Registry>>,
    anchor_pin: Option<Rc<RefCell<Option<v8::Global<v8::Object>>>>>,
}
type EndpointHandle = Rc<RefCell<Endpoint>>;
struct Endpoint {
    id: u32,
    anchor: v8::Weak<v8::Object>,
    task_pin: Option<v8::Global<v8::Object>>,
    generation: u32,
    peer: Option<u32>,
    owned: bool,
    started: bool,
    closed: bool,
    scheduled: bool,
    queue: VecDeque<crate::crypto_clone::CloneSnapshot>,
    reservations: usize,
}
struct Registry {
    next: u32,
    queued_bytes: usize,
    reserved_bytes: usize,
    max_messages: usize,
    max_bytes: usize,
    endpoints: HashMap<u32, EndpointHandle>,
    dirty: HashSet<u32>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            next: 0,
            queued_bytes: 0,
            reserved_bytes: 0,
            max_messages: 65536,
            max_bytes: 128 * 1024 * 1024,
            endpoints: HashMap::new(),
            dirty: HashSet::new(),
        }
    }
}
struct Reservation {
    registry: RegistryHandle,
    endpoint: EndpointHandle,
    bytes: usize,
    _anchor_pin: v8::Global<v8::Object>,
}
impl Reservation {
    fn new(
        scope: &mut v8::PinScope,
        registry: RegistryHandle,
        endpoint: EndpointHandle,
    ) -> Result<Self, JsErrorBox> {
        let anchor = endpoint
            .borrow()
            .anchor
            .to_local(scope)
            .ok_or_else(|| JsErrorBox::generic("Port receiver was collected"))?;
        let anchor_pin = v8::Global::new(scope, anchor);
        let maximum = registry.borrow().max_messages;
        {
            let mut endpoint = endpoint.borrow_mut();
            if endpoint.queue.len().saturating_add(endpoint.reservations) >= maximum {
                return Err(JsErrorBox::range_error("Native port queue budget exceeded"));
            }
            endpoint.reservations += 1;
        }
        Ok(Self {
            registry,
            endpoint,
            bytes: 0,
            _anchor_pin: anchor_pin,
        })
    }
    fn admit(&mut self, bytes: usize) -> Result<(), JsErrorBox> {
        let mut registry = self.registry.borrow_mut();
        if registry
            .queued_bytes
            .saturating_add(registry.reserved_bytes)
            .saturating_add(bytes)
            > registry.max_bytes
        {
            return Err(JsErrorBox::range_error(
                "Native message queue memory budget exceeded",
            ));
        }
        registry.reserved_bytes += bytes;
        self.bytes = bytes;
        Ok(())
    }
    fn enqueue(mut self, scope: &mut v8::PinScope, snapshot: crate::crypto_clone::CloneSnapshot) {
        {
            let mut registry = self.registry.borrow_mut();
            registry.reserved_bytes -= self.bytes;
            registry.queued_bytes += self.bytes;
        }
        self.bytes = 0;
        self.endpoint.borrow_mut().queue.push_back(snapshot);
        let id = self.endpoint.borrow().id;
        self.registry.borrow_mut().dirty.insert(id);
        refresh_dirty(scope, &self.registry);
        // Private receiver graph now traces these ports; a native registry queue
        // must not become an independent root of an unreachable transfer cycle.
        self.endpoint
            .borrow()
            .queue
            .back()
            .unwrap()
            .release_port_pins();
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.endpoint.borrow_mut().reservations -= 1;
        self.registry.borrow_mut().reserved_bytes -= self.bytes;
    }
}
type RegistryHandle = Rc<RefCell<Registry>>;
fn graph_key<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    field: &str,
) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, &format!("turbo-surf.MessagePort.graph.{field}"))?;
    Some(v8::Private::for_api(scope, Some(name)))
}
fn graph_set(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    field: &str,
    value: v8::Local<v8::Value>,
) {
    if let Some(key) = graph_key(scope, field) {
        object.set_private(scope, key, value);
    }
}
fn current_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    endpoint: &Endpoint,
) -> Option<v8::Local<'s, v8::Object>> {
    if !endpoint.owned {
        return None;
    }
    let anchor = endpoint.anchor.to_local(scope)?;
    let key = graph_key(scope, "owner")?;
    anchor
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
}
fn dirty(registry: &RegistryHandle, endpoint: &EndpointHandle) {
    let state = endpoint.borrow();
    let mut registry = registry.borrow_mut();
    registry.dirty.insert(state.id);
    if let Some(peer) = state.peer {
        registry.dirty.insert(peer);
    }
}
fn refresh_dirty(scope: &mut v8::PinScope, registry: &RegistryHandle) {
    let ids = std::mem::take(&mut registry.borrow_mut().dirty);
    for id in ids {
        if scope.is_execution_terminating() {
            registry.borrow_mut().dirty.insert(id);
            continue;
        }
        let Some(endpoint) = registry.borrow().endpoints.get(&id).cloned() else {
            continue;
        };
        let Some(anchor) = endpoint.borrow().anchor.to_local(scope) else {
            continue;
        };
        let peer = endpoint
            .borrow()
            .peer
            .and_then(|id| registry.borrow().endpoints.get(&id).cloned());
        let peer_anchor = peer.and_then(|peer| {
            let peer = peer.borrow();
            let owner = current_owner(scope, &peer)?;
            (crate::browser_env::native_event_target_has_listener(scope, owner, "message")
                || crate::browser_env::native_event_target_has_listener(
                    scope,
                    owner,
                    "messageerror",
                ))
            .then(|| peer.anchor.to_local(scope))
            .flatten()
        });
        let null = v8::null(scope);
        graph_set(
            scope,
            anchor,
            "peer",
            peer_anchor.map_or(null.into(), |anchor| anchor.into()),
        );
        let mut edges = Vec::new();
        for snapshot in &endpoint.borrow().queue {
            edges.extend(
                snapshot
                    .port_anchors(scope)
                    .into_iter()
                    .map(v8::Local::<v8::Value>::from),
            );
        }
        let edges = v8::Array::new_with_elements(scope, &edges);
        graph_set(scope, anchor, "queue", edges.into());
    }
}
fn listener_changed(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    let Some(token) = token(scope, object) else {
        return;
    };
    let registry = registry(scope);
    let endpoint = registry.borrow().endpoints.get(&token.id).cloned();
    if let Some(endpoint) = endpoint {
        if endpoint.borrow().generation == token.generation {
            dirty(&registry, &endpoint);
            refresh_dirty(scope, &registry);
        }
    }
}
pub(crate) fn sweep(scope: &mut v8::PinScope) {
    let registry = registry(scope);
    refresh_dirty(scope, &registry);
    let removed = {
        let mut registry = registry.borrow_mut();
        let ids = registry
            .endpoints
            .iter()
            .filter_map(|(id, endpoint)| {
                let endpoint = endpoint.borrow();
                (endpoint.anchor.is_empty() && endpoint.reservations == 0).then_some(*id)
            })
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| {
                registry
                    .endpoints
                    .remove(&id)
                    .map(|endpoint| (id, endpoint))
            })
            .collect::<Vec<_>>()
    };
    let mut queues = VecDeque::new();
    for (id, endpoint) in removed {
        queues.extend(disentangle(&registry, id, &endpoint));
    }
    discard_queue(queues);
    refresh_dirty(scope, &registry);
}

struct Adapter {
    create: v8::Weak<v8::Function>,
    dispatch: v8::Weak<v8::Function>,
    schedule: v8::Weak<v8::Function>,
}
fn registry(scope: &mut v8::PinScope) -> RegistryHandle {
    JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<RegistryHandle>()
        .clone()
}
fn key<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "turbo-surf.MessagePort.endpoint")?;
    Some(v8::Private::for_api(scope, Some(name)))
}
pub(crate) fn token(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Option<PortToken> {
    let key = key(scope)?;
    let values = object
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())?;
    Some(PortToken {
        id: values.get_index(scope, 0)?.uint32_value(scope)?,
        generation: values.get_index(scope, 1)?.uint32_value(scope)?,
    })
}
fn set_token(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    token: PortToken,
) -> Option<()> {
    let id = v8::Integer::new_from_unsigned(scope, token.id);
    let generation = v8::Integer::new_from_unsigned(scope, token.generation);
    let values = v8::Array::new_with_elements(scope, &[id.into(), generation.into()]);
    let key = key(scope)?;
    (object.set_private(scope, key, values.into()) == Some(true)).then_some(())
}
fn lookup_endpoint(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
) -> Result<(PortToken, Option<EndpointHandle>), JsErrorBox> {
    sweep(scope);
    let token = token(scope, object).ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let endpoint = registry(scope).borrow().endpoints.get(&token.id).cloned();
    Ok((token, endpoint))
}
fn transfer_contains_endpoint(
    scope: &mut v8::PinScope,
    transfer: Option<v8::Local<v8::Value>>,
    id: u32,
) -> bool {
    let Some(array) = transfer.and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
    else {
        return false;
    };
    for index in 0..array.length().min(4096) {
        let object = array
            .get_index(scope, index)
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok());
        if object
            .and_then(|object| token(scope, object))
            .is_some_and(|token| token.id == id)
        {
            return true;
        }
    }
    false
}
pub(crate) fn validate_transfer(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
) -> Result<PortTransfer, JsErrorBox> {
    let (token, endpoint) = lookup_endpoint(scope, object)?;
    let endpoint = endpoint
        .ok_or_else(|| crate::crypto_clone::data_clone_error(scope, "MessagePort is detached"))?;
    if endpoint.borrow().generation != token.generation || token.generation == u32::MAX {
        return Err(crate::crypto_clone::data_clone_error(
            scope,
            "MessagePort is already detached",
        ));
    }
    Ok(PortTransfer {
        token,
        endpoint: Rc::downgrade(&endpoint),
        registry: Rc::downgrade(&registry(scope)),
        anchor_pin: None,
    })
}
pub(crate) fn is_endpoint(transfer: &PortTransfer, id: u32) -> bool {
    transfer.token.id == id
}
pub(crate) fn same_transfer(a: &PortTransfer, b: &PortTransfer) -> bool {
    a.token.id == b.token.id
}
pub(crate) fn transfer_matches(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    transfer: &PortTransfer,
) -> bool {
    token(scope, object) == Some(transfer.token)
}
pub(crate) fn revalidate(transfer: &PortTransfer) -> bool {
    transfer
        .endpoint
        .upgrade()
        .is_some_and(|endpoint| endpoint.borrow().generation == transfer.token.generation)
}
pub(crate) fn commit(scope: &mut v8::PinScope, transfer: &mut PortTransfer) {
    let endpoint = transfer
        .endpoint
        .upgrade()
        .expect("validated port endpoint disappeared");
    let anchor = endpoint
        .borrow()
        .anchor
        .to_local(scope)
        .expect("validated port anchor disappeared");
    transfer.anchor_pin = Some(Rc::new(RefCell::new(Some(v8::Global::new(scope, anchor)))));
    if let Some(owner) = current_owner(scope, &endpoint.borrow()) {
        if let Some(key) = graph_key(scope, "anchor") {
            owner.delete_private(scope, key);
        }
    }
    let null = v8::null(scope);
    graph_set(scope, anchor, "owner", null.into());
    {
        let mut state = endpoint.borrow_mut();
        state.generation += 1;
        state.owned = false;
        state.task_pin = None;
        state.started = false;
        state.scheduled = false;
        transfer.token.generation = state.generation;
    }
    if let Some(registry) = transfer.registry.upgrade() {
        dirty(&registry, &endpoint);
        refresh_dirty(scope, &registry);
    }
}
pub(crate) fn transfer_anchor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    transfer: &PortTransfer,
) -> Option<v8::Local<'s, v8::Object>> {
    transfer.endpoint.upgrade()?.borrow().anchor.to_local(scope)
}
pub(crate) fn pin_transfer(scope: &mut v8::PinScope, transfer: &PortTransfer) {
    if let Some(pin) = &transfer.anchor_pin {
        if pin.borrow().is_none() {
            if let Some(anchor) = transfer_anchor(scope, transfer) {
                *pin.borrow_mut() = Some(v8::Global::new(scope, anchor));
            }
        }
    }
}
pub(crate) fn release_pin(transfer: &PortTransfer) {
    if let Some(pin) = &transfer.anchor_pin {
        pin.borrow_mut().take();
    }
}
pub(crate) fn receive<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    transfer: &PortTransfer,
) -> Option<v8::Local<'s, v8::Object>> {
    let endpoint = transfer.endpoint.upgrade()?;
    if endpoint.borrow().generation != transfer.token.generation || endpoint.borrow().owned {
        return None;
    }
    let context = scope.get_current_context();
    let adapter = context.get_slot::<Adapter>()?;
    let create = crate::context_ownership::local(scope, &adapter.create);
    let receiver = v8::undefined(scope).into();
    let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
    let scope = &mut scope.init();
    let object = create.call(scope, receiver, &[])?.to_object(scope)?;
    set_token(scope, object, transfer.token)?;
    let anchor = endpoint.borrow().anchor.to_local(scope)?;
    graph_set(scope, object, "anchor", anchor.into());
    graph_set(scope, anchor, "owner", object.into());
    endpoint.borrow_mut().owned = true;
    if let Some(registry) = transfer.registry.upgrade() {
        dirty(&registry, &endpoint);
        refresh_dirty(scope, &registry);
    }
    Some(object)
}
fn schedule(scope: &mut v8::PinScope, endpoint: &EndpointHandle) {
    let owner = {
        let mut endpoint = endpoint.borrow_mut();
        if endpoint.closed || !endpoint.started || endpoint.scheduled || endpoint.queue.is_empty() {
            return;
        }
        let Some(owner) = current_owner(scope, &endpoint) else {
            return;
        };
        let owner = v8::Global::new(scope, owner);
        endpoint.scheduled = true;
        endpoint.task_pin = endpoint.anchor.to_global(scope);
        owner
    };
    let object = v8::Local::new(scope, &owner);
    let Some(context) = object.get_creation_context(scope) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(adapter) = context.get_slot::<Adapter>() else {
        return;
    };
    let callback = crate::context_ownership::local(scope, &adapter.schedule);
    let receiver = v8::undefined(scope).into();
    callback.call(scope, receiver, &[object.into()]);
}
#[op2(fast)]
fn op_port_install_adapter(
    scope: &mut v8::PinScope,
    create: v8::Local<v8::Function>,
    dispatch: v8::Local<v8::Function>,
    schedule: v8::Local<v8::Function>,
) {
    let Some(context) = create.get_creation_context(scope) else {
        return;
    };
    {
        let scope = &mut v8::ContextScope::new(scope, context);
        crate::browser_env::set_native_event_target_observer(scope, listener_changed);
    }
    let Some(create) = crate::context_ownership::function(scope, create) else {
        return;
    };
    let Some(dispatch) = crate::context_ownership::function(scope, dispatch) else {
        return;
    };
    let Some(schedule) = crate::context_ownership::function(scope, schedule) else {
        return;
    };
    context.set_slot(Rc::new(Adapter {
        create,
        dispatch,
        schedule,
    }));
}
#[op2(reentrant)]
fn op_port_new_channel<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Array>, JsErrorBox> {
    let context = owner
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Port interface realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    sweep(scope);
    let registry = registry(scope);
    if registry.borrow().endpoints.len() > 131070 {
        return Err(JsErrorBox::range_error(
            "Native port endpoint budget exceeded",
        ));
    }
    let mut ports = Vec::new();
    for _ in 0..2 {
        let (token, endpoint) = {
            let mut registry = registry.borrow_mut();
            registry.next = registry
                .next
                .checked_add(1)
                .ok_or_else(|| JsErrorBox::generic("Port identifier budget exceeded"))?;
            let token = PortToken {
                id: registry.next,
                generation: 0,
            };
            let anchor = v8::Object::new(scope);
            let null = v8::null(scope);
            anchor.set_prototype(scope, null.into());
            let anchor = v8::Weak::new(scope, anchor);
            let endpoint = Rc::new(RefCell::new(Endpoint {
                id: token.id,
                anchor,
                task_pin: None,
                generation: 0,
                peer: None,
                owned: false,
                started: false,
                closed: false,
                scheduled: false,
                queue: VecDeque::new(),
                reservations: 0,
            }));
            registry.endpoints.insert(token.id, endpoint.clone());
            (token, endpoint)
        };
        let transfer = PortTransfer {
            token,
            endpoint: Rc::downgrade(&endpoint),
            registry: Rc::downgrade(&registry),
            anchor_pin: None,
        };
        let object = receive(scope, &transfer)
            .ok_or_else(|| JsErrorBox::generic("Port interface unavailable"))?;
        ports.push((token, object));
    }
    {
        let registry = registry.borrow();
        registry.endpoints[&ports[0].0.id].borrow_mut().peer = Some(ports[1].0.id);
        registry.endpoints[&ports[1].0.id].borrow_mut().peer = Some(ports[0].0.id);
    }
    for (token, _) in &ports {
        registry.borrow_mut().dirty.insert(token.id);
    }
    refresh_dirty(scope, &registry);
    let values = ports
        .into_iter()
        .map(|(_, object)| object.into())
        .collect::<Vec<_>>();
    Ok(v8::Array::new_with_elements(scope, &values))
}
#[op2(nofast, reentrant)]
fn op_port_post_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<v8::Object>,
    value: v8::Local<'s, v8::Value>,
    transfer: Option<v8::Local<'s, v8::Value>>,
) -> Result<(), JsErrorBox> {
    let (token, source) = lookup_endpoint(scope, object)?;
    let Some(source) = source else { return Ok(()) };
    let peer = {
        let source = source.borrow();
        if source.generation != token.generation || source.closed {
            return Ok(());
        }
        source.peer
    };
    let registry = registry(scope);
    let destination = peer.and_then(|peer| registry.borrow().endpoints.get(&peer).cloned());
    let doomed = peer.is_some_and(|peer| transfer_contains_endpoint(scope, transfer, peer));
    let mut reservation = destination
        .as_ref()
        .filter(|destination| !doomed && !destination.borrow().closed)
        .map(|destination| Reservation::new(scope, registry.clone(), destination.clone()))
        .transpose()?;
    v8::tc_scope!(let scope,scope);
    let result = crate::crypto_clone::prepare_transfer(scope, value, transfer, Some(object));
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(());
    }
    let prepared = result?;
    // A missing/closed/doomed receiver still performs the specified transfer,
    // but consumes no delivery queue capacity.
    let source_live = {
        let state = source.borrow();
        state.generation == token.generation && !state.closed && state.peer == peer
    };
    let deliver = source_live
        && peer.is_some_and(|peer| !prepared.transfers_endpoint(peer))
        && destination.as_ref().is_some_and(|destination| {
            let state = destination.borrow();
            !state.closed && state.peer == Some(token.id)
        });
    if deliver {
        reservation
            .as_mut()
            .expect("live receiver reservation")
            .admit(prepared.size_bytes())?;
    }
    let data = prepared.commit(scope)?;
    if !deliver {
        return Ok(());
    }
    reservation
        .take()
        .expect("live receiver reservation")
        .enqueue(scope, data);
    let destination = destination.expect("live receiver");
    schedule(scope, &destination);
    Ok(())
}
#[op2(nofast, reentrant)]
fn op_port_start(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let (token, endpoint) = lookup_endpoint(scope, object)?;
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    if endpoint.borrow().generation != token.generation {
        return Ok(());
    }
    endpoint.borrow_mut().started = true;
    schedule(scope, &endpoint);
    Ok(())
}
#[op2(fast)]
fn op_port_close(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let (token, endpoint) = lookup_endpoint(scope, object)?;
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    if endpoint.borrow().generation != token.generation {
        return Ok(());
    }
    let queue = disentangle(&registry(scope), token.id, &endpoint);
    discard_queue(queue);
    let port_registry = registry(scope);
    refresh_dirty(scope, &port_registry);
    Ok(())
}
#[op2(nofast, reentrant)]
fn op_port_deliver(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let (token, endpoint) = lookup_endpoint(scope, object)?;
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    let data = {
        let mut endpoint = endpoint.borrow_mut();
        if endpoint.generation != token.generation {
            return Ok(());
        }
        endpoint.scheduled = false;
        endpoint.task_pin = None;
        if endpoint.closed || !endpoint.started {
            return Ok(());
        }
        endpoint.queue.pop_front()
    };
    if let Some(data) = &data {
        data.pin_port_anchors(scope);
    }
    let port_registry = registry(scope);
    dirty(&port_registry, &endpoint);
    refresh_dirty(scope, &port_registry);
    let Some(data) = data else {
        return Ok(());
    };
    {
        let registry = registry(scope);
        let mut registry = registry.borrow_mut();
        registry.queued_bytes = registry.queued_bytes.saturating_sub(data.size_bytes());
    }
    let context = object
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("Port owner unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let (value, ports) = crate::crypto_clone::deserialize_with_transfers(scope, &data)?;
    let values = ports
        .into_iter()
        .map(|port| port.into())
        .collect::<Vec<_>>();
    let ports_array = v8::Array::new_with_elements(scope, &values);
    let adapter = context
        .get_slot::<Adapter>()
        .ok_or_else(|| JsErrorBox::generic("Port event adapter unavailable"))?;
    let dispatch = crate::context_ownership::local(scope, &adapter.dispatch);
    let receiver = v8::undefined(scope).into();
    dispatch.call(scope, receiver, &[object.into(), value, ports_array.into()]);
    schedule(scope, &endpoint);
    Ok(())
}
fn disentangle(
    registry: &RegistryHandle,
    id: u32,
    endpoint: &EndpointHandle,
) -> VecDeque<crate::crypto_clone::CloneSnapshot> {
    let (peer, queue) = {
        let mut state = endpoint.borrow_mut();
        state.closed = true;
        state.started = false;
        state.scheduled = false;
        state.task_pin = None;
        (state.peer.take(), std::mem::take(&mut state.queue))
    };
    {
        let mut registry = registry.borrow_mut();
        registry.dirty.insert(id);
        if let Some(peer) = peer {
            registry.dirty.insert(peer);
        }
        registry.queued_bytes = registry.queued_bytes.saturating_sub(
            queue
                .iter()
                .map(crate::crypto_clone::CloneSnapshot::size_bytes)
                .sum::<usize>(),
        );
    }
    if let Some(peer) = peer {
        let peer = registry.borrow().endpoints.get(&peer).cloned();
        if let Some(peer) = peer {
            let mut peer = peer.borrow_mut();
            if peer.peer == Some(id) {
                peer.peer = None;
            }
        }
    }
    queue
}
fn discard_queue(mut queue: VecDeque<crate::crypto_clone::CloneSnapshot>) {
    // Removing ownership plans first makes destruction iterative even when
    // queued messages contain ports with further queued transfers.
    while let Some(snapshot) = queue.pop_front() {
        for transfer in snapshot.take_port_transfers() {
            queue.extend(abandon(&transfer));
        }
    }
}
pub(crate) fn abandon(transfer: &PortTransfer) -> VecDeque<crate::crypto_clone::CloneSnapshot> {
    let Some(endpoint) = transfer.endpoint.upgrade() else {
        return VecDeque::new();
    };
    let Some(registry) = transfer.registry.upgrade() else {
        return VecDeque::new();
    };
    {
        let state = endpoint.borrow();
        if state.generation != transfer.token.generation || state.owned || state.closed {
            return VecDeque::new();
        }
    }
    disentangle(&registry, transfer.token.id, &endpoint)
}
pub(crate) fn discard_transfers(transfers: impl IntoIterator<Item = PortTransfer>) {
    let mut queue = VecDeque::new();
    for transfer in transfers {
        queue.extend(abandon(&transfer));
    }
    discard_queue(queue);
}
pub(crate) fn retire_context(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    let registry = registry(scope);
    let endpoints = registry
        .borrow()
        .endpoints
        .iter()
        .map(|(id, endpoint)| (*id, endpoint.clone()))
        .collect::<Vec<_>>();
    let mut queue = VecDeque::new();
    for (id, endpoint) in endpoints {
        let Some(object) = current_owner(scope, &endpoint.borrow()) else {
            continue;
        };
        if object.get_creation_context(scope) == Some(context) {
            endpoint.borrow_mut().owned = false;
            if let Some(anchor) = endpoint.borrow().anchor.to_local(scope) {
                let null = v8::null(scope);
                graph_set(scope, anchor, "owner", null.into());
            }
            queue.extend(disentangle(&registry, id, &endpoint));
        }
    }
    discard_queue(queue);
    refresh_dirty(scope, &registry);
}
pub(crate) fn clear(rt: &mut JsRuntime) {
    let Some(registry) = rt
        .op_state()
        .borrow()
        .try_borrow::<RegistryHandle>()
        .cloned()
    else {
        return;
    };
    let endpoints = {
        let mut registry = registry.borrow_mut();
        registry.queued_bytes = 0;
        registry.reserved_bytes = 0;
        registry.dirty.clear();
        std::mem::take(&mut registry.endpoints)
    };
    // Mark first, then destroy outside registry borrows. Snapshot destructors can
    // inspect other transferred endpoints while discarding their ownership plans.
    for endpoint in endpoints.values() {
        let mut endpoint = endpoint.borrow_mut();
        endpoint.closed = true;
        endpoint.peer = None;
        endpoint.task_pin = None;
    }
    drop(endpoints);
}
pub(crate) fn set_limits(scope: &mut v8::PinScope, max_messages: usize, max_bytes: usize) {
    let registry = registry(scope);
    let mut registry = registry.borrow_mut();
    registry.max_messages = max_messages;
    registry.max_bytes = max_bytes;
}
pub(crate) fn diagnostics(scope: &mut v8::PinScope) -> serde_json::Value {
    sweep(scope);
    let registry = registry(scope);
    let registry = registry.borrow();
    serde_json::json!({"endpoints":registry.endpoints.len(),"queuedBytes":registry.queued_bytes,"reservedBytes":registry.reserved_bytes,
      "queuedMessages":registry.endpoints.values().map(|endpoint|endpoint.borrow().queue.len()).sum::<usize>(),
      "reservations":registry.endpoints.values().map(|endpoint|endpoint.borrow().reservations).sum::<usize>()})
}
deno_core::extension!(
    turbo_surf_message_ports,
    ops = [
        op_port_install_adapter,
        op_port_new_channel,
        op_port_post_message,
        op_port_start,
        op_port_close,
        op_port_deliver
    ],
    state = |state| state.put(RegistryHandle::default())
);
pub(crate) fn extension() -> Extension {
    turbo_surf_message_ports::init()
}

pub(crate) fn clear_context(context: v8::Local<v8::Context>) {
    context.remove_slot::<Adapter>();
}
