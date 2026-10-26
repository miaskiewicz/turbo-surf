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
# Upper bound for a fake that should finish promptly (never a timing assumption: an
# idle host takes well under a second, a loaded CI host may take many).
SAFETY_SECONDS = 120


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


FAKE = r'''import os, sys, time
mode = sys.argv[1] if len(sys.argv) > 1 else "good"
if mode == "wait":
    # Publish the marker atomically, before any heavier import: the interrupt control
    # waits for it instead of guessing how long interpreter startup takes.
    with open(os.environ["SMOKE_TEST_MARKER"] + ".tmp", "w") as marker:
        marker.write(str(os.getpid()))
    os.replace(os.environ["SMOKE_TEST_MARKER"] + ".tmp", os.environ["SMOKE_TEST_MARKER"])
    time.sleep(600)
    sys.exit(1)
import json, subprocess, urllib.request
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

    def run_fake(self, mode, timeout=SAFETY_SECONDS):
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
            # Interrupt only once the fake is running, however long two interpreter
            # startups take on a loaded host; a helper that exits first fails at once
            # with its own output. The cap only keeps a broken run from hanging.
            deadline = time.monotonic() + SAFETY_SECONDS
            while not self.marker.exists():
                if child.poll() is not None:
                    self.fail(("helper exited before the fake started", child.returncode, *child.communicate()))
                if time.monotonic() > deadline:
                    self.fail("fake executable never started")
                time.sleep(0.02)
            child.send_signal(signal.SIGINT)
            stdout, stderr = child.communicate(timeout=SAFETY_SECONDS)
            self.assertEqual(child.returncode, 130, (stdout, stderr))
            receipt = json.loads((self.root / "proof" / "initialize.json").read_text())
            self.assertEqual(receipt["interruptedSignal"], signal.SIGINT)
            self.assertFalse(receipt["javascriptVerified"])
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)
            for stream in (child.stdout, child.stderr):
                stream.close()


class _LateReapedLeader:
    """A group leader that has exited but whose first poll() still says "running": the
    leader exits between group_alive's reap and its killpg probe (turbo-surf#45)."""

    def __init__(self):
        self.child = subprocess.Popen(["true"], start_new_session=True)
        self.pid = self.child.pid
        self.polls = 0
        self.returncode = None
        # Wait until the leader is a zombie: the only member of its own group.
        deadline = time.monotonic() + SAFETY_SECONDS
        while time.monotonic() < deadline:
            try:
                os.killpg(self.pid, 0)
            except PermissionError:
                return
            time.sleep(0.01)
        raise AssertionError("leader never became a zombie")

    def poll(self):
        self.polls += 1
        if self.polls == 1:
            return None
        self.returncode = self.child.poll()
        return self.returncode

    def wait(self, timeout=None):
        self.returncode = self.child.wait(timeout=timeout)
        return self.returncode


@unittest.skipIf(sys.platform != "darwin", "macOS reports EPERM for a zombie-only process group")
class ZombieGroupTests(unittest.TestCase):
    @unittest.expectedFailure  # turbo-surf#45: fails until the fix
    def test_group_alive_treats_a_zombie_only_group_as_gone(self):
        leader = _LateReapedLeader()
        try:
            self.assertFalse(SMOKE.group_alive(leader))
        finally:
            leader.wait(timeout=5)

    @unittest.expectedFailure  # turbo-surf#45: fails until the fix
    def test_retire_finishes_when_the_leader_becomes_a_zombie(self):
        leader = _LateReapedLeader()
        try:
            SMOKE.retire(leader)
            self.assertEqual(leader.returncode, 0)
        finally:
            leader.wait(timeout=5)


class _GitCheckoutCase(unittest.TestCase):
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
        self.attributes = (self.source / ".gitattributes").read_bytes()

    def write_assets(self):
        for name, data in self.assets.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

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

    def checkout(self, attributes, additional=()):
        (self.repo / ".gitattributes").write_bytes(attributes)
        self.git("-c", "core.autocrlf=false", "-c", "core.safecrlf=false", "add", "--force", "--", ".gitattributes", *self.assets, *additional)
        output = self.root / "checkout"
        output.mkdir()
        self.git("-c", "core.autocrlf=true", "checkout-index", "--prefix=" + output.as_posix() + "/", "--", *self.assets, *additional)
        return {name: (output / name).read_bytes() for name in self.assets}


class LicenseCheckoutTests(_GitCheckoutCase):
    """Shipping MIT and notice copies retain the canonical full bytes."""

    PATHS = (
        'LICENSE',
        'rust/LICENSE',
        'THIRD_PARTY_NOTICES.md',
        'rust/crates/turbo-surf-core/LICENSE',
        'rust/crates/turbo-surf-page/LICENSE',
        'rust/crates/turbo-surf-view/LICENSE',
        'rust/crates/turbo-surf-render/LICENSE',
        'rust/crates/turbo-surf-transform/LICENSE',
        'rust/crates/turbo-surf-napi/LICENSE',
        'rust/crates/turbo-surf-py/LICENSE',
        'rust/crates/turbo-surf-mcp/LICENSE',
        'rust/crates/turbo-surf-render/THIRD_PARTY_NOTICES.md',
        'rust/crates/turbo-surf-py/python/turbo_surf/THIRD_PARTY_NOTICES.md',
    )

    def setUp(self):
        super().setUp()
        mit = "27d70e62f016d41bc5d379a420e277cee423d0148d3bd083061044a1a54a2fe2"
        notices = "b9528e75b4f5b3eeb794fdf7c78c28a0dc244abef4e35fead9b95533219b0797"
        for name in self.PATHS:
            self.read_asset(Path(name), mit if name.endswith("LICENSE") else notices)
        self.assertEqual(len(self.assets), 13)
        for name, data in self.assets.items():
            self.assertEqual(data, self.assets["LICENSE" if name.endswith("LICENSE") else "THIRD_PARTY_NOTICES.md"], name)
        self.rules = {("/" + name + " -text -whitespace").encode() for name in self.PATHS}
        self.write_assets()

    def test_windows_checkout_preserves_all_full_license_bytes(self):
        self.assertTrue(self.rules.issubset(self.attributes.splitlines()))
        self.assertEqual(self.checkout(self.attributes), self.assets)
        values = self.git("check-attr", "-z", "text", "whitespace", "--", *self.assets).decode().split("\0")
        self.assertEqual(values[-1], "")
        rows = [values[index:index + 3] for index in range(0, len(values) - 1, 3)]
        self.assertEqual(len(rows), 26)
        self.assertEqual({row[0] for row in rows}, set(self.assets))
        self.assertTrue(all(row[1] in ("text", "whitespace") and row[2] == "unset" for row in rows))

    def test_missing_license_rules_reproduces_full_payload_byte_drift(self):
        attributes = b"".join(line for line in self.attributes.splitlines(keepends=True) if line.rstrip(b"\r\n") not in self.rules)
        actual = self.checkout(attributes)
        self.assertEqual({name for name in self.assets if actual[name] != self.assets[name]}, set(self.assets))
        for name, data in self.assets.items():
            self.assertEqual(actual[name], data.replace(b"\n", b"\r\n"), name)

    def test_crlf_attributes_preserve_all_full_license_bytes(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_windows_checkout_preserves_all_full_license_bytes()

    def test_crlf_attributes_without_license_rules_reproduce_drift(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_missing_license_rules_reproduces_full_payload_byte_drift()

    def notices_check(self, attributes):
        manifests = sorted(self.source.glob("rust/crates/turbo-surf-*/Cargo.toml"))
        self.assertEqual(len(manifests), 8)
        support = [Path("scripts/sync-third-party-notices.py")] + [path.relative_to(self.source) for path in manifests]
        for name in support:
            destination = self.repo / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((self.source / name).read_bytes())
        actual = self.checkout(attributes, [path.as_posix() for path in support])
        checkout = self.root / "checkout"
        result = subprocess.run([sys.executable, "-B", str(checkout / support[0]), "--check"], cwd=checkout, env=self.env, capture_output=True, text=True, timeout=15)
        return actual, result

    def test_actual_notices_checker_accepts_windows_checkout(self):
        actual, result = self.notices_check(self.attributes)
        self.assertEqual(actual, self.assets)
        self.assertEqual(result.returncode, 0, (result.stdout, result.stderr))
        self.assertIn("match their root sources", result.stdout)

    def test_actual_notices_checker_rejects_unprotected_workspace_license(self):
        rule = b"/rust/LICENSE -text -whitespace"
        self.assertIn(rule, self.attributes.splitlines())
        attributes = b"".join(line for line in self.attributes.splitlines(keepends=True) if line.rstrip(b"\r\n") != rule)
        actual, result = self.notices_check(attributes)
        self.assertEqual({name for name in self.assets if actual[name] != self.assets[name]}, {"rust/LICENSE"})
        self.assertEqual(result.returncode, 1, (result.stdout, result.stderr))
        self.assertIn("Stale package notices or license:", result.stderr)
        self.assertIn("rust/LICENSE", result.stderr.replace("\\", "/"))

    def test_actual_notices_checker_accepts_crlf_attribute_checkout(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_actual_notices_checker_accepts_windows_checkout()

    def test_actual_notices_checker_rejects_crlf_attributes_without_workspace_rule(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_actual_notices_checker_rejects_unprotected_workspace_license()


class PythonSourceCheckoutTests(_GitCheckoutCase):
    """Installed Python source and smoke receipts use canonical LF hashes."""

    PATH_HASHES = {
        'rust/crates/turbo-surf-py/tests/test_smoke.py': 'a2688b317b60774242bd37427349af9195797c5dd31f5dfa146d87ec6e6aeab5',
        'rust/crates/turbo-surf-py/python/turbo_surf/__init__.py': 'f5a9ca6ea8e257b955e7f47f17806a8f77d9a6ccaf46ebc5f6582d22eb70af3b',
        'rust/crates/turbo-surf-py/python/turbo_surf/__init__.pyi': '51d0c7d4788ac137f90dee9c1b27c48209bc598e59e65dc0a279d3b65d74a2bf',
    }

    def setUp(self):
        super().setUp()
        for name, expected in self.PATH_HASHES.items():
            self.read_asset(Path(name), expected)
        self.assertEqual(len(self.assets), 3)
        self.rules = {(name + " text eol=lf").encode() for name in self.PATH_HASHES}
        self.write_assets()

    def test_windows_checkout_preserves_all_authored_python_bytes(self):
        self.assertTrue(self.rules.issubset(self.attributes.splitlines()))
        self.assertEqual(self.checkout(self.attributes), self.assets)
        values = self.git("check-attr", "-z", "text", "eol", "--", *self.assets).decode().split("\0")
        self.assertEqual(values[-1], "")
        rows = [values[index:index + 3] for index in range(0, len(values) - 1, 3)]
        self.assertEqual(len(rows), 6)
        self.assertEqual({row[0] for row in rows}, set(self.assets))
        self.assertTrue(all((row[1], row[2]) in (("text", "set"), ("eol", "lf")) for row in rows))

    def test_missing_python_rules_reproduces_source_hash_drift(self):
        attributes = b"".join(line for line in self.attributes.splitlines(keepends=True) if line.rstrip(b"\r\n") not in self.rules)
        actual = self.checkout(attributes)
        self.assertEqual({name for name in self.assets if actual[name] != self.assets[name]}, set(self.assets))
        for name, data in self.assets.items():
            self.assertEqual(actual[name], data.replace(b"\n", b"\r\n"), name)

    def test_crlf_attributes_preserve_all_authored_python_bytes(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_windows_checkout_preserves_all_authored_python_bytes()

    def test_crlf_attributes_without_python_rules_reproduce_drift(self):
        self.attributes = self.attributes.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.test_missing_python_rules_reproduces_source_hash_drift()


if __name__ == "__main__":
    unittest.main()
