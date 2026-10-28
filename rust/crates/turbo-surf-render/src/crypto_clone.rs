//! V8 graph cloning with private native key handles carried out of band.
use deno_core::v8::{ValueDeserializerHelper, ValueSerializerHelper, ValueSerializerImpl};
use deno_core::{op2, v8, Extension};
use deno_error::JsErrorBox;
use std::{cell::RefCell, rc::Rc};

struct KeyAdapter {
    key: v8::Weak<v8::Function>,
    error: v8::Weak<v8::Function>,
}

#[op2(fast)]
fn op_install_crypto_clone_adapter(
    scope: &mut v8::PinScope,
    adapter: v8::Local<v8::Function>,
    error: v8::Local<v8::Function>,
) {
    if let Some(context) = adapter.get_creation_context(scope) {
        let Some(key) = crate::context_ownership::function(scope, adapter) else {
            return;
        };
        let Some(error) = crate::context_ownership::function(scope, error) else {
            return;
        };
        context.set_slot(Rc::new(KeyAdapter { key, error }));
    }
}

#[op2(fast)]
fn op_surf_is_window(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return false;
    };
    object
        .get_creation_context(scope)
        .is_some_and(|context| object == context.global(scope))
}

#[derive(Default)]
struct CloneHandles {
    local_clone: bool,
    /// Chrome's "Failed to execute ..." prefix for the transfer-plan errors raised
    /// while serializing (V8's own clone errors are still unprefixed: turbo-surf#76).
    prefix: std::cell::Cell<&'static str>,
    /// Every port/stream named by the transfer list, live or not: the value may
    /// reference a neutered port or locked stream that the post-serialization
    /// checks reject with Chrome's text.
    listed_ports: RefCell<Vec<crate::message_ports::PortToken>>,
    listed_streams: RefCell<Vec<v8::Global<v8::Object>>>,
    stream_plans: RefCell<Vec<StreamPlan>>,
    public_ports: std::cell::Cell<usize>,
    objects: RefCell<Vec<NativeSnapshot>>,
    transfers: RefCell<Vec<v8::SharedRef<v8::BackingStore>>>,
    shared: RefCell<Vec<SharedSnapshot>>,
    wasm: RefCell<Vec<v8::CompiledWasmModule>>,
    ports: RefCell<Vec<crate::message_ports::PortTransfer>>,
    port_objects: RefCell<Vec<v8::Global<v8::Object>>>,
}

impl Drop for CloneHandles {
    fn drop(&mut self) {
        crate::message_ports::discard_transfers(std::mem::take(self.ports.get_mut()));
    }
}

struct StreamPlan {
    object: v8::Global<v8::Object>,
    plan: v8::Global<v8::Object>,
    kind: u32,
    first: usize,
}

struct SharedSnapshot {
    store: v8::SharedRef<v8::BackingStore>,
    identity: Option<(u32, String)>,
    budget_bytes: usize,
}

struct Delegate(Rc<CloneHandles>);

enum NativeSnapshot {
    Key(Box<deno_crypto::NativeKeySnapshot>),
    HybridKey(crate::crypto_hybrid::HybridKeySnapshot),
    Port(usize),
    Stream { kind: u32, first: usize },
    Rectangle(crate::geometry_backend::RectData),
    PointMatrix(crate::geometry_backend::PointMatrixData),
    Exception { name: String, message: String },
}

impl NativeSnapshot {
    fn size_bytes(&self) -> usize {
        match self {
            Self::Key(key) => key.size_bytes(),
            Self::HybridKey(key) => key.size_bytes(),
            Self::Exception { name, message } => name.len().saturating_add(message.len()),
            Self::Port(_) | Self::Stream { .. } => 0,
            Self::Rectangle(_) => 40,
            Self::PointMatrix(data) => data.size_bytes(),
        }
    }
}

fn own_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
    let name = v8::String::new(scope, name)?;
    if object.has_own_property(scope, name.into()) != Some(true) {
        return None;
    }
    object.get(scope, name.into())
}

fn exception_snapshot<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> Option<NativeSnapshot> {
    let record = crate::browser_env::get_interface_record(scope, object)?.to_object(scope)?;
    let runtime = own_value(scope, record, "runtime")?.to_object(scope)?;
    let state = own_value(scope, runtime, "DOMException")?.to_object(scope)?;
    let name = own_value(scope, state, "name")?;
    let message = own_value(scope, state, "message")?;
    if !name.is_string() || !message.is_string() {
        return None;
    }
    Some(NativeSnapshot::Exception {
        name: name.to_rust_string_lossy(scope),
        message: message.to_rust_string_lossy(scope),
    })
}

/// Serialized graph bytes contain only key indices; handles stay in this isolate.
pub(crate) struct CloneSnapshot {
    bytes: Vec<u8>,
    handles: Rc<CloneHandles>,
}

impl CloneSnapshot {
    pub(crate) fn transfers_endpoint(&self, id: u32) -> bool {
        self.handles
            .ports
            .borrow()
            .iter()
            .any(|port| crate::message_ports::is_endpoint(port, id))
    }
    pub(crate) fn size_bytes(&self) -> usize {
        self.handles
            .transfers
            .borrow()
            .iter()
            .map(|store| store.byte_length())
            .chain(
                self.handles
                    .shared
                    .borrow()
                    .iter()
                    .map(|snapshot| snapshot.budget_bytes),
            )
            .chain(
                self.handles
                    .objects
                    .borrow()
                    .iter()
                    .map(NativeSnapshot::size_bytes),
            )
            .fold(self.bytes.len(), usize::saturating_add)
    }
    pub(crate) fn port_anchors<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> Vec<v8::Local<'s, v8::Object>> {
        self.handles
            .ports
            .borrow()
            .iter()
            .filter_map(|port| crate::message_ports::transfer_anchor(scope, port))
            .collect()
    }
    pub(crate) fn pin_port_anchors(&self, scope: &mut v8::PinScope) {
        for port in self.handles.ports.borrow().iter() {
            crate::message_ports::pin_transfer(scope, port);
        }
    }
    pub(crate) fn release_port_pins(&self) {
        for port in self.handles.ports.borrow().iter() {
            crate::message_ports::release_pin(port);
        }
    }
    pub(crate) fn take_port_transfers(&self) -> Vec<crate::message_ports::PortTransfer> {
        std::mem::take(&mut *self.handles.ports.borrow_mut())
    }
}

impl Delegate {
    /// Chrome's text for a stream the value references without a transfer plan:
    /// a listed (locked) stream or one missing from the transfer list.
    fn untransferable_stream<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        object: v8::Local<'s, v8::Object>,
    ) -> Option<String> {
        let kind = crate::stream_transfers::kind(scope, object).ok()?;
        let name = stream_interface(kind)?;
        let listed = self
            .0
            .listed_streams
            .borrow()
            .iter()
            .any(|stream| v8::Local::new(scope, stream) == object);
        let reason = if listed { "locked" } else { "not transferred." };
        let prefix = self.0.prefix.get();
        Some(format!(
            "{prefix}A {name} could not be cloned because it was {reason}"
        ))
    }
}

impl v8::ValueSerializerImpl for Delegate {
    fn throw_data_clone_error<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        message: v8::Local<'s, v8::String>,
    ) {
        if let Some(adapter) = scope.get_current_context().get_slot::<KeyAdapter>() {
            let factory = crate::context_ownership::local(scope, &adapter.error);
            let receiver = v8::undefined(scope);
            let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
            let scope = &mut scope.init();
            if let Some(error) = factory.call(scope, receiver.into(), &[message.into()]) {
                scope.throw_exception(error);
            }
        } else {
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        }
    }

    fn has_custom_host_object(&self, _: &v8::Isolate) -> bool {
        true
    }

    fn is_host_object<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        object: v8::Local<'s, v8::Object>,
    ) -> Option<bool> {
        if crate::message_ports::token(scope, object).is_some()
            || object.internal_field_count() > 0
            || deno_crypto::is_crypto_object(scope, object.into())
            || crate::crypto_hybrid::is_hybrid_key(scope, object.into())
        {
            return Some(true);
        }
        // The canonical Event brand lives in a private WeakMap. Its captured
        // accessor never reads page properties or constructor prototypes.
        let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
        let scope = &mut scope.init();
        Some(
            crate::browser_env::is_native_event(scope, object)
                || crate::browser_env::is_native_event_target(scope, object)
                || crate::browser_env::is_native_abort_object(scope, object)
                || crate::browser_env::is_native_xpath_object(scope, object)
                || crate::browser_env::get_interface_record(scope, object)
                    .is_some_and(|record| record.is_object())
                || crate::geometry_backend::point_matrix_snapshot(scope, object).is_some(),
        )
    }

    fn write_host_object<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        object: v8::Local<'s, v8::Object>,
        serializer: &dyn v8::ValueSerializerHelper,
    ) -> Option<bool> {
        let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
        let scope = &mut scope.init();
        let snapshot = if crate::message_ports::token(scope, object).is_some() {
            let ports = self.0.ports.borrow();
            let index = ports
                .iter()
                .position(|port| crate::message_ports::transfer_matches(scope, object, port));
            let listed = crate::message_ports::token(scope, object)
                .is_some_and(|token| self.0.listed_ports.borrow().contains(&token));
            match index {
                Some(index) => NativeSnapshot::Port(index),
                // A listed neutered port: the post-serialization check rejects it.
                None if listed => NativeSnapshot::Port(usize::MAX),
                None => {
                    let prefix = self.0.prefix.get();
                    let message = v8::String::new(
                        scope,
                        &format!("{prefix}A MessagePort could not be cloned because it was not transferred."),
                    )?;
                    self.throw_data_clone_error(scope, message);
                    return None;
                }
            }
        } else if let Some((kind, first)) = self
            .0
            .stream_plans
            .borrow()
            .iter()
            .find(|plan| v8::Local::new(scope, &plan.object) == object)
            .map(|plan| (plan.kind, plan.first))
        {
            // A getter may lock a planned stream before the serializer reaches it.
            if crate::stream_transfers::kind(scope, object).ok()? < 0 {
                let text = self.untransferable_stream(scope, object)?;
                let message = v8::String::new(scope, &text)?;
                self.throw_data_clone_error(scope, message);
                return None;
            }
            NativeSnapshot::Stream { kind, first }
        } else if deno_crypto::is_crypto_key(scope, object.into()) {
            NativeSnapshot::Key(Box::new(deno_crypto::NativeKeySnapshot::capture(
                scope,
                object.into(),
            )?))
        } else if crate::crypto_hybrid::is_hybrid_key(scope, object.into()) {
            NativeSnapshot::HybridKey(crate::crypto_hybrid::HybridKeySnapshot::capture(
                scope,
                object.into(),
            )?)
        } else if let Some(rectangle) = crate::geometry_backend::snapshot(scope, object) {
            NativeSnapshot::Rectangle(rectangle)
        } else if let Some(data) = crate::geometry_backend::point_matrix_snapshot(scope, object) {
            NativeSnapshot::PointMatrix(data)
        } else if let Some(snapshot) = exception_snapshot(scope, object) {
            snapshot
        } else if let Some(text) = self.untransferable_stream(scope, object) {
            let message = v8::String::new(scope, &text)?;
            self.throw_data_clone_error(scope, message);
            return None;
        } else {
            let message = v8::String::new(scope, "Unsupported native object")?;
            self.throw_data_clone_error(scope, message);
            return None;
        };
        let mut objects = self.0.objects.borrow_mut();
        let index = u32::try_from(objects.len()).ok()?;
        objects.push(snapshot);
        serializer.write_uint32(index);
        Some(true)
    }

    fn get_shared_array_buffer_id<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        buffer: v8::Local<'s, v8::SharedArrayBuffer>,
    ) -> Option<u32> {
        if !self.0.local_clone && !crate::runtime::shared_memory_allowed(scope) {
            let message =
                v8::String::new(scope, "Shared memory requires cross-origin isolation").unwrap();
            self.throw_data_clone_error(scope, message);
            return None;
        }
        let mut stores = self.0.shared.borrow_mut();
        let index = u32::try_from(stores.len()).ok()?;
        stores.push(SharedSnapshot {
            budget_bytes: buffer.byte_length(),
            store: buffer.get_backing_store(),
            identity: crate::runtime::shared_memory_identity(scope),
        });
        Some(index)
    }

    fn get_wasm_module_transfer_id<'s, 'i>(
        &self,
        _: &mut v8::PinScope<'s, 'i>,
        module: v8::Local<v8::WasmModuleObject>,
    ) -> Option<u32> {
        let mut modules = self.0.wasm.borrow_mut();
        let index = u32::try_from(modules.len()).ok()?;
        modules.push(module.get_compiled_module());
        Some(index)
    }
}

impl v8::ValueDeserializerImpl for Delegate {
    fn read_host_object<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        deserializer: &dyn v8::ValueDeserializerHelper,
    ) -> Option<v8::Local<'s, v8::Object>> {
        let mut index = 0;
        if !deserializer.read_uint32(&mut index) {
            return None;
        }
        let objects = self.0.objects.borrow();
        let snapshot = objects.get(index as usize)?;
        if let NativeSnapshot::Port(index) = snapshot {
            return self
                .0
                .port_objects
                .borrow()
                .get(*index)
                .map(|object| v8::Local::new(scope, object));
        }
        if let NativeSnapshot::Stream { kind, first } = snapshot {
            let kind = *kind;
            let first = *first;
            drop(objects);
            let ports = self.0.port_objects.borrow();
            let a = v8::Local::new(scope, ports.get(first)?);
            let b: v8::Local<v8::Value> = if kind == 3 {
                v8::Local::new(scope, ports.get(first + 1)?).into()
            } else {
                v8::undefined(scope).into()
            };
            drop(ports);
            let kind = v8::Integer::new_from_unsigned(scope, kind);
            return crate::stream_transfers::call(scope, 3, &[kind.into(), a.into(), b])?
                .to_object(scope);
        }
        if let NativeSnapshot::Rectangle(data) = snapshot {
            let data = *data;
            drop(objects);
            let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
            let scope = &mut scope.init();
            return crate::geometry_backend::restore(scope, data);
        }
        if let NativeSnapshot::PointMatrix(data) = snapshot {
            let data = data.clone();
            drop(objects);
            let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
            let scope = &mut scope.init();
            return crate::geometry_backend::point_matrix_restore(scope, &data);
        }
        let adapter = scope.get_current_context().get_slot::<KeyAdapter>()?;
        if let NativeSnapshot::Exception { name, message } = snapshot {
            let factory = crate::context_ownership::local(scope, &adapter.error);
            let message = v8::String::new(scope, message)?;
            let name = v8::String::new(scope, name)?;
            drop(objects);
            let receiver = v8::undefined(scope);
            let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
            let scope = &mut scope.init();
            return factory
                .call(scope, receiver.into(), &[message.into(), name.into()])?
                .to_object(scope);
        }
        // The snapshot has no guest wrapper/prototype/metadata references.
        // Each delivery reconstructs native state; V8 preserves graph aliases.
        let key = match snapshot {
            NativeSnapshot::Key(key) => key.restore(scope),
            NativeSnapshot::HybridKey(key) => key.restore(scope),
            _ => return None,
        };
        drop(objects);
        let adapter = crate::context_ownership::local(scope, &adapter.key);
        let receiver = v8::undefined(scope);
        let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
        let scope = &mut scope.init();
        adapter
            .call(scope, receiver.into(), &[key.into()])?
            .to_object(scope)
    }

    fn get_shared_array_buffer_from_id<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        index: u32,
    ) -> Option<v8::Local<'s, v8::SharedArrayBuffer>> {
        if !crate::runtime::shared_memory_allowed(scope) {
            let message =
                v8::String::new(scope, "Shared memory requires cross-origin isolation").unwrap();
            self.throw_data_clone_error(scope, message);
            return None;
        }
        let stores = self.0.shared.borrow();
        let snapshot = stores.get(index as usize)?;
        if snapshot.identity != crate::runtime::shared_memory_identity(scope) {
            let message =
                v8::String::new(scope, "Shared memory cannot cross agent clusters").unwrap();
            self.throw_data_clone_error(scope, message);
            return None;
        }
        Some(v8::SharedArrayBuffer::with_backing_store(
            scope,
            &snapshot.store,
        ))
    }

    fn get_wasm_module_from_id<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        index: u32,
    ) -> Option<v8::Local<'s, v8::WasmModuleObject>> {
        let modules = self.0.wasm.borrow();
        v8::WasmModuleObject::from_compiled_module(scope, modules.get(index as usize)?)
    }
}

pub(crate) fn data_clone_error(scope: &mut v8::PinScope, message: &str) -> JsErrorBox {
    if let Some(text) = v8::String::new(scope, message) {
        Delegate(Rc::default()).throw_data_clone_error(scope, text);
    }
    JsErrorBox::new("DOMExceptionDataCloneError", message.to_string())
}

struct ValidatedTransfers<'s> {
    listed: ListedTransfers<'s>,
    buffers: Vec<v8::Local<'s, v8::ArrayBuffer>>,
    ports: Vec<crate::message_ports::PortTransfer>,
    streams: Vec<StreamPlan>,
    public_ports: usize,
}
/// Chrome's exception prefix for the operation that validates the transfer list.
fn clone_prefix(local_clone: bool, sender: bool) -> &'static str {
    if local_clone {
        "Failed to execute 'structuredClone' on 'Window': "
    } else if sender {
        "Failed to execute 'postMessage' on 'MessagePort': "
    } else {
        "Failed to execute 'postMessage' on 'Window': "
    }
}

/// The interface of a `stream_transfers::kind` result (negative when locked).
fn stream_interface(kind: i32) -> Option<&'static str> {
    ["ReadableStream", "WritableStream", "TransformStream"]
        .get((kind.unsigned_abs() as usize).wrapping_sub(1))
        .copied()
}

/// Chrome 156's list scan: primitives, non-transferable values and duplicates
/// (list index) fail before the value is serialized; the remaining checks run
/// after it (`post_serialization_checks`).
fn validate_transfers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    transfer: Option<v8::Local<'s, v8::Value>>,
    prefix: &str,
) -> Result<ValidatedTransfers<'s>, JsErrorBox> {
    let Some(transfer) = transfer.filter(|value| !value.is_undefined()) else {
        return Ok(ValidatedTransfers {
            listed: ListedTransfers {
                buffers: Vec::new(),
                ports: Vec::new(),
                streams: Vec::new(),
            },
            buffers: Vec::new(),
            ports: Vec::new(),
            streams: Vec::new(),
            public_ports: 0,
        });
    };
    let array = v8::Local::<v8::Array>::try_from(transfer)
        .map_err(|_| JsErrorBox::type_error("Transfer sequence required"))?;
    if array.length() > 4096 {
        return Err(JsErrorBox::range_error(
            "Native transfer sequence budget exceeded",
        ));
    }
    let mut buffers = Vec::new();
    let mut port_objects =
        Vec::<(v8::Local<'s, v8::Object>, crate::message_ports::PortToken)>::new();
    let mut stream_objects = Vec::<(v8::Local<'s, v8::Object>, i32)>::new();
    for index in 0..array.length() {
        if scope.is_execution_terminating() {
            return Err(JsErrorBox::generic("Clone interrupted"));
        }
        let value = array
            .get_index(scope, index)
            .ok_or_else(|| JsErrorBox::generic("Transfer sequence interrupted"))?;
        let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
            return Err(JsErrorBox::type_error(format!(
                "{prefix}Failed to convert value to 'object'."
            )));
        };
        if let Some(token) = crate::message_ports::token(scope, object) {
            if port_objects.iter().any(|(_, existing)| *existing == token) {
                return Err(data_clone_error(
                    scope,
                    &format!(
                        "{prefix}Message port at index {index} is a duplicate of an earlier port."
                    ),
                ));
            }
            port_objects.push((object, token));
            continue;
        }
        let kind = crate::stream_transfers::kind(scope, object)?;
        if kind != 0 {
            if stream_objects
                .iter()
                .any(|(existing, _)| *existing == object)
            {
                let name = stream_interface(kind).unwrap_or("Stream");
                return Err(data_clone_error(
                    scope,
                    &format!(
                        "{prefix}{name} at index {index} is a duplicate of an earlier {name}."
                    ),
                ));
            }
            stream_objects.push((object, kind));
            continue;
        }
        let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) else {
            return Err(data_clone_error(
                scope,
                &format!("{prefix}Value at index {index} does not have a transferable type."),
            ));
        };
        if buffers.contains(&buffer) {
            return Err(data_clone_error(
                scope,
                &format!("{prefix}ArrayBuffer at index {index} is a duplicate of an earlier ArrayBuffer."),
            ));
        }
        buffers.push(buffer);
    }
    // A neutered port or locked stream gets no transfer handle; the value may
    // still reference it, and the post-serialization checks reject it.
    let mut ports = Vec::new();
    for (object, _) in &port_objects {
        if let Some(port) = crate::message_ports::validate_transfer(scope, *object) {
            ports.push(port);
        }
    }
    let mut streams = Vec::<StreamPlan>::new();
    for &(object, kind) in &stream_objects {
        if kind < 0 {
            continue;
        }
        let plan = crate::stream_transfers::call(scope, 1, &[object.into()])
            .and_then(|value| value.to_object(scope))
            .ok_or_else(|| JsErrorBox::generic("Stream preparation interrupted"))?;
        streams.push(StreamPlan {
            object: v8::Global::new(scope, object),
            plan: v8::Global::new(scope, plan),
            kind: kind.unsigned_abs(),
            first: 0,
        });
    }
    let public_ports = ports.len();
    for entry in &mut streams {
        entry.first = ports.len();
        let plan = v8::Local::new(scope, &entry.plan);
        let receiver = own_value(scope, plan, "receiver")
            .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
            .ok_or_else(|| JsErrorBox::generic("Missing stream receiver"))?;
        for index in 0..receiver.length() {
            let object = receiver
                .get_index(scope, index)
                .and_then(|value| value.to_object(scope))
                .ok_or_else(|| JsErrorBox::generic("Missing stream endpoint"))?;
            let port = crate::message_ports::validate_transfer(scope, object)
                .ok_or_else(|| data_clone_error(scope, "MessagePort is detached"))?;
            ports.push(port);
        }
    }
    Ok(ValidatedTransfers {
        listed: ListedTransfers {
            buffers: buffers.clone(),
            ports: port_objects,
            streams: stream_objects
                .into_iter()
                .map(|(object, _)| object)
                .collect(),
        },
        buffers,
        ports,
        streams,
        public_ports,
    })
}

/// The transfer list as scanned, for Chrome's checks after value serialization.
struct ListedTransfers<'s> {
    buffers: Vec<v8::Local<'s, v8::ArrayBuffer>>,
    ports: Vec<(v8::Local<'s, v8::Object>, crate::message_ports::PortToken)>,
    streams: Vec<v8::Local<'s, v8::Object>>,
}

/// Chrome 156 runs these after serializing the value (a getter may have changed
/// them): detached, then non-detachable buffers (buffer index), locked streams,
/// the source port and neutered ports (port index).
fn post_serialization_checks<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    listed: &ListedTransfers<'s>,
    sender: Option<v8::Local<v8::Object>>,
    prefix: &str,
) -> Result<(), JsErrorBox> {
    if let Some(index) = listed
        .buffers
        .iter()
        .position(|buffer| buffer.was_detached())
    {
        return Err(data_clone_error(
            scope,
            &format!("{prefix}ArrayBuffer at index {index} is already detached."),
        ));
    }
    if let Some(index) = listed
        .buffers
        .iter()
        .position(|buffer| !buffer.is_detachable())
    {
        return Err(JsErrorBox::type_error(format!(
            "{prefix}ArrayBuffer at index {index} is not detachable and could not be transferred."
        )));
    }
    for stream in &listed.streams {
        if crate::stream_transfers::kind(scope, *stream)? < 0 {
            return Err(JsErrorBox::type_error(format!(
                "{prefix}Cannot transfer a locked stream"
            )));
        }
    }
    let source = sender.and_then(|sender| crate::message_ports::token(scope, sender));
    if let Some(index) = listed
        .ports
        .iter()
        .position(|(_, token)| Some(*token) == source)
    {
        return Err(data_clone_error(
            scope,
            &format!("{prefix}Port at index {index} contains the source port."),
        ));
    }
    for (index, (object, _)) in listed.ports.iter().enumerate() {
        if crate::message_ports::validate_transfer(scope, *object).is_none() {
            return Err(data_clone_error(
                scope,
                &format!("{prefix}Port at index {index} is already neutered."),
            ));
        }
    }
    Ok(())
}

pub(crate) struct PreparedClone<'s> {
    snapshot: CloneSnapshot,
    buffers: Vec<v8::Local<'s, v8::ArrayBuffer>>,
}
impl PreparedClone<'_> {
    pub(crate) fn size_bytes(&self) -> usize {
        self.snapshot.size_bytes()
    }
    pub(crate) fn transfers_endpoint(&self, id: u32) -> bool {
        self.snapshot.transfers_endpoint(id)
    }
    pub(crate) fn commit(self, scope: &mut v8::PinScope) -> Result<CloneSnapshot, JsErrorBox> {
        // Admission and lock validation precede commit. Chrome detaches buffers
        // before piping streams; an already-started source may synchronously pull
        // and change a later transfer. Preserve those ordered partial effects.
        if scope.is_execution_terminating() {
            return Err(JsErrorBox::generic("Clone interrupted"));
        }
        if self
            .buffers
            .iter()
            .any(|buffer| buffer.was_detached() || !buffer.is_detachable())
            || self
                .snapshot
                .handles
                .ports
                .borrow()
                .iter()
                .any(|port| !crate::message_ports::revalidate(port))
        {
            return Err(data_clone_error(
                scope,
                "Transfer ownership changed during serialization",
            ));
        }
        for entry in self.snapshot.handles.stream_plans.borrow().iter() {
            let object = v8::Local::new(scope, &entry.object);
            if crate::stream_transfers::kind(scope, object)? <= 0 {
                return Err(data_clone_error(
                    scope,
                    "Stream ownership changed during serialization",
                ));
            }
        }
        for buffer in &self.buffers {
            if buffer.detach(None) != Some(true) {
                return Err(data_clone_error(scope, "ArrayBuffer cannot be transferred"));
            }
        }
        let plans: Vec<_> = self
            .snapshot
            .handles
            .stream_plans
            .borrow()
            .iter()
            .map(|entry| v8::Local::new(scope, &entry.plan))
            .collect();
        let values: Vec<v8::Local<v8::Value>> = plans.iter().map(|plan| (*plan).into()).collect();
        let all_plans = v8::Array::new_with_elements(scope, &values);
        for plan in plans {
            if crate::stream_transfers::call(scope, 2, &[plan.into(), all_plans.into()]).is_none() {
                return Err(JsErrorBox::generic("Stream transfer interrupted"));
            }
        }
        if self
            .snapshot
            .handles
            .ports
            .borrow()
            .iter()
            .any(|port| !crate::message_ports::revalidate(port))
        {
            return Err(data_clone_error(
                scope,
                "MessagePort ownership changed during stream transfer",
            ));
        }
        self.snapshot.handles.stream_plans.borrow_mut().clear();
        for port in self.snapshot.handles.ports.borrow_mut().iter_mut() {
            crate::message_ports::commit(scope, port);
        }
        Ok(self.snapshot)
    }
}
pub(crate) fn prepare_transfer<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    transfer: Option<v8::Local<'s, v8::Value>>,
    sender: Option<v8::Local<v8::Object>>,
) -> Result<PreparedClone<'s>, JsErrorBox> {
    prepare_transfer_with_mode(scope, value, transfer, sender, false)
}

fn prepare_transfer_with_mode<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    transfer: Option<v8::Local<'s, v8::Value>>,
    sender: Option<v8::Local<v8::Object>>,
    local_clone: bool,
) -> Result<PreparedClone<'s>, JsErrorBox> {
    let prefix = clone_prefix(local_clone, sender.is_some());
    let ValidatedTransfers {
        listed,
        buffers,
        ports,
        streams,
        public_ports,
    } = validate_transfers(scope, transfer, prefix)?;
    let context = scope.get_current_context();
    let mut handles = CloneHandles::default();
    handles.local_clone = local_clone;
    handles.prefix.set(prefix);
    *handles.listed_ports.get_mut() = listed.ports.iter().map(|(_, token)| *token).collect();
    *handles.listed_streams.get_mut() = listed
        .streams
        .iter()
        .map(|stream| v8::Global::new(scope, *stream))
        .collect();
    let handles = Rc::new(handles);
    handles.ports.borrow_mut().extend_from_slice(&ports);
    handles.public_ports.set(public_ports);
    handles.stream_plans.borrow_mut().extend(streams);
    let serializer = v8::ValueSerializer::new(scope, Box::new(Delegate(handles.clone())));
    for (index, buffer) in buffers.iter().enumerate() {
        serializer.transfer_array_buffer(index as u32, *buffer);
    }
    serializer.write_header();
    if serializer.write_value(context, value) != Some(true) {
        return Err(JsErrorBox::new(
            "DOMExceptionDataCloneError",
            "Value cannot be cloned",
        ));
    }
    let bytes = serializer.release();
    post_serialization_checks(scope, &listed, sender, prefix)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(JsErrorBox::range_error(
            "Native message serialization budget exceeded",
        ));
    }
    if buffers
        .iter()
        .any(|buffer| buffer.was_detached() || !buffer.is_detachable())
        || ports
            .iter()
            .any(|port| !crate::message_ports::revalidate(port))
    {
        return Err(data_clone_error(
            scope,
            "Transfer ownership changed during serialization",
        ));
    }
    for buffer in &buffers {
        handles
            .transfers
            .borrow_mut()
            .push(buffer.get_backing_store());
    }
    Ok(PreparedClone {
        snapshot: CloneSnapshot { bytes, handles },
        buffers,
    })
}
pub(crate) fn deserialize<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    snapshot: &CloneSnapshot,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    deserialize_with_transfers(scope, snapshot).map(|(value, _)| value)
}

pub(crate) fn deserialize_with_transfers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    snapshot: &CloneSnapshot,
) -> Result<(v8::Local<'s, v8::Value>, Vec<v8::Local<'s, v8::Object>>), JsErrorBox> {
    let failed = || JsErrorBox::new("DOMExceptionDataCloneError", "Value cannot be cloned");
    let mut ports = Vec::new();
    for port in snapshot.handles.ports.borrow().iter() {
        ports.push(crate::message_ports::receive(scope, port).ok_or_else(failed)?);
    }
    *snapshot.handles.port_objects.borrow_mut() = ports
        .iter()
        .map(|port| v8::Global::new(scope, *port))
        .collect();
    let context = scope.get_current_context();
    let deserializer = v8::ValueDeserializer::new(
        scope,
        Box::new(Delegate(snapshot.handles.clone())),
        &snapshot.bytes,
    );
    for (index, backing) in snapshot.handles.transfers.borrow().iter().enumerate() {
        let buffer = v8::ArrayBuffer::with_backing_store(scope, backing);
        deserializer.transfer_array_buffer(index as u32, buffer);
    }
    if deserializer.read_header(context) != Some(true) {
        return Err(failed());
    }
    deserializer
        .read_value(context)
        .map(|value| {
            ports.truncate(snapshot.handles.public_ports.get());
            (value, ports)
        })
        .ok_or_else(failed)
}

pub(crate) struct MessageClone<'s> {
    pub value: v8::Local<'s, v8::Value>,
    pub ports: Vec<v8::Local<'s, v8::Object>>,
    pub failed: bool,
}
pub(crate) fn deserialize_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    snapshot: &CloneSnapshot,
) -> Result<MessageClone<'s>, JsErrorBox> {
    v8::tc_scope!(let scope,scope);
    match deserialize_with_transfers(scope, snapshot) {
        Ok((value, ports)) => Ok(MessageClone {
            value,
            ports,
            failed: false,
        }),
        Err(error) if scope.has_terminated() => {
            scope.rethrow();
            Err(error)
        }
        Err(_) => {
            scope.reset();
            Ok(MessageClone {
                value: v8::null(scope).into(),
                ports: Vec::new(),
                failed: true,
            })
        }
    }
}

#[op2(reentrant)]
fn op_surf_structured_clone<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    owner: v8::Local<v8::Object>,
    transfer: Option<v8::Local<'s, v8::Value>>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = owner
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Invalid clone realm"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    // Local cloning completes transfers before deserialization rejects forbidden
    // shared memory; messaging rejects during serialization instead.
    let result = prepare_transfer_with_mode(scope, value, transfer, None, true)
        .and_then(|prepared| prepared.commit(scope))
        .and_then(|snapshot| deserialize(scope, &snapshot));
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    result
}

deno_core::extension!(
    turbo_surf_crypto_clone,
    ops = [
        op_install_crypto_clone_adapter,
        op_surf_is_window,
        op_surf_structured_clone
    ],
);

pub(crate) fn extension() -> Extension {
    turbo_surf_crypto_clone::init()
}

pub(crate) fn clear_context(context: v8::Local<v8::Context>) {
    context.remove_slot::<KeyAdapter>();
    context.remove_slot::<crate::geometry_backend::GeometryCloneAdapter>();
}
