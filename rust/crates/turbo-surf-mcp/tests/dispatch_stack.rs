//! Cold MCP initialization must fit an ordinary Linux-sized host thread.
use serde_json::json;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use turbo_surf_mcp::{call_tool, Session};

const CHILD: &str = "TURBO_SURF_MCP_STACK_CHILD";
const TEST: &str = "cold_dispatch_initializes_on_a_standard_two_mib_thread";
const HTML: &str = "<body><p id=p>before</p><script>document.getElementById('p').textContent='after'</script></body>";

async fn dispatch_sequence() {
    let mut session = Session::new();
    session.load("https://stack.test/", HTML);
    let args = json!({"script":"document.getElementById('p').textContent"});
    eprintln!("mcp-stack-stage:evaluate");
    let evaluation = call_tool(&mut session, "evaluate", &args);
    eprintln!(
        "mcp-stack-dispatch-future-bytes:{}",
        std::mem::size_of_val(&evaluation)
    );
    assert_eq!(evaluation.await.unwrap(), json!("before"));
    eprintln!("mcp-stack-stage:render");
    assert_eq!(
        call_tool(&mut session, "render", &json!({})).await.unwrap(),
        json!({"ok":true})
    );
    assert_eq!(
        call_tool(&mut session, "text_content", &json!({"selector":"#p"}))
            .await
            .unwrap(),
        json!("after")
    );
    eprintln!("mcp-stack-stage:inject");
    call_tool(
        &mut session,
        "render",
        &json!({"script":"document.getElementById('p').textContent='again'"}),
    )
    .await
    .unwrap();
    assert_eq!(
        call_tool(&mut session, "evaluate", &args).await.unwrap(),
        json!("again")
    );
}

fn child_case(mode: &str) -> std::process::Output {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env(CHILD, mode)
        .env("RUST_BACKTRACE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "{mode} child exceeded its fixture deadline: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn cold_dispatch_initializes_on_a_standard_two_mib_thread() {
    if let Ok(mode) = std::env::var(CHILD) {
        std::thread::Builder::new()
            .name("mcp-ci-stack".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    if mode == "direct" {
                        eprintln!("mcp-stack-stage:direct-evaluate");
                        assert_eq!(
                            turbo_surf_render::run_with_dom(
                                HTML,
                                "document.getElementById('p').textContent"
                            )
                            .unwrap(),
                            "before"
                        );
                    } else {
                        assert_eq!(mode, "dispatch");
                        dispatch_sequence().await;
                    }
                });
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    for mode in ["direct", "dispatch"] {
        let output = child_case(mode);
        assert!(
            output.status.success(),
            "{mode} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
