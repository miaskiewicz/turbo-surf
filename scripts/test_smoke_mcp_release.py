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


class EmbeddedSourceCheckoutTests(unittest.TestCase):
    """Real Git checkout conversion must preserve every pinned runtime asset."""

    ASSET_DIR = Path("rust/crates/turbo-surf-render/src/vendor/deno_extensions")
    ATTRIBUTE_RULE = "rust/crates/turbo-surf-render/src/vendor/deno_extensions/** -text -whitespace"

    def setUp(self):
        self.source = Path(__file__).resolve().parents[1]
        self.temp = tempfile.TemporaryDirectory(prefix="surf-source-checkout-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "repository"
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.git("init", "--quiet")
        self.assets = {}
        pins = json.loads((self.source / self.ASSET_DIR / "pins.json").read_bytes())
        for package in pins["packages"]:
            folder = self.ASSET_DIR / package["alias"]
            for row in package["sources"]:
                self.read_asset(folder / row["file"], row["sha256"])
            self.read_asset(folder / package["licenseFile"], package["licenseSha256"])
        self.read_asset(self.ASSET_DIR / "pins.json")
        self.assertEqual(len(self.assets), 32)
        actual = {path.relative_to(self.source).as_posix() for path in (self.source / self.ASSET_DIR).rglob("*") if path.is_file()}
        self.assertEqual(set(self.assets), actual)
        self.assertEqual(sum(name.endswith(".js") for name in self.assets), 28)
        self.assertEqual(sum(name.endswith("/LICENSE.md") for name in self.assets), 3)
        for name, data in self.assets.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.attributes = (self.source / ".gitattributes").read_bytes()

    def read_asset(self, path, expected=None):
        import hashlib
        name = path.as_posix()
        self.assertNotIn(name, self.assets)
        data = (self.source / path).read_bytes()
        self.assertIn(b"\n", data)
        self.assertNotIn(b"\r", data)
        if expected is not None:
            self.assertEqual(hashlib.sha256(data).hexdigest(), expected, name)
        self.assets[name] = data

    def git(self, *args):
        result = subprocess.run(["git", *args], cwd=self.repo, env=self.env, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, (args, result.stdout, result.stderr))
        return result.stdout

    def checkout(self, attributes):
        (self.repo / ".gitattributes").write_bytes(attributes)
        self.git("-c", "core.autocrlf=false", "-c", "core.safecrlf=false", "add", "--force", "--", ".gitattributes", *self.assets)
        output = self.root / "checkout"
        output.mkdir()
        self.git("-c", "core.autocrlf=true", "checkout-index", "--prefix=" + output.as_posix() + "/", "--", *self.assets)
        return {name: (output / name).read_bytes() for name in self.assets}

    def test_windows_checkout_preserves_all_pinned_extension_bytes(self):
        self.assertIn(self.ATTRIBUTE_RULE.encode(), self.attributes.splitlines())
        actual = self.checkout(self.attributes)
        self.assertEqual(actual, self.assets)
        values = self.git("check-attr", "-z", "text", "whitespace", "--", *self.assets).decode().split("\0")
        self.assertEqual(values[-1], "")
        rows = [values[index:index + 3] for index in range(0, len(values) - 1, 3)]
        self.assertEqual(len(rows), 64)
        self.assertEqual({row[0] for row in rows}, set(self.assets))
        self.assertTrue(all(row[1] in ("text", "whitespace") and row[2] == "unset" for row in rows))

    def test_missing_nested_rule_reproduces_windows_byte_drift(self):
        attributes = b"".join(line for line in self.attributes.splitlines(keepends=True) if line.rstrip(b"\r\n") != self.ATTRIBUTE_RULE.encode())
        actual = self.checkout(attributes)
        changed = {name for name in self.assets if actual[name] != self.assets[name]}
        self.assertEqual(changed, set(self.assets))
        for name, data in self.assets.items():
            self.assertEqual(actual[name], data.replace(b"\n", b"\r\n"), name)

    def test_windows_checkout_with_crlf_attributes_preserves_all_bytes(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_windows_checkout_preserves_all_pinned_extension_bytes()

    def test_missing_nested_rule_with_crlf_attributes_reproduces_drift(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_missing_nested_rule_reproduces_windows_byte_drift()

if __name__ == "__main__":
    unittest.main()
