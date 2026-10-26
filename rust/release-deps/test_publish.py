import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("dependency_publish", ROOT / "publish.py")
publish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publish)


def crate(path, *, content=b"reviewed", lock=b"generated", manifest_suffix="", duplicate=False):
    manifest = '[package]\nname="example"\nversion="1.0.0"\n' + manifest_suffix
    with tarfile.open(path, "w:gz") as archive:
        values = {"Cargo.toml": manifest.encode(), "Cargo.toml.orig": manifest.encode(),
                  "Cargo.lock": lock, ".cargo_vcs_info.json": lock,
                  "src/lib.rs": content, "LICENSE": b"MIT"}
        for name, value in values.items():
            entry = tarfile.TarInfo("example-1.0.0/" + name)
            entry.size = len(value)
            entry.mode = 0o644
            archive.addfile(entry, io.BytesIO(value))
        if duplicate:
            entry = tarfile.TarInfo("example-1.0.0/Cargo.lock")
            archive.addfile(entry, io.BytesIO())


class PublicationControls(unittest.TestCase):
    def test_same_version_rejects_changed_runtime_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "a.crate", Path(directory) / "b.crate"
            crate(a)
            crate(b, content=b"different")
            with self.assertRaisesRegex(ValueError, "Published payload differs"):
                publish.require_equal_payload(a, b, "example", "1.0.0")

    def test_only_generated_records_and_manifest_serialization_may_differ(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "a.crate", Path(directory) / "b.crate"
            crate(a)
            crate(b, lock=b"another generated lock", manifest_suffix="# serialization\n")
            publish.require_equal_payload(a, b, "example", "1.0.0")
            crate(b, manifest_suffix='description="changed semantics"\n')
            with self.assertRaises(ValueError):
                publish.require_equal_payload(a, b, "example", "1.0.0")

    def test_duplicate_generated_member_is_not_hidden_by_exclusion(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "a.crate"
            crate(path, duplicate=True)
            with self.assertRaisesRegex(ValueError, "Duplicate"):
                publish.archive_payload(path, "example", "1.0.0")

    def test_escaping_and_link_members_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            for name, kind in [("example-1.0.0/../escape", tarfile.REGTYPE),
                               ("example-1.0.0/link", tarfile.SYMTYPE)]:
                path = Path(directory) / "bad.crate"
                with tarfile.open(path, "w:gz") as archive:
                    entry = tarfile.TarInfo(name)
                    entry.type = kind
                    archive.addfile(entry)
                with self.assertRaisesRegex(ValueError, "Invalid"):
                    publish.archive_payload(path, "example", "1.0.0")

    def test_retry_after_seconds_http_date_and_deadline(self):
        self.assertEqual(publish.retry_after("12"), 12)
        self.assertEqual(publish.retry_after("Thu, 01 Jan 1970 00:00:20 GMT", now=5), 15)
        self.assertIsNone(publish.retry_after("not-a-date"))
        sleeps = []
        publish.pause(100, 0, "17", clock=lambda: 0, sleep=sleeps.append)
        self.assertEqual(sleeps, [17])
        with self.assertRaises(TimeoutError):
            publish.pause(10, 0, "11", clock=lambda: 0, sleep=sleeps.append)
        self.assertEqual(sleeps, [17])

    def test_registry_429_honors_header_and_auth_failure_is_not_retried(self):
        response = io.BytesIO(b"metadata")
        error = urllib.error.HTTPError("https://crates.io/example", 429, "limited", {"Retry-After": "7"}, None)
        with patch.object(publish.urllib.request, "urlopen", side_effect=[error, response]), \
                patch.object(publish, "pause") as pause:
            self.assertEqual(publish.Registry().request("https://crates.io/example", publish.time.monotonic() + 100), b"metadata")
            self.assertEqual(pause.call_args.args[2], "7")
        denied = urllib.error.HTTPError("https://crates.io/example", 403, "denied", {}, None)
        with patch.object(publish.urllib.request, "urlopen", side_effect=denied) as request:
            with self.assertRaisesRegex(RuntimeError, "HTTP 403"):
                publish.Registry().request("https://crates.io/example", publish.time.monotonic() + 100)
            self.assertEqual(request.call_count, 1)

    def test_registry_checks_identity_checksum_yank_and_exact_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            expected = directory / "expected.crate"
            crate(expected)
            payload = expected.read_bytes()
            record = {"version": {"crate": "example", "num": "1.0.0", "yanked": False,
                                  "checksum": hashlib.sha256(payload).hexdigest()}}
            with patch.object(publish.Registry, "request", side_effect=[json.dumps(record).encode(), payload]):
                self.assertTrue(publish.Registry().version("example", "1.0.0", expected, directory / "download", 100))
            record["version"]["yanked"] = True
            with patch.object(publish.Registry, "request", return_value=json.dumps(record).encode()):
                with self.assertRaisesRegex(ValueError, "yank"):
                    publish.Registry().version("example", "1.0.0", expected, directory / "download", 100)
            record["version"]["yanked"] = False
            with patch.object(publish.Registry, "request", side_effect=[json.dumps(record).encode(), b"tampered"]):
                with self.assertRaisesRegex(ValueError, "checksum"):
                    publish.Registry().version("example", "1.0.0", expected, directory / "download", 100)

    def test_runner_keeps_token_out_of_verification_and_forces_stock_engine(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "CARGO_REGISTRY_TOKEN": "fake-unit-only", "RUSTY_V8_ARCHIVE": "wrong",
                "CARGO_PROFILE_DEV_DEBUG_ASSERTIONS": "false", "CARGO_SOURCE_CRATES_IO_REPLACE_WITH": "foreign"}):
            environment = publish.Runner(Path(directory)).environment
            self.assertNotIn("CARGO_REGISTRY_TOKEN", environment)
            self.assertNotIn("RUSTY_V8_ARCHIVE", environment)
            self.assertNotIn("CARGO_PROFILE_DEV_DEBUG_ASSERTIONS", environment)
            self.assertNotIn("CARGO_SOURCE_CRATES_IO_REPLACE_WITH", environment)
            self.assertEqual(environment["RUSTUP_TOOLCHAIN"], "1.98.0")

    def test_commit_guard_rejects_dirty_release_sources(self):
        sha = "1" * 40
        with patch.object(publish.subprocess, "check_output", side_effect=[sha + "\n", " M rust/release-deps/publish.py\n"]):
            with self.assertRaisesRegex(ValueError, "dirty"):
                publish.commit_matches(sha)

    def test_lost_publish_response_checks_registry_before_retrying(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            source = output / "source"
            source.mkdir()
            (source / "lib.rs").write_text("reviewed")
            archive = output / "expected.crate"
            crate(archive)
            package = {"name": "example", "version": "1.0.0", "source": str(source),
                       "sourceFiles": publish.source_files(source), "sourceModes": {"lib.rs": False},
                       "archive": str(archive), "archiveSha256": publish.digest(archive)}
            class FakeRunner:
                calls = []
                def run(self, command, name, check):
                    self.calls.append(command)
                    log = output / "lost-response.log"
                    log.write_text("HTTP 503 upload response lost")
                    return 101, log
            class FakeRegistry:
                calls = 0
                def version(self, name, version, expected, downloaded, deadline):
                    self.calls += 1
                    if self.calls == 1:
                        return False
                    downloaded.write_bytes(expected.read_bytes())
                    return True
            runner = FakeRunner()
            result = publish.publish_package(package, runner, FakeRegistry(), output, publish.time.monotonic() + 100)
            self.assertTrue(result["exactPayloadVerified"])
            self.assertEqual(len(runner.calls), 1)
            self.assertNotIn("--no-verify", runner.calls[0])

    def test_quota_retry_does_not_stop_uploading_after_six_attempts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            source = output / "source"
            source.mkdir()
            (source / "lib.rs").write_text("reviewed")
            archive = output / "expected.crate"
            crate(archive)
            package = {"name": "example", "version": "1.0.0", "source": str(source),
                       "sourceFiles": publish.source_files(source), "sourceModes": {"lib.rs": False},
                       "archive": str(archive), "archiveSha256": publish.digest(archive)}
            class FakeRunner:
                calls = 0
                def run(self, command, name, check):
                    self.calls += 1
                    log = output / (name + ".log")
                    log.write_text("HTTP 429\nRetry-After: 60\n")
                    return 101, log
            runner = FakeRunner()
            class FakeRegistry:
                def version(self, name, version, expected, downloaded, deadline):
                    if runner.calls < 8:
                        return False
                    downloaded.write_bytes(expected.read_bytes())
                    return True
            with patch.object(publish, "pause", side_effect=[None] * 8):
                result = publish.publish_package(package, runner, FakeRegistry(), output, publish.time.monotonic() + 900)
            self.assertTrue(result["exactPayloadVerified"])
            self.assertEqual(runner.calls, 8)

    def test_preexisting_foreign_payload_refuses_before_any_upload(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            source = output / "source"
            source.mkdir()
            (source / "lib.rs").write_text("reviewed")
            archive = output / "expected.crate"
            crate(archive)
            package = {"name": "example", "version": "1.0.0", "source": str(source),
                       "sourceFiles": publish.source_files(source), "sourceModes": {"lib.rs": False},
                       "archive": str(archive), "archiveSha256": publish.digest(archive)}
            with patch.object(publish.Runner, "run") as upload, \
                    patch.object(publish.Registry, "version", side_effect=ValueError("Published payload differs")):
                with self.assertRaisesRegex(ValueError, "Published payload differs"):
                    publish.publish_package(package, publish.Runner(output), publish.Registry(), output, 100)
                upload.assert_not_called()

    def test_candidate_identity_keeps_reviewed_lock_but_allows_new_generated_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "packages/example"
            source.mkdir(parents=True)
            (source / "Cargo.toml").write_text('[package]\nname="example"\nversion="1.0.0"\n')
            lock = source / "Cargo.lock"
            lock.write_text("reviewed lock")
            files = publish.source_files(source)
            receipt = root / "prepared-source-identity.json"
            receipt.write_text(json.dumps({"packages": [{"name": "example", "candidateFiles": files}]}))
            with patch.object(publish, "ORDER", [("example", "1.0.0", "deno")]):
                self.assertEqual(len(publish.candidates({"deno": root})), 1)
                lock.write_text("changed reviewed lock")
                with self.assertRaisesRegex(ValueError, "source identity"):
                    publish.candidates({"deno": root})
                files.pop("Cargo.lock")
                receipt.write_text(json.dumps({"packages": [{"name": "example", "candidateFiles": files}]}))
                self.assertEqual(len(publish.candidates({"deno": root})), 1)

    def test_fixed_order_contains_all_normal_and_dev_edges(self):
        order = {name: index for index, (name, _, _) in enumerate(publish.ORDER)}
        self.assertEqual(len(order), 12)
        self.assertNotIn("turbo-surf-v8", order)
        for dependency, dependent in [("turbo-surf-deno-v8", "turbo-surf-serde-v8"),
                                      ("turbo-surf-serde-v8", "turbo-surf-deno-core"),
                                      ("turbo-surf-deno-v8", "turbo-surf-deno-core"),
                                      ("turbo-surf-deno-core", "turbo-surf-deno-crypto"),
                                      ("turbo-surf-deno-core", "turbo-surf-deno-bench-util"),
                                      ("turbo-surf-deno-bench-util", "turbo-surf-deno-webidl"),
                                      ("turbo-surf-deno-webidl", "turbo-surf-deno-web"),
                                      ("turbo-surf-btls-sys", "turbo-surf-btls"),
                                      ("turbo-surf-btls", "turbo-surf-tokio-btls"),
                                      ("turbo-surf-tokio-btls", "turbo-surf-wreq"),
                                      ("turbo-surf-wreq", "turbo-surf-wreq-util")]:
            self.assertLess(order[dependency], order[dependent])

    def test_deno_order_matches_prepared_publish_versions(self):
        spec = importlib.util.spec_from_file_location("deno_prepare_for_order", ROOT / "deno/prepare.py")
        prepare = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(prepare)
        self.assertEqual({(n, v) for n, v, family in publish.ORDER if family == "deno"},
                         {(p.name, p.publish) for p in prepare.PACKAGES})

    def test_registry_consumer_requires_one_registry_turbo_dom_v8(self):
        registry = "registry+https://github.com/rust-lang/crates.io-index"
        def graph(*packages):
            return {"packages": [{"name": n, "version": v, "source": s} for n, v, s in packages]}
        publish.require_deno_runtime(graph(("turbo-dom-v8", "150.4.0", registry), ("turbo-surf-deno-v8", "0.4.1", registry)))
        for packages in [[], [("turbo-dom-v8", "150.5.0", registry)], [("turbo-dom-v8", "150.4.0", "git+https://x")],
                         [("turbo-dom-v8", "150.4.0", registry), ("turbo-dom-v8", "150.4.1", registry)]]:
            with self.subTest(packages=packages), self.assertRaisesRegex(ValueError, "exactly one"):
                publish.require_deno_runtime(graph(*packages))
        for retired in ["turbo-surf-v8", "v8", "v8x", "deno_core"]:
            with self.subTest(retired=retired), self.assertRaisesRegex(ValueError, "original runtime"):
                publish.require_deno_runtime(graph(("turbo-dom-v8", "150.4.0", registry), (retired, "1.0.0", registry)))

    def test_commit_guard_no_longer_tracks_the_rusty_v8_vendor_copy(self):
        with patch.object(publish.subprocess, "check_output", side_effect=["1" * 40 + "\n", ""]) as output:
            publish.commit_matches("1" * 40)
        paths = output.call_args_list[1].args[0]
        self.assertIn("rust/vendor/deno_crypto", paths)
        self.assertNotIn("rust/vendor/rusty_v8", paths)


if __name__ == "__main__":
    unittest.main()
