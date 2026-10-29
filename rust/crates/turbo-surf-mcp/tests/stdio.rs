//! The stdio server binary: newline-delimited JSON-RPC 2.0 in, one response line per
//! request out; blank lines and notifications produce no output. Errors follow
//! JSON-RPC 2.0 and MCP 2024-11-05: a failing tool is a result with `isError: true`,
//! protocol failures use the standard codes, and `ping` answers `{}`.
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn stdio_server_answers_requests_with_jsonrpc_and_mcp_error_shapes() {
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
        r#"{"jsonrpc":"2.0","id":6,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"no_such_tool"}}"#,
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"set_mode","arguments":[1]}}"#,
        r#"[1]"#,
        r#"{"jsonrpc":"2.0","id":9}"#,
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
    assert_eq!(lines.len(), 11, "{lines:?}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "turbo-surf");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        lines[1],
        json!({"jsonrpc":"2.0","id":"two","result":{"content":[{"type":"text","text":"{\"mode\":\"js\"}"}]}})
    );
    assert_eq!(
        lines[2],
        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})
    );
    assert_eq!(
        lines[3],
        json!({"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"web_search_set_engine: missing 'engine'"}],"isError":true}})
    );
    assert_eq!(
        lines[4],
        json!({"jsonrpc":"2.0","id":4,"error":{"code":-32602,"message":"tools/call: missing 'name'"}})
    );
    assert_eq!(
        lines[5],
        json!({"jsonrpc":"2.0","id":5,"error":{"code":-32601,"message":"Method not found: nope"}})
    );
    assert_eq!(lines[6], json!({"jsonrpc":"2.0","id":6,"result":{}}));
    assert_eq!(
        lines[7],
        json!({"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"unknown tool: no_such_tool"}})
    );
    assert_eq!(
        lines[8],
        json!({"jsonrpc":"2.0","id":8,"error":{"code":-32602,"message":"tools/call: 'arguments' must be an object"}})
    );
    assert_eq!(
        lines[9],
        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}})
    );
    assert_eq!(
        lines[10],
        json!({"jsonrpc":"2.0","id":9,"error":{"code":-32600,"message":"Invalid Request"}})
    );
}
