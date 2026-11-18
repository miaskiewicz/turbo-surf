//! Real navigation source deadlines over localhost module loading.
use super::*;

struct ModuleDeadlineServer {
    origin: String,
    requests: tokio::sync::mpsc::UnboundedReceiver<String>,
    task: tokio::task::JoinHandle<()>,
}

impl ModuleDeadlineServer {
    async fn start() -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (send, requests) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    assert!(bytes.len() <= 8192);
                }
                let request = String::from_utf8(bytes).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                send.send(path.to_owned()).unwrap();
                let (mime, body) = match path {
                    "/pending" => ("text/html", "<iframe id=descendant srcdoc='<body>descendant'></iframe><script type=module src='/pending.mjs'></script>"),
                    "/pending.mjs" => ("application/javascript", "globalThis.deadlineModuleStarted=true;await new Promise(resolve=>{globalThis.releaseDeadlineModule=resolve});globalThis.deadlineModuleFinished=true"),
                    _ => ("text/html", "<body>fresh source"),
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }
}

impl Drop for ModuleDeadlineServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn native_module_started(page: &mut PageSession, target: u64) -> bool {
    deno_core::scope!(scope, &mut page.rt);
    let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
    let scope = &mut v8::ContextScope::new(scope, root);
    let key = v8::String::new(scope, "deadlineModuleStarted").unwrap();
    let Some(descriptor) = root
        .global(scope)
        .get_own_property_descriptor(scope, key.into())
    else {
        return false;
    };
    let Ok(descriptor) = v8::Local::<v8::Object>::try_from(descriptor) else {
        return false;
    };
    let value = v8::String::new(scope, "value").unwrap();
    descriptor
        .get(scope, value.into())
        .is_some_and(|value| value.is_true())
}

async fn pending_module_source(
    server: &mut ModuleDeadlineServer,
) -> (PageSession, u64, Arc<Mutex<Clock>>) {
    pending_module_source_with_options(server, None).await
}

async fn pending_module_source_with_options(
    server: &mut ModuleDeadlineServer,
    options: Option<PageSessionOptions>,
) -> (PageSession, u64, Arc<Mutex<Clock>>) {
    let mut page = match options {
        None => PageSession::open("<body>seed", &server.origin, "", "", 5000).await,
        Some(options) => {
            PageSession::open_with_document_response_and_options(
                "<body>seed",
                &server.origin,
                "",
                "",
                5000,
                "",
                ClientHints::default(),
                &[],
                BrowserStorage::default(),
                crate::DocumentResponseMetadata::default(),
                None,
                options,
            )
            .await
        }
    }
    .unwrap();
    let target = page.replaced_root_target().unwrap();
    page.navigate_target_with_options(
        target,
        &format!("{}/pending", server.origin),
        "push",
        r#"{"waitUntil":"load","timeoutMs":5000}"#,
    )
    .await
    .unwrap();
    let activation = page.rt.activation_key();
    let _activation = activation.enter();
    let clock = {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let source = root.get_slot::<SourceLease>().unwrap();
        assert_eq!(source.goal, ExecutionGoal::Complete);
        assert!(!source.settled.load(Ordering::Acquire));
        let shared = JsRuntime::op_state_from(scope);
        assert_eq!(
            shared
                .borrow()
                .borrow::<State>()
                .loading
                .get(&target)
                .unwrap()
                .generation,
            source.generation
        );
        assert!(root.get_slot::<NativePageModuleContext>().is_some());
        source.clock.as_ref().unwrap().clone()
    };
    let deadline = {
        let clock = clock.lock().unwrap();
        tokio::time::Instant::from_std(clock.started + clock.budget)
    };
    tokio::time::timeout_at(
        deadline,
        std::future::poll_fn(|cx| {
            if let Poll::Ready(Err(error)) = page.rt.poll_event_loop(cx, Default::default()) {
                panic!("native module setup failed: {error}");
            }
            if native_module_started(&mut page, target) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .expect("actual module must start before its source deadline");
    assert_eq!(server.requests.recv().await.as_deref(), Some("/pending"));
    assert_eq!(
        server.requests.recv().await.as_deref(),
        Some("/pending.mjs")
    );
    drop(_activation);
    (page, target, clock)
}

fn assert_current_loading_source(page: &mut PageSession, target: u64, clock: &Arc<Mutex<Clock>>) {
    {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let source = root.get_slot::<SourceLease>().unwrap();
        assert_eq!(source.target, target);
        assert!(same_document(scope, root, &source.document));
        assert!(Arc::ptr_eq(source.clock.as_ref().unwrap(), clock));
        let shared = JsRuntime::op_state_from(scope);
        assert_eq!(
            shared
                .borrow()
                .borrow::<State>()
                .loading
                .get(&target)
                .unwrap()
                .generation,
            source.generation
        );
    }
    assert!(native_module_started(page, target));
}

#[tokio::test(flavor = "current_thread")]
async fn expired_source_module_work_is_cancelled_before_same_target_navigation_recovers() {
    let mut server = ModuleDeadlineServer::start().await;
    let (mut page, target, clock) = pending_module_source(&mut server).await;
    let _activation = page.rt.activation_key().enter();
    {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let budget = attempt_budget(scope, root);
        assert!(Arc::ptr_eq(budget.clock.as_ref().unwrap(), &clock));
    }
    let descendant_host = auxiliary_pages::execute_target(
        &mut page.rt,
        AuxiliaryContextId(target),
        "document.getElementById('descendant')",
        false,
    )
    .unwrap();
    let descendant_token = {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let host = v8::Local::new(scope, &descendant_host)
            .to_object(scope)
            .unwrap();
        let token = new_frame_attempt(scope, root, host).unwrap();
        assert!(frame_attempt_active(scope, root, host, token).unwrap());
        v8::Global::new(scope, token)
    };
    reconcile_expired_source_modules(&mut page.rt, target).unwrap();
    assert_current_loading_source(&mut page, target, &clock);
    let deadline = {
        let clock = clock.lock().unwrap();
        tokio::time::Instant::from_std(clock.started + clock.budget)
    };
    assert!(
        tokio::time::timeout_at(deadline, std::future::pending::<()>())
            .await
            .is_err()
    );
    assert_current_loading_source(&mut page, target, &clock);
    {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let budget = attempt_budget(scope, root);
        assert!(budget.clock.is_none());
        assert!(budget.expiry.is_none());
        let host = v8::Local::new(scope, &descendant_host)
            .to_object(scope)
            .unwrap();
        let token = v8::Local::new(scope, &descendant_token);
        assert!(!frame_attempt_active(scope, root, host, token).unwrap());
        assert_eq!(
            check_frame_attempt(scope, root, host, token).unwrap_err(),
            "Frame load attempt is cancelled"
        );
        assert!(matches!(
            frame_execution_deadline(scope, root, host, token),
            Err(FrameStartError::Cancelled)
        ));
        let fresh = new_frame_attempt(scope, root, host).unwrap();
        assert!(frame_attempt_active(scope, root, host, fresh).unwrap());
        assert!(frame_execution_deadline(scope, root, host, fresh)
            .unwrap()
            .is_none());
    }
    reconcile_expired_source_modules(&mut page.rt, target).unwrap();
    {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        assert!(root
            .get_slot::<SourceLease>()
            .unwrap()
            .modules_cancelled
            .get());
    }
    reconcile_expired_source_modules(&mut page.rt, target).unwrap();
    page.navigate_target(target, &format!("{}/fresh", server.origin), "push")
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression("document.body.textContent")
            .await
            .unwrap(),
        "fresh source"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn completing_a_real_module_source_releases_its_descendant_budget() {
    let mut server = ModuleDeadlineServer::start().await;
    // The actor services real due tasks; it never advances a virtual child-load timer.
    let (mut page, target, clock) = pending_module_source_with_options(
        &mut server,
        Some(PageSessionOptions {
            window_timer_mode: WindowTimerMode::RealTime,
            ..Default::default()
        }),
    )
    .await;
    assert_eq!(
        page.rt.op_state().borrow().borrow::<TimerClock>().mode,
        WindowTimerMode::RealTime
    );
    let _activation = page.rt.activation_key().enter();
    auxiliary_pages::execute_target(
        &mut page.rt,
        AuxiliaryContextId(target),
        "releaseDeadlineModule()",
        false,
    )
    .unwrap();
    let deadline = {
        let clock = clock.lock().unwrap();
        tokio::time::Instant::from_std(clock.started + clock.budget)
    };
    tokio::time::timeout_at(deadline, async {
        while page.has_pending_auxiliary_work() {
            page.poll_root_navigation_work().await.unwrap();
        }
    })
    .await
    .expect("released module must finish inside its original source deadline");
    {
        deno_core::scope!(scope, &mut page.rt);
        let root = auxiliary_pages::context(scope, AuxiliaryContextId(target)).unwrap();
        let source = root.get_slot::<SourceLease>().unwrap();
        assert!(source.settled.load(Ordering::Acquire));
        assert!(!source.modules_cancelled.get());
        let budget = attempt_budget(scope, root);
        assert!(budget.clock.is_none());
        assert!(budget.expiry.is_none());
    }
    assert_eq!(
        page.eval_expression("String(deadlineModuleFinished)")
            .await
            .unwrap(),
        "true"
    );
}
