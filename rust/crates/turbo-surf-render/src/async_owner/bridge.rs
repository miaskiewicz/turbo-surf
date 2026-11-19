//! Captured infrastructure async-context helpers; document entry is authenticated by the caller.
use deno_core::v8;

pub(super) enum OwnerEntryFailure {
    Interrupted,
    Authority,
}

pub struct OwnerAsyncContext {
    context: v8::Weak<v8::Context>,
    enter: v8::Weak<v8::Function>,
    read: v8::Weak<v8::Function>,
    restore: v8::Weak<v8::Function>,
}

const FACTORY: &str = r#"(function(core) {
  'use strict';
  const variable = new core.AsyncVariable();
  const apply = Reflect.apply;
  const enter = core.AsyncVariable.prototype.enter;
  const read = core.AsyncVariable.prototype.get;
  const restore = core.setAsyncContext;
  return Object.freeze({
    enter(token) { return apply(enter, variable, [token]); },
    read() { return apply(read, variable, []); },
    restore(previous) { return restore(previous); },
  });
})"#;

impl OwnerAsyncContext {
    /// Called exactly once in the trusted private service realm, before guests.
    /// E's global owns the helper; native storage contains weak functions only.
    pub fn initialize<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        infrastructure: v8::Local<'s, v8::Context>,
        trusted_core: v8::Local<'s, v8::Object>,
    ) -> Result<Self, &'static str> {
        let scope = &mut v8::ContextScope::new(scope, infrastructure);
        let source = v8::String::new(scope, FACTORY).ok_or("Owner factory allocation failed")?;
        let factory = v8::Script::compile(scope, source, None)
            .and_then(|script| script.run(scope))
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
            .ok_or("Owner factory unavailable")?;
        let receiver = v8::undefined(scope);
        let helper = factory
            .call(scope, receiver.into(), &[trusted_core.into()])
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
            .ok_or("Owner helper initialization failed")?;
        // The key is never published. The private V8 edge owns the helper and
        // does not introduce a native Global<Function> or strong guest context.
        let key = v8::Private::new(scope, None);
        if infrastructure
            .global(scope)
            .set_private(scope, key, helper.into())
            != Some(true)
        {
            return Err("Owner helper anchor installation failed");
        }
        let enter = Self::method(scope, helper, "enter")?;
        let read = Self::method(scope, helper, "read")?;
        let restore = Self::method(scope, helper, "restore")?;
        Ok(Self {
            context: v8::Weak::new(scope, infrastructure),
            enter: v8::Weak::new(scope, enter),
            read: v8::Weak::new(scope, read),
            restore: v8::Weak::new(scope, restore),
        })
    }

    fn method<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        helper: v8::Local<'s, v8::Object>,
        name: &str,
    ) -> Result<v8::Local<'s, v8::Function>, &'static str> {
        let name = v8::String::new(scope, name).ok_or("Owner method allocation failed")?;
        helper
            .get(scope, name.into())
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
            .ok_or("Owner helper method unavailable")
    }

    /// Reads only the private variable, never guest global properties.
    /// Undefined is UNKNOWN and must never be converted to the selected Root.
    pub fn current<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> Result<v8::Local<'s, v8::Value>, &'static str> {
        let context = self
            .context
            .to_local(scope)
            .ok_or("Owner service realm closed")?;
        let read = self
            .read
            .to_local(scope)
            .ok_or("Owner getter unavailable")?;
        let scope = &mut v8::ContextScope::new(scope, context);
        let receiver = v8::undefined(scope);
        read.call(scope, receiver.into(), &[])
            .ok_or("Owner getter failed")
    }

    /// The caller authenticates the token through WeakOwners. Document-task
    /// entry requires a live owner; the private retained-stream path reuses an
    /// existing identity without readmitting retired document tasks.
    /// A JS exception is represented by the caller's R (usually Option/Result).
    /// No graph/registry RefCell borrow may be held across this reentrant call.
    pub fn with_owner<'s, R>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        authenticated_token: v8::Local<'s, v8::Value>,
        run: impl FnOnce(&mut v8::PinScope<'s, '_>) -> R,
    ) -> Result<R, OwnerEntryFailure> {
        let previous = scope.get_continuation_preserved_embedder_data();
        let entered = self.enter_owner(scope, authenticated_token);
        if let Err(failure) = entered {
            scope.set_continuation_preserved_embedder_data(previous);
            return Err(failure);
        }
        let result = run(scope);
        // Native restore remains available even with a pending guest exception
        // or terminating execution; do not call JS while such state is pending.
        scope.set_continuation_preserved_embedder_data(previous);
        Ok(result)
    }

    fn enter_owner<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        token: v8::Local<'s, v8::Value>,
    ) -> Result<(), OwnerEntryFailure> {
        let context = self
            .context
            .to_local(scope)
            .ok_or(OwnerEntryFailure::Authority)?;
        let enter = self
            .enter
            .to_local(scope)
            .ok_or(OwnerEntryFailure::Authority)?;
        // Check the complete captured capability set, including restore. The
        // normal restoration path uses the native setter to preserve exceptions.
        self.restore
            .to_local(scope)
            .ok_or(OwnerEntryFailure::Authority)?;
        let scope = &mut v8::ContextScope::new(scope, context);
        let receiver = v8::undefined(scope);
        if enter.call(scope, receiver.into(), &[token]).is_none() {
            return Err(if scope.is_execution_terminating() {
                OwnerEntryFailure::Interrupted
            } else {
                OwnerEntryFailure::Authority
            });
        }
        Ok(())
    }
}
