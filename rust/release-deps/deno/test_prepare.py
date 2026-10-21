"""Offline controls for pinned source acquisition and dependency identity."""

import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("deno_prepare", ROOT / "prepare.py")
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)
archive_spec = importlib.util.spec_from_file_location("deno_archive_check", ROOT / "check_archives.py")
archive_check = importlib.util.module_from_spec(archive_spec)
archive_spec.loader.exec_module(archive_check)


class ArchiveContracts(unittest.TestCase):
    def pin(self, payload):
        return {"sourceUrl": "https://static.crates.io/crates/example/example-1.0.0.crate",
                "archiveSha256": hashlib.sha256(payload).hexdigest()}

    def test_ci_fetch_writes_only_verified_private_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = b"exact upstream archive"
            with patch.object(prepare.urllib.request, "urlopen", return_value=io.BytesIO(payload)) as request:
                result = prepare.source_archive("example", "1.0.0", self.pin(payload), root / "cargo", root / "archives", True)
            self.assertEqual(result.read_bytes(), payload)
            self.assertFalse((root / "cargo").exists())
            self.assertEqual(request.call_args.args[0].full_url, self.pin(payload)["sourceUrl"])

    def test_bad_download_is_rejected_before_archive_write(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(prepare.urllib.request, "urlopen", return_value=io.BytesIO(b"tampered")):
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    prepare.source_archive("example", "1.0.0", self.pin(b"original"), root / "cargo", root / "archives", True)
            self.assertFalse((root / "archives").exists())

    def test_cached_source_copies_to_private_archive_without_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / "cargo"
            cached = cargo / "registry/cache/index/example-1.0.0.crate"
            cached.parent.mkdir(parents=True)
            cached.write_bytes(b"cached source")
            before = cached.stat()
            result = prepare.source_archive("example", "1.0.0", self.pin(b"cached source"), cargo, root / "archives", True)
            self.assertEqual(result.read_bytes(), cached.read_bytes())
            self.assertNotEqual(result, cached)
            self.assertEqual((before.st_ino, before.st_mtime_ns), (cached.stat().st_ino, cached.stat().st_mtime_ns))

    def test_bad_private_archive_and_cache_output_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archives = root / "archives"
            archives.mkdir()
            (archives / "example-1.0.0.crate").write_bytes(b"wrong")
            with self.assertRaisesRegex(ValueError, "modified exact"):
                prepare.source_archive("example", "1.0.0", self.pin(b"right"), root / "cargo", archives, True)
            with self.assertRaisesRegex(ValueError, "outside Cargo"):
                prepare.source_archive("example", "1.0.0", self.pin(b"right"), root / "cargo", root / "cargo/private", True)

    def test_manifest_keeps_aliases_features_and_native_archive_version(self):
        source = '''[package]
name = "deno_v8"
version = "0.4.0"
repository = "https://github.com/denoland/deno"
[lib]
name = "deno_v8"
[dependencies.rusty_v8]
package = "v8"
version = "150.4.0"
default-features = false
[features]
v8 = ["dep:rusty_v8"]
'''
        result = tomllib.loads(prepare.manifest(source, "deno_v8", "0.4.0", "turbo-surf-deno-v8"))
        self.assertEqual(result["lib"]["name"], "deno_v8")
        self.assertEqual(result["features"], {"v8": ["dep:rusty_v8"]})
        self.assertEqual(result["dependencies"]["rusty_v8"]["package"], "turbo-surf-v8")
        self.assertEqual(result["dependencies"]["rusty_v8"]["version"], "=150.4.0")

    def packaged_fixture(self, root, mutate=None):
        source = root / "prepared/packages/example"
        source.mkdir(parents=True)
        manifest = '[package]\nname="example"\nversion="1.0.0"\n[dependencies.foo]\nversion="1.0"\npath = "../foo"\n'
        files = {"Cargo.toml": manifest.encode(), "lib.rs": b"pub fn real_code() {}\n",
                 "extension.js": b"globalThis.working = true;\n", "LICENSE.md": b"MIT license fixture\n"}
        for name, payload in files.items():
            (source / name).write_bytes(payload)
        receipt = {"packages": [{"name": "example", "version": "1.0.0", "candidateFiles": prepare.tree(source)}]}
        (root / "prepared/prepared-source-identity.json").write_text(json.dumps(receipt))
        packaged = {**files, "Cargo.toml": manifest.replace('path = "../foo"\n', '').encode(),
                    "Cargo.toml.orig": manifest.encode()}
        if mutate:
            mutate(packaged)
        archives = root / "archives"
        archives.mkdir()
        with tarfile.open(archives / "example-1.0.0.crate", "w:gz") as output:
            for name, payload in packaged.items():
                item = tarfile.TarInfo("example-1.0.0/" + name)
                item.size = len(payload)
                output.addfile(item, io.BytesIO(payload))
        return root / "prepared", archives

    def test_actual_archive_normalizes_paths_and_keeps_source_and_license(self):
        with tempfile.TemporaryDirectory() as directory:
            prepared, archives = self.packaged_fixture(Path(directory))
            with patch.object(archive_check.prepare, "PACKAGES", [("example", "1.0.0", "example", None)]):
                result = archive_check.check(prepared, archives)
            self.assertEqual(result["packages"][0]["licenses"], ["LICENSE.md"])
            self.assertTrue(result["packages"][0]["sourceAndNormalizedManifestVerified"])

    def test_cargo_resolver_materialization_preserves_explicit_package_policy(self):
        manifest = {"package": {"name": "example", "version": "1.0.0"}}
        self.assertEqual(archive_check.normalized(manifest, "2")["package"]["resolver"], "2")
        self.assertNotIn("resolver", manifest["package"])
        manifest["package"]["resolver"] = "1"
        self.assertEqual(archive_check.normalized(manifest, "2")["package"]["resolver"], "1")

    def test_actual_archive_rejects_source_replacement(self):
        with tempfile.TemporaryDirectory() as directory:
            prepared, archives = self.packaged_fixture(Path(directory), lambda files: files.update({"lib.rs": b"changed code"}))
            with patch.object(archive_check.prepare, "PACKAGES", [("example", "1.0.0", "example", None)]):
                with self.assertRaisesRegex(ValueError, "Packaged source changed"):
                    archive_check.check(prepared, archives)

    def test_actual_archive_requires_license_and_embedded_javascript(self):
        for missing, message in [("LICENSE.md", "license missing"), ("extension.js", "source missing")]:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as directory:
                prepared, archives = self.packaged_fixture(Path(directory), lambda files: files.pop(missing))
                with patch.object(archive_check.prepare, "PACKAGES", [("example", "1.0.0", "example", None)]):
                    with self.assertRaisesRegex(ValueError, message):
                        archive_check.check(prepared, archives)


if __name__ == "__main__":
    unittest.main()
