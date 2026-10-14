//! Enter a parked session isolate only during synchronous work or one future poll.
use deno_core::v8;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Clone, Copy)]
pub(super) struct ActivationKey(v8::UnsafeRawIsolatePtr);

impl ActivationKey {
    pub(super) fn new(isolate: &mut v8::Isolate) -> Self {
        Self(unsafe { isolate.as_raw_isolate_ptr() })
    }

    pub(super) fn enter(self) -> RuntimeActivation {
        // Store the native pointer, never the movable Rust OwnedIsolate wrapper.
        // Each caller borrows its runtime for the complete guarded operation.
        let isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(self.0) };
        unsafe { isolate.enter() };
        RuntimeActivation {
            isolate: self.0,
            native: Some(crate::browser_env::activate_isolate(&isolate)),
        }
    }

    pub(super) fn during<F: Future>(self, future: F) -> ActiveFuture<F> {
        ActiveFuture { key: self, future }
    }
}

pub(super) struct RuntimeActivation {
    isolate: v8::UnsafeRawIsolatePtr,
    native: Option<crate::browser_env::NativeDomScope>,
}

impl Drop for RuntimeActivation {
    fn drop(&mut self) {
        self.native.take();
        // All V8 scopes and the native activation have ended before restoring
        // the previous isolate; no guard remains live across a pending await.
        unsafe { v8::Isolate::from_raw_isolate_ptr(self.isolate).exit() };
    }
}

pub(super) struct ActiveFuture<F> {
    key: ActivationKey,
    future: F,
}

impl<F: Future> Future for ActiveFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // Project the pinned future without moving it. Call sites borrow the
        // runtime externally, so polling cannot destroy the guarded isolate.
        let this = unsafe { self.get_unchecked_mut() };
        let _activation = this.key.enter();
        unsafe { Pin::new_unchecked(&mut this.future) }.poll(context)
    }
}
