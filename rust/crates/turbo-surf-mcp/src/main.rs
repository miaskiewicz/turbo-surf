//! turbo-surf MCP server binary — newline-delimited JSON-RPC 2.0 over stdio.
//! No Node: one native process reads requests from stdin, writes responses to
//! stdout. The logic lives in the library (`turbo_surf_mcp`).

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use turbo_surf_mcp::{handle, parse_error, Session};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut session = Session::new();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        // A line that is not JSON answers -32700 with `id: null` (JSON-RPC 2.0).
        let resp = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(req) => handle(&mut session, &req).await,
            Err(_) => Some(parse_error()),
        };
        if let Some(resp) = resp {
            let body = serde_json::to_string(&resp).unwrap_or_default();
            let _ = stdout.write_all(body.as_bytes()).await;
            let _ = stdout.write_all(b"\n").await;
            let _ = stdout.flush().await;
        }
    }
}
