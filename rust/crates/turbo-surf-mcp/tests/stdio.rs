//! The stdio server binary: newline-delimited JSON-RPC 2.0 in, one response line per
//! request out; blank and malformed lines and notifications produce no output.
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn stdio_server_answers_requests_and_skips_blank_malformed_and_notifications() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_turbo-surf-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        "",
        "   ",
        "{not json",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":"two","method":"tools/call","params":{"name":"set_mode","arguments":{"mode":"js"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"web_search_set_engine"}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"nope"}"#,
    ]
    .join("\n");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{input}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let lines: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "turbo-surf");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        lines[1],
        json!({"jsonrpc":"2.0","id":"two","result":{"content":[{"type":"text","text":"{\"mode\":\"js\"}"}]}})
    );
    assert_eq!(
        lines[2],
        json!({"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"web_search_set_engine: missing 'engine'"}})
    );
    assert_eq!(
        lines[3],
        json!({"jsonrpc":"2.0","id":4,"error":{"code":-32000,"message":"tools/call: missing 'name'"}})
    );
    assert_eq!(
        lines[4],
        json!({"jsonrpc":"2.0","id":5,"error":{"code":-32000,"message":"unknown method: nope"}})
    );
}
