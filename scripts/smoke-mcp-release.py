#!/usr/bin/env python3
"""Offline artifact smoke: initialize, load loopback HTML, execute native page JS."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import threading
import time

STOP = None
CHILD = None
HTML = b'<!doctype html><html><body><p id="release-probe">portable-runtime</p></body></html>'
EXPECTED = {"text": "portable-runtime", "sum": 42, "crypto": True, "streams": True, "private": True}
SCRIPT = """(() => {
  const bytes = new Uint8Array(8);
  const stream = new ReadableStream({start(controller) { controller.close(); }});
  return JSON.stringify({text: document.getElementById('release-probe').textContent,
    sum: 20 + 22, crypto: crypto.getRandomValues(bytes) === bytes && bytes.length === 8,
    streams: stream instanceof ReadableStream && !stream.locked, private: typeof Deno === 'undefined'});
})()"""


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def clean_environment():
    exact = {"HYPER_API_KEY", "SCRAPFLY_API_KEY", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"}
    prefixes = ("TURBO_SURF_", "NODE_", "NPM_", "DENO_", "V8_", "RUSTY_V8_", "DYLD_", "LD_", "PYTHON")
    removed = sorted(key for key in os.environ if key.upper() in exact or key.startswith(prefixes) or key.endswith("_TOKEN"))
    return {key: value for key, value in os.environ.items() if key not in removed}, removed


def interrupted(number, _frame):
    global STOP
    STOP = number
    if CHILD is not None and CHILD.poll() is None:
        try:
            if os.name == "nt":
                CHILD.send_signal(signal.CTRL_BREAK_EVENT)
            else:
                os.killpg(CHILD.pid, signal.SIGINT)
        except (ProcessLookupError, OSError):
            pass


def group_alive(child):
    # Reap an exited leader before probing its remaining owned group.
    child.poll()
    if os.name == "nt":
        return child.poll() is None
    try:
        os.killpg(child.pid, 0)
        return True
    except ProcessLookupError:
        return False


def retire(child):
    if os.name == "nt":
        if child.poll() is None:
            result = subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"], capture_output=True, timeout=10)
            if result.returncode != 0 and child.poll() is None:
                child.kill()
        child.wait(timeout=10)
        return
    for number, grace in [(signal.SIGINT, 2), (signal.SIGTERM, 2), (signal.SIGKILL, 2)]:
        if not group_alive(child):
            break
        try:
            os.killpg(child.pid, number)
        except ProcessLookupError:
            break
        deadline = time.monotonic() + grace
        while group_alive(child) and time.monotonic() < deadline:
            child.poll()
            time.sleep(0.02)
    child.wait(timeout=10)
    if group_alive(child):
        raise RuntimeError("Owned smoke process group did not close")


def verify_protocol(stdout, version, url):
    rows = [json.loads(line) for line in stdout.splitlines() if line.strip()]
    if len(rows) != 3 or [row.get("id") for row in rows] != [1, 2, 3]:
        raise ValueError("Expected exactly initialize, goto and evaluate responses")
    if any(row.get("jsonrpc") != "2.0" or "error" in row for row in rows):
        raise ValueError("Artifact returned a JSON-RPC error or invalid envelope")
    if rows[0].get("result", {}).get("serverInfo") != {"name": "turbo-surf", "version": version}:
        raise ValueError("Artifact initialize version differs")
    def tool_value(row):
        result = row.get("result", {})
        content = result.get("content", [])
        if result.get("isError") or len(content) != 1 or content[0].get("type") != "text":
            raise ValueError("Expected a successful MCP text tool result")
        return json.loads(content[0]["text"])
    page = tool_value(rows[1])
    if page.get("status") != 200 or page.get("url") != url:
        raise ValueError("Artifact did not load the actual offline fixture")
    if tool_value(rows[2]) != EXPECTED:
        raise ValueError("Artifact did not execute expected DOM/native crypto/stream JS")


def smoke(command, version, source_commit, work, output, timeout=60):
    global CHILD
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) or not re.fullmatch(r"[0-9a-f]{40}", source_commit):
        raise ValueError("Expected exact release version and source commit")
    work, output = Path(work).absolute(), Path(output).absolute()
    if any(path != path.resolve() or path.exists() for path in (work, output)):
        raise ValueError("Use fresh unlinked private work and output directories")
    if work.is_relative_to(output) or output.is_relative_to(work):
        raise ValueError("Keep artifact evidence separate from smoke cwd")
    if any((parent / ".env").exists() for parent in (work, *work.parents)):
        raise ValueError("Private smoke cwd must have no ancestor .env")
    work.mkdir(mode=0o700)
    output.mkdir(mode=0o700)
    command = [str(Path(value).absolute()) if index == 0 else str(value) for index, value in enumerate(command)]
    if not command or not Path(command[0]).is_file():
        raise ValueError("Expected an actual executable command")
    command_hashes = {str(Path(value).resolve()): sha(Path(value)) for value in command if Path(value).is_file()}
    events, lock = [], threading.Lock()
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            with lock:
                events.append({"method": "GET", "path": self.path})
            if self.path != "/portable":
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(HTML)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(HTML)
        def log_message(self, *_arguments):
            pass
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = "http://127.0.0.1:" + str(server.server_address[1]) + "/portable"
    env, removed = clean_environment()
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "artifact-javascript-smoke", "version": "1"}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "goto", "arguments": {"url": url}}},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "evaluate", "arguments": {"script": SCRIPT}}},
    ]
    receipt = {"initializeVerified": False, "javascriptVerified": False, "sourceCommit": source_commit, "expectedVersion": version, "command": command, "cwd": str(work), "timeoutSeconds": timeout, "timedOut": False, "exitCode": None, "clearedEnvironmentNames": removed, "commandFileSha256": command_hashes}
    started = time.monotonic()
    try:
        if STOP is not None:
            raise InterruptedError("Smoke was interrupted before launch")
        CHILD = subprocess.Popen(command, cwd=work, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=os.name != "nt", creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0)
        if STOP is not None:
            retire(CHILD)
        try:
            stdout, stderr = CHILD.communicate("".join(json.dumps(row) + "\n" for row in requests), timeout=timeout)
        except subprocess.TimeoutExpired:
            receipt["timedOut"] = True
            retire(CHILD)
            stdout, stderr = CHILD.communicate(timeout=10)
        (output / "stdout.log").write_text(stdout)
        (output / "stderr.log").write_text(stderr)
        receipt["exitCode"] = CHILD.returncode
        lingering = group_alive(CHILD)
        receipt["lingeringOwnedGroupAfterLeaderExit"] = lingering
        if lingering:
            retire(CHILD)
        if receipt["timedOut"] or receipt["exitCode"] != 0 or STOP is not None or lingering:
            raise RuntimeError("Artifact process failed, timed out, was interrupted, or left owned descendants")
        verify_protocol(stdout, version, url)
        with lock:
            if events != [{"method": "GET", "path": "/portable"}]:
                raise ValueError("Expected exactly one actual loopback fixture request")
        if any(not Path(path).is_file() or sha(Path(path)) != digest for path, digest in command_hashes.items()):
            raise ValueError("Actual smoke command files changed")
        receipt.update(initializeVerified=True, javascriptVerified=True, artifactUnchanged=True, verifiedRequests=3, offlineFixtureRequests=1, javascriptResult=EXPECTED)
    except BaseException as error:
        receipt["error"] = {"type": type(error).__name__, "message": str(error)}
    finally:
        try:
            if CHILD is not None and group_alive(CHILD):
                retire(CHILD)
        except BaseException as error:
            receipt["initializeVerified"] = False
            receipt["javascriptVerified"] = False
            receipt["cleanupError"] = {"type": type(error).__name__, "message": str(error)}
        if CHILD is not None:
            for stream in (CHILD.stdin, CHILD.stdout, CHILD.stderr):
                if stream is not None:
                    stream.close()
        CHILD = None
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        receipt.update(elapsedSeconds=time.monotonic() - started, interruptedSignal=STOP)
        (output / "initialize.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--node", type=Path, help="Run the binary argument as an actual packaged Node cli.js")
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    for name in ("SIGINT", "SIGTERM", "SIGHUP"):
        if hasattr(signal, name):
            signal.signal(getattr(signal, name), interrupted)
    binary = args.binary.absolute()
    if not binary.is_file():
        raise ValueError("Missing actual artifact")
    before = sha(binary)
    command = [binary] if args.node is None else [args.node.absolute(), binary]
    receipt = smoke(command, args.expected_version, args.source_commit, args.work, args.output)
    after = sha(binary)
    receipt.update(binary=str(binary), sha256=before, artifactUnchanged=after == before)
    if after != before:
        receipt.update(initializeVerified=False, javascriptVerified=False, artifactChanged=True)
    (args.output / "initialize.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return 128 + STOP if STOP is not None else (0 if receipt["initializeVerified"] and receipt["javascriptVerified"] else 1)


if __name__ == "__main__":
    sys.exit(main())
