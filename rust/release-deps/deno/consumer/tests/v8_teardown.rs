use std::sync::{Mutex, MutexGuard, Once};

// A separate test binary avoids sharing V8 initialization with JsRuntime tests.
fn setup() -> MutexGuard<'static, ()> {
    static START: Once = Once::new();
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap();
    START.call_once(|| {
        v8::V8::initialize_platform(v8::new_default_platform(0, false).make_shared());
        v8::V8::initialize();
    });
    guard
}

#[test]
fn isolate_slot_drop_can_access_annex_during_teardown() {
    let _setup_guard = setup();

    struct DropCheck {
        isolate: v8::UnsafeRawIsolatePtr,
    }

    impl Drop for DropCheck {
        fn drop(&mut self) {
            let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr(&self.isolate) };
            let handle = isolate.thread_safe_handle();
            assert!(
                handle.cancel_terminate_execution(),
                "slot Drop must retain a live IsolateHandle until weak owners reset"
            );
        }
    }

    {
        let isolate = &mut v8::Isolate::new(Default::default());
        let isolate_ptr = unsafe { isolate.as_raw_isolate_ptr() };
        isolate.set_slot(DropCheck {
            isolate: isolate_ptr,
        });
    }
}

#[test]
fn weak_handles_owned_by_slots_retire_before_native_disposal() {
    use std::cell::Cell;
    use std::rc::Rc;

    let _setup_guard = setup();

    struct WeakSlot {
        weak: Option<v8::Weak<v8::Object>>,
        handle: v8::IsolateHandle,
        dropped: Rc<Cell<bool>>,
    }

    impl Drop for WeakSlot {
        fn drop(&mut self) {
            assert!(self.handle.cancel_terminate_execution());
            drop(self.weak.take());
            self.dropped.set(true);
        }
    }

    for snapshot in [false, true] {
        let dropped = Rc::new(Cell::new(false));
        let finalized = Rc::new(Cell::new(false));
        let mut isolate = if snapshot {
            v8::Isolate::snapshot_creator(None, None)
        } else {
            v8::Isolate::new(Default::default())
        };
        let surviving_handle = isolate.thread_safe_handle();
        {
            v8::scope!(let scope, &mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            if snapshot {
                scope.set_default_context(context);
            }
            let object = v8::Object::new(scope);
            let called = finalized.clone();
            let weak = v8::Weak::with_finalizer(
                scope,
                object,
                Box::new(move |_| {
                    called.set(true);
                }),
            );
            scope.set_slot(WeakSlot {
                weak: Some(weak),
                handle: surviving_handle.clone(),
                dropped: dropped.clone(),
            });
        }
        if snapshot {
            let blob = isolate
                .create_blob(v8::FunctionCodeHandling::Clear)
                .unwrap();
            assert!(!blob.is_empty());
        } else {
            drop(isolate);
        }
        assert!(dropped.get());
        assert!(
            !finalized.get(),
            "dropped Weak must unregister its callback"
        );
        assert!(!surviving_handle.cancel_terminate_execution());
    }
}
