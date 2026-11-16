import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest
import urllib.error
import urllib.request
import urllib.response
from email.message import Message
from unittest import mock

ROOT = Path(__file__).parent
spec = importlib.util.spec_from_file_location("exact_archive", ROOT / "exact_archive.py")
exact = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exact)


class ExactArchives(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.packages, self.archives = [], []
        for name, version in exact.ORDER:
            directory = self.root / (name + "-" + version)
            directory.mkdir()
            manifest = f'[package]\nname="{name}"\nversion="{version}"\nlicense="MIT"\n'
            files = {"Cargo.toml": manifest.encode(), "README.md": b"Reviewed README\n", "LICENSE": b"MIT\n"}
            for file, data in files.items():
                (directory / file).write_bytes(data)
            archive = self.root / (directory.name + ".crate")
            with tarfile.open(archive, "w:gz") as output:
                for file, data in files.items():
                    info = tarfile.TarInfo(directory.name + "/" + file)
                    info.size = len(data)
                    output.addfile(info, io.BytesIO(data))
            self.archives.append(archive)
            self.packages.append({"name": name, "version": version,
                "manifest_path": str(directory / "Cargo.toml"), "authors": ["Deno authors"],
                "license": "MIT", "readme": "README.md", "license_file": None,
                "features": {"default": ["v8"], "v8": ["dep:v8"]},
                "dependencies": [{"name": "turbo-surf-deno-core", "rename": "deno_core",
                    "req": "=0.412.4", "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "features": ["reactor-tokio"], "optional": True, "uses_default_features": False,
                    "target": "cfg(unix)", "kind": "build"}]})
        self.metadata = self.root / "cargo-metadata.json"
        self.metadata.write_text(json.dumps({"packages": self.packages}))
        self.evidence = self.root / "proof.json"
        self.evidence.write_text('{"actualTestsPassed":true}')
        self.commit = "a" * 40
        self.plan = exact.build_plan(self.archives, self.metadata, self.commit, [self.evidence])
        self.plan_path = self.root / "plan.json"
        self.plan_path.write_text(json.dumps(self.plan))
        self.sha = exact.digest(self.plan_path.read_bytes())

    def test_packaged_lock_preserves_only_registry_dependencies_and_exact_root(self):
        identity = exact.ORDER[0]
        lock = f'[[package]]\nname="{identity[0]}"\nversion="{identity[1]}"\n'
        exact.validate_lock({"Cargo.lock": lock.encode()}, identity)
        remote = '[[package]]\nname="serde"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\n'
        exact.validate_lock({"Cargo.lock": (lock + remote).encode()}, identity)
        for invalid in (remote, lock + lock, lock + remote.replace('registry+https://github.com/rust-lang/crates.io-index', 'git+https://example.invalid/repo'), lock + '[[package]]\nname="local"\nversion="1.0.0"\n'):
            with self.subTest(lock=invalid), self.assertRaises(ValueError):
                exact.validate_lock({"Cargo.lock": invalid.encode()}, identity)


    def invoke(self, **kwargs):
        defaults = {"token_provider": lambda: "unit-secret-not-real",
                    "transport": lambda *_args, **_kwargs: self.fail("unexpected upload"),
                    "commit_check": lambda value: self.assertEqual(value, self.commit),
                    "readback": lambda *_args: True}
        defaults.update(kwargs)
        return exact.upload_plan(self.plan_path, self.sha, self.commit, self.root / "readback", **defaults)

    def test_exact_wire_aliases_flags_and_no_repack(self):
        uploaded, requests = set(), []
        def transport(request, timeout):
            self.assertEqual(request.full_url, exact.ENDPOINT)
            self.assertEqual(request.method, "PUT")
            self.assertEqual(request.get_header("Authorization"), "unit-secret-not-real")
            self.assertEqual(request.get_header("Content-type"), "application/octet-stream")
            self.assertEqual(timeout, 60)
            data = request.data
            length, = struct.unpack("<I", data[:4])
            metadata = json.loads(data[4:4 + length])
            size, = struct.unpack("<I", data[4 + length:8 + length])
            raw = data[8 + length:]
            self.assertEqual(len(raw), size)
            package = next(p for p in self.plan["packages"] if p["name"] == metadata["name"])
            self.assertEqual(exact.digest(raw), package["archiveSha256"])
            self.assertEqual(raw, Path(package["archive"]).read_bytes())
            self.assertEqual(metadata["deps"][0], {"name": "turbo-surf-deno-core", "version_req": "=0.412.4",
                "features": ["reactor-tokio"], "optional": True, "default_features": False,
                "target": "cfg(unix)", "kind": "build", "registry": None, "explicit_name_in_toml": "deno_core"})
            self.assertEqual(metadata["features"], self.packages[0]["features"])
            self.assertEqual(metadata["readme"], "Reviewed README\n")
            self.assertNotIn("unit-secret-not-real", json.dumps(metadata))
            requests.append(metadata["name"])
            uploaded.add(metadata["name"])
            response = io.BytesIO(b'{"warnings":{}}')
            response.status = 200
            return response
        with mock.patch("subprocess.run", side_effect=AssertionError("must not invoke cargo")):
            result = self.invoke(transport=transport, readback=lambda p, *_args: p["name"] in uploaded)
        self.assertEqual(requests, [p[0] for p in exact.ORDER])
        self.assertEqual(len(result), len(exact.ORDER))

    def test_existing_exact_versions_need_no_credential(self):
        self.assertEqual(len(self.invoke(token_provider=lambda: self.fail("credential read"))), len(exact.ORDER))

    def test_wrong_plan_sha_stops_before_credentials(self):
        with self.assertRaisesRegex(ValueError, "approved SHA"):
            exact.upload_plan(self.plan_path, "0" * 64, self.commit, self.root / "out",
                              token_provider=lambda: self.fail("credential read"))

    def test_wrong_source_commit_stops(self):
        with self.assertRaisesRegex(ValueError, "Source or registry"):
            exact.validate_plan(self.plan_path, self.sha, "b" * 40)

    def test_changed_archive_stops_before_credentials(self):
        self.archives[0].write_bytes(b"wrong archive")
        with self.assertRaises((ValueError, tarfile.ReadError)):
            self.invoke(token_provider=lambda: self.fail("credential read"))

    def test_changed_metadata_stops(self):
        self.metadata.write_text('{"packages":[]}')
        with self.assertRaises(ValueError):
            self.invoke()

    def test_changed_source_stops(self):
        Path(self.packages[0]["manifest_path"]).with_name("README.md").write_text("changed")
        with self.assertRaisesRegex(ValueError, "source differs"):
            self.invoke()

    def test_changed_evidence_stops(self):
        self.evidence.write_text("changed")
        with self.assertRaises(ValueError):
            self.invoke()

    def test_wrong_version_is_rejected(self):
        metadata = {"packages": [dict(self.packages[0], version="0.412.2")]}
        with self.assertRaisesRegex(ValueError, "identity"):
            exact.package_plan(self.archives[0], metadata)

    def test_nonregistry_dependency_is_rejected(self):
        with self.assertRaises(ValueError):
            exact.dependency(dict(self.packages[0]["dependencies"][0], path="/private/source"))

    def test_endpoint_is_not_configurable(self):
        self.plan["endpoint"] = "http://unreviewed.invalid/new"
        self.plan_path.write_text(json.dumps(self.plan))
        with self.assertRaisesRegex(ValueError, "registry"):
            exact.validate_plan(self.plan_path, exact.digest(self.plan_path.read_bytes()), self.commit)

    def test_redirects_never_send_a_second_request_or_foreign_authorization(self):
        for code in (303, 307):
            with self.subTest(code=code):
                requests = []
                def fake_https(_handler, request):
                    requests.append((request.full_url, request.get_header("Authorization")))
                    self.assertEqual(request.full_url, exact.ENDPOINT)
                    self.assertEqual(len(requests), 1, "redirect forwarded a second request")
                    headers = Message()
                    headers["Location"] = "https://foreign.invalid/receive"
                    response = urllib.response.addinfourl(io.BytesIO(b""), headers, request.full_url, code)
                    response.msg = "Redirect"
                    return response
                request = urllib.request.Request(exact.ENDPOINT, data=b"reviewed body", method="PUT",
                                                 headers={"Authorization": "dummy-redirect-test"})
                with mock.patch.object(urllib.request.HTTPSHandler, "https_open", fake_https):
                    with self.assertRaisesRegex(urllib.error.HTTPError, "redirect refused"):
                        exact.no_redirect_transport(request, timeout=60)
                self.assertEqual(requests, [(exact.ENDPOINT, "dummy-redirect-test")])

    def test_put_failure_is_not_retried(self):
        calls = []
        def transport(*_args, **_kwargs):
            calls.append(1)
            raise OSError("ambiguous unit failure")
        with self.assertRaises(OSError):
            self.invoke(transport=transport, readback=lambda *_args: False)
        self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
