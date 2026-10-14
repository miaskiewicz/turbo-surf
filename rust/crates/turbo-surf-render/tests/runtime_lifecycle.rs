//! Private weak records are cleared before every native isolate disposal path.
use turbo_surf_render::{eval_async, render_html, run_with_dom, PageSession};

const RANGES: &str = r#"
  const frame=document.createElement('iframe');document.body.append(frame);
  const parentRange=document.createRange(),childRange=frame.contentDocument.createRange();
  parentRange.selectNodeContents(document.body);childRange.selectNodeContents(frame.contentDocument.body);
  frame.remove();globalThis.__RESULT='ready';
"#;

#[tokio::test(flavor = "current_thread")]
async fn async_and_live_sessions_dispose_ranges_after_success_and_failure() {
    for _ in 0..3 {
        assert_eq!(
            eval_async("<body></body>", "https://lifetime.test/", RANGES)
                .await
                .unwrap(),
            "ready"
        );
        let failing = format!("{RANGES}throw new Error('authored teardown sentinel');");
        assert!(
            eval_async("<body></body>", "https://lifetime.test/", &failing)
                .await
                .unwrap_err()
                .contains("authored teardown sentinel")
        );
        for explicit_close in [false, true] {
            let mut page =
                PageSession::open("<body></body>", "https://lifetime.test/", "", "", 5000)
                    .await
                    .unwrap();
            assert_eq!(page.eval(RANGES).await.unwrap(), "ready");
            if explicit_close {
                page.close();
            } else {
                drop(page);
            }
        }
    }
}

#[test]
fn synchronous_and_thread_local_runtimes_dispose_ranges_before_thread_exit() {
    std::thread::spawn(|| {
        render_html("<body></body>", RANGES).unwrap();
        run_with_dom("<body></body>", &format!("{RANGES}'ready'")).unwrap();
        // Replacing the cached document disposes the previous isolate too.
        run_with_dom(
            "<body><p>replacement</p></body>",
            "document.createRange();'ready'",
        )
        .unwrap();
    })
    .join()
    .expect("native runtime teardown must not panic");
}

#[test]
fn cached_runtime_teardown_handles_binding_tls_destroyed_first() {
    std::thread::spawn(|| {
        // Initialize the cached runtime before the DOM's TLS cells, reversing
        // their destructor order compared with an earlier standalone render.
        run_with_dom("<body></body>", "document.createRange();'ready'").unwrap();
    })
    .join()
    .expect("cached runtime cleanup must tolerate binding TLS destruction");
}
