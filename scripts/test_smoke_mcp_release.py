#!/usr/bin/env python3
"""Protocol and owned-process controls; only fake executables + localhost, no engine."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("smoke_mcp_release", Path(__file__).with_name("smoke-mcp-release.py"))
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
VERSION = "0.5.4"
COMMIT = "a" * 40
URL = "http://127.0.0.1:12345/portable"


def replies():
    def tool(number, value):
        return {"jsonrpc": "2.0", "id": number, "result": {"content": [{"type": "text", "text": json.dumps(value)}]}}
    return [
        {"jsonrpc": "2.0", "id": 1, "result": {"serverInfo": {"name": "turbo-surf", "version": VERSION}}},
        tool(2, {"url": URL, "status": 200, "title": ""}),
        tool(3, SMOKE.EXPECTED),
    ]


def validate(rows):
    SMOKE.verify_protocol("".join(json.dumps(row) + "\n" for row in rows), VERSION, URL)


FAKE = r'''import json, os, subprocess, sys, time, urllib.request
mode = sys.argv[1] if len(sys.argv) > 1 else "good"
if mode == "wait":
    open(os.environ["SMOKE_TEST_MARKER"], "w").write(str(os.getpid()))
    time.sleep(60)
    sys.exit(1)
rows = [json.loads(line) for line in sys.stdin if line.strip()]
url = rows[1]["params"]["arguments"]["url"]
with urllib.request.urlopen(url, timeout=5) as response:
    assert response.status == 200 and b"portable-runtime" in response.read()
def tool(number, value):
    return {"jsonrpc": "2.0", "id": number, "result": {"content": [{"type": "text", "text": json.dumps(value)}]}}
result = {"text":"portable-runtime", "sum":42, "crypto":True, "streams":True, "private":True}
answers = [{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"turbo-surf","version":"0.5.4"}}}, tool(2,{"url":url,"status":200}), tool(3,result)]
if mode == "js-error":
    answers[2] = {"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"missing relocated source"}}
if mode == "lingering":
    child = subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    open(os.environ["SMOKE_TEST_MARKER"], "w").write(str(child.pid))
for answer in answers:
    print(json.dumps(answer), flush=True)
'''


class ProtocolTests(unittest.TestCase):
    def test_actual_three_request_contract(self):
        validate(replies())

    def test_initialize_alone_does_not_prove_javascript(self):
        with self.assertRaises(ValueError):
            validate(replies()[:1])

    def test_runtime_tool_error_is_failure(self):
        rows = replies()
        rows[2] = {"jsonrpc": "2.0", "id": 3, "error": {"code": -32000, "message": "missing source"}}
        with self.assertRaisesRegex(ValueError, "JSON-RPC error"):
            validate(rows)

    def test_wrong_dom_crypto_and_stream_results_are_rejected(self):
        for key in SMOKE.EXPECTED:
            with self.subTest(key=key):
                rows = replies()
                value = dict(SMOKE.EXPECTED)
                value[key] = False if isinstance(value[key], bool) else "wrong"
                rows[2]["result"]["content"][0]["text"] = json.dumps(value)
                with self.assertRaisesRegex(ValueError, "expected DOM"):
                    validate(rows)

    def test_wrong_document_status_or_url_is_rejected(self):
        for value in ({"url": URL, "status": 429}, {"url": "about:blank", "status": 200}):
            with self.subTest(value=value):
                rows = replies()
                rows[1]["result"]["content"][0]["text"] = json.dumps(value)
                with self.assertRaisesRegex(ValueError, "offline fixture"):
                    validate(rows)

    def test_wrong_version_duplicate_or_error_envelope_is_rejected(self):
        rows = replies()
        rows[0]["result"]["serverInfo"]["version"] = "0.0.0"
        with self.assertRaises(ValueError):
            validate(rows)
        rows = replies()
        rows[2]["id"] = 2
        with self.assertRaises(ValueError):
            validate(rows)
        rows = replies()
        rows[2]["result"]["isError"] = True
        with self.assertRaises(ValueError):
            validate(rows)

    def test_solver_engine_node_and_proxy_environment_is_cleared(self):
        names = ["TURBO_SURF_SOLVER", "TURBO_SURF_ENGINE", "NODE_OPTIONS", "V8_FLAGS", "GITHUB_TOKEN", "HTTPS_PROXY", "http_proxy", "HYPER_API_KEY"]
        with patch.dict(os.environ, {name: "not-a-real-secret" for name in names}):
            env, removed = SMOKE.clean_environment()
        self.assertTrue(set(names) <= set(removed))
        self.assertTrue(set(names).isdisjoint(env))


class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="turbo-surf-mcp-smoke-control-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.fake = self.root / "fake.py"
        self.fake.write_text(FAKE)
        self.marker = self.root / "marker"
        SMOKE.STOP = None

    def run_fake(self, mode, timeout=5):
        with patch.dict(os.environ, {"SMOKE_TEST_MARKER": str(self.marker)}):
            return SMOKE.smoke([sys.executable, self.fake, mode], VERSION, COMMIT, self.root / "work", self.root / "proof", timeout=timeout)

    def test_pre_canceled_smoke_does_not_launch_child(self):
        SMOKE.STOP = signal.SIGINT
        with patch.object(SMOKE.subprocess, "Popen", side_effect=AssertionError("must not launch")) as spawn:
            result = self.run_fake("good")
        spawn.assert_not_called()
        self.assertFalse(result["javascriptVerified"])
        self.assertEqual(result["error"]["type"], "InterruptedError")
        SMOKE.STOP = None

    def test_fake_full_protocol_requires_actual_localhost_request(self):
        result = self.run_fake("good")
        self.assertTrue(result["javascriptVerified"])
        self.assertTrue(result["initializeVerified"])
        self.assertEqual(result["verifiedRequests"], 3)
        self.assertEqual(result["offlineFixtureRequests"], 1)

    def test_initialize_success_and_javascript_failure_stays_failure(self):
        result = self.run_fake("js-error")
        self.assertFalse(result["javascriptVerified"])
        self.assertEqual(result["error"]["type"], "ValueError")

    def test_timeout_retires_owned_fake_process(self):
        result = self.run_fake("wait", timeout=0.2)
        self.assertTrue(result["timedOut"])
        self.assertNotIn("cleanupError", result)
        self.assertIsNotNone(result["exitCode"])
        self.assertNotEqual(result["exitCode"], 0)
        self.assertFalse(result["lingeringOwnedGroupAfterLeaderExit"])
        self.assertFalse(result["javascriptVerified"])
        self.assertIsNone(SMOKE.CHILD)

    @unittest.skipIf(os.name == "nt", "POSIX group ownership control")
    def test_lingering_owned_descendant_fails_and_is_retired(self):
        result = self.run_fake("lingering")
        self.assertTrue(result["lingeringOwnedGroupAfterLeaderExit"])
        self.assertFalse(result["javascriptVerified"])
        self.assertNotIn("cleanupError", result)

    @unittest.skipIf(os.name == "nt", "POSIX signal control")
    def test_interrupt_writes_failure_receipt_and_retires_child(self):
        executable = self.root / "fake-executable"
        executable.write_text("#!" + sys.executable + "\n" + FAKE.replace('mode = sys.argv[1] if len(sys.argv) > 1 else "good"', 'mode = "wait"'))
        executable.chmod(0o700)
        helper = Path(SMOKE.__file__).resolve()
        with patch.dict(os.environ, {"SMOKE_TEST_MARKER": str(self.marker)}):
            child = subprocess.Popen([sys.executable, "-B", str(helper), "--binary", str(executable), "--expected-version", VERSION, "--source-commit", COMMIT, "--work", str(self.root / "work"), "--output", str(self.root / "proof")], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 5
            while not self.marker.exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(self.marker.exists(), "fake executable never started")
            child.send_signal(signal.SIGINT)
            stdout, stderr = child.communicate(timeout=15)
            self.assertEqual(child.returncode, 130, (stdout, stderr))
            receipt = json.loads((self.root / "proof" / "initialize.json").read_text())
            self.assertEqual(receipt["interruptedSignal"], signal.SIGINT)
            self.assertFalse(receipt["javascriptVerified"])
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
