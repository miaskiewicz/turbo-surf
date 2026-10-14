//! V8 graph cloning with private native key handles carried out of band.
use deno_core::v8::{ValueDeserializerHelper, ValueSerializerHelper, ValueSerializerImpl};
use deno_core::{op2, v8, Extension};
use deno_error::JsErrorBox;
use std::{cell::RefCell, rc::Rc};

struct KeyAdapter {
    key: v8::Global<v8::Function>,
    error: v8::Global<v8::Function>,
}

#[op2(fast)]
fn op_install_crypto_clone_adapter(
    scope: &mut v8::PinScope,
    adapter: v8::Local<v8::Function>,
    error: v8::Local<v8::Function>,
) {
    if let Some(context) = adapter.get_creation_context(scope) {
        context.set_slot(Rc::new(KeyAdapter {
            key: v8::Global::new(scope, adapter),
            error: v8::Global::new(scope, error),
        }));
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
    objects: RefCell<Vec<NativeSnapshot>>,
    transfers: RefCell<Vec<v8::SharedRef<v8::BackingStore>>>,
    shared: RefCell<Vec<v8::SharedRef<v8::BackingStore>>>,
    wasm: RefCell<Vec<v8::CompiledWasmModule>>,
    ports: RefCell<Vec<crate::message_ports::PortTransfer>>,
    port_objects: RefCell<Vec<v8::Global<v8::Object>>>,
}

impl Drop for CloneHandles {
    fn drop(&mut self) {
        crate::message_ports::discard_transfers(std::mem::take(self.ports.get_mut()));
    }
}

struct Delegate(Rc<CloneHandles>);

enum NativeSnapshot {
    Key(Box<deno_crypto::NativeKeySnapshot>),
    HybridKey(crate::crypto_hybrid::HybridKeySnapshot),
    Port(usize),
    Exception { name: String, message: String },
}

impl NativeSnapshot {
    fn size_bytes(&self) -> usize {
        match self {
            Self::Key(key) => key.size_bytes(),
            Self::HybridKey(key) => key.size_bytes(),
            Self::Exception { name, message } => name.len().saturating_add(message.len()),
            Self::Port(_) => 0,
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

impl v8::ValueSerializerImpl for Delegate {
    fn throw_data_clone_error<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i>,
        message: v8::Local<'s, v8::String>,
    ) {
        if let Some(adapter) = scope.get_current_context().get_slot::<KeyAdapter>() {
            let factory = v8::Local::new(scope, &adapter.error);
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
                    .is_some_and(|record| record.is_object()),
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
            let Some(index) = ports
                .iter()
                .position(|port| crate::message_ports::transfer_matches(scope, object, port))
            else {
                let message = v8::String::new(scope, "MessagePort must occur in transfer list")?;
                self.throw_data_clone_error(scope, message);
                return None;
            };
            NativeSnapshot::Port(index)
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
        } else if let Some(snapshot) = exception_snapshot(scope, object) {
            snapshot
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
        _: &mut v8::PinScope<'s, 'i>,
        buffer: v8::Local<'s, v8::SharedArrayBuffer>,
    ) -> Option<u32> {
        let mut stores = self.0.shared.borrow_mut();
        let index = u32::try_from(stores.len()).ok()?;
        stores.push(buffer.get_backing_store());
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
        let adapter = scope.get_current_context().get_slot::<KeyAdapter>()?;
        if let NativeSnapshot::Exception { name, message } = snapshot {
            let factory = v8::Local::new(scope, &adapter.error);
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
        let adapter = v8::Local::new(scope, &adapter.key);
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
        let store = self.0.shared.borrow().get(index as usize)?.clone();
        Some(v8::SharedArrayBuffer::with_backing_store(scope, &store))
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

pub(crate) fn data_clone_error(scope: &mut v8::PinScope, message: &'static str) -> JsErrorBox {
    if let Some(message) = v8::String::new(scope, message) {
        Delegate(Rc::default()).throw_data_clone_error(scope, message);
    }
    JsErrorBox::new("DOMExceptionDataCloneError", message)
}

fn validate_transfers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    transfer: Option<v8::Local<'s, v8::Value>>,
) -> Result<
    (
        Vec<v8::Local<'s, v8::ArrayBuffer>>,
        Vec<crate::message_ports::PortTransfer>,
    ),
    JsErrorBox,
> {
    let Some(transfer) = transfer.filter(|value| !value.is_undefined()) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let array = v8::Local::<v8::Array>::try_from(transfer)
        .map_err(|_| JsErrorBox::type_error("Transfer sequence required"))?;
    if array.length() > 4096 {
        return Err(JsErrorBox::range_error(
            "Native transfer sequence budget exceeded",
        ));
    }
    let mut buffers = Vec::new();
    let mut ports = Vec::new();
    for index in 0..array.length() {
        if scope.is_execution_terminating() {
            return Err(JsErrorBox::generic("Clone interrupted"));
        }
        let value = array
            .get_index(scope, index)
            .ok_or_else(|| JsErrorBox::generic("Transfer sequence interrupted"))?;
        if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
            if crate::message_ports::token(scope, object).is_some() {
                let port = crate::message_ports::validate_transfer(scope, object)?;
                if ports
                    .iter()
                    .any(|existing| crate::message_ports::same_transfer(existing, &port))
                {
                    return Err(data_clone_error(
                        scope,
                        "MessagePort occurs twice in transfer list",
                    ));
                }
                ports.push(port);
                continue;
            }
        }
        let buffer = v8::Local::<v8::ArrayBuffer>::try_from(value)
            .map_err(|_| data_clone_error(scope, "Value is not transferable"))?;
        if buffers.contains(&buffer) {
            return Err(data_clone_error(
                scope,
                "ArrayBuffer occurs twice in transfer list",
            ));
        }
        if buffer.was_detached() || !buffer.is_detachable() {
            return Err(data_clone_error(scope, "ArrayBuffer cannot be transferred"));
        }
        buffers.push(buffer);
    }
    Ok((buffers, ports))
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
        // Admission happens after getters. No guest code runs between the final
        // ownership check and the native detach/endpoint generation changes.
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
        for buffer in &self.buffers {
            if buffer.detach(None) != Some(true) {
                return Err(data_clone_error(scope, "ArrayBuffer cannot be transferred"));
            }
        }
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
    let (buffers, ports) = validate_transfers(scope, transfer)?;
    if sender.is_some_and(|sender| {
        ports
            .iter()
            .any(|port| crate::message_ports::transfer_matches(scope, sender, port))
    }) {
        return Err(data_clone_error(
            scope,
            "Source MessagePort cannot be transferred to itself",
        ));
    }
    let context = scope.get_current_context();
    let handles = Rc::new(CloneHandles::default());
    handles.ports.borrow_mut().extend_from_slice(&ports);
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
pub(crate) fn serialize_transfer<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    transfer: Option<v8::Local<'s, v8::Value>>,
    sender: Option<v8::Local<v8::Object>>,
) -> Result<CloneSnapshot, JsErrorBox> {
    prepare_transfer(scope, value, transfer, sender)?.commit(scope)
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
        .map(|value| (value, ports))
        .ok_or_else(failed)
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
    let result = serialize_transfer(scope, value, transfer, None)
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
