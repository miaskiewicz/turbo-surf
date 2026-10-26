"""Offline controls for pinned source acquisition and dependency identity."""

import hashlib
import importlib.util
import io
import json
import re
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
tree_spec = importlib.util.spec_from_file_location("deno_candidate_tree", ROOT / "candidate_tree.py")
candidate_tree = importlib.util.module_from_spec(tree_spec)
tree_spec.loader.exec_module(candidate_tree)
RENDER_PINS = ROOT.parents[1] / "crates/turbo-surf-render/src/deno_extension_sources_tests.rs"


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

    FACADE = '''[package]
name = "deno_v8"
version = "0.4.0"
repository = "https://github.com/denoland/deno"

[features]
default = ["v8"]
quickjs = [
    "dep:v8x_backend",
    "v8x_backend/quickjs",
]
simdutf = [
    "rusty_v8?/simdutf",
    "v8x_backend?/simdutf",
]
use_custom_libcxx = [
    "rusty_v8?/use_custom_libcxx",
    "v8x_backend?/use_custom_libcxx",
]
v8 = ["dep:rusty_v8"]
v8_enable_pointer_compression = [
    "rusty_v8?/v8_enable_pointer_compression",
    "v8x_backend?/v8_enable_pointer_compression",
]
v8_enable_sandbox = [
    "rusty_v8?/v8_enable_sandbox",
    "v8x_backend?/v8_enable_sandbox",
]
v8_enable_v8_checks = [
    "rusty_v8?/v8_enable_v8_checks",
    "v8x_backend?/v8_enable_v8_checks",
]

[lib]
name = "deno_v8"
path = "lib.rs"

[dependencies.rusty_v8]
version = "150.4.0"
optional = true
default-features = false
package = "v8"

[dependencies.v8x_backend]
version = "=149.4.0-rc.4"
optional = true
default-features = false
package = "v8x"
'''

    def package(self, upstream):
        return next(p for p in prepare.PACKAGES if p.upstream == upstream)

    def test_facade_requires_registry_turbo_dom_v8_and_drops_second_backend(self):
        result = tomllib.loads(prepare.manifest(self.FACADE, self.package("deno_v8")))
        self.assertEqual(result["package"]["name"], "turbo-surf-deno-v8")
        self.assertEqual(result["package"]["version"], "0.4.1")
        self.assertEqual(result["package"]["metadata"]["turbo-surf-upstream"]["version"], "0.4.0")
        self.assertEqual(result["lib"], {"name": "deno_v8", "path": "lib.rs"})
        self.assertEqual(result["dependencies"], {"rusty_v8": {
            "version": "~150.4", "optional": True, "default-features": False, "package": "turbo-dom-v8"}})
        self.assertEqual(result["features"], {
            "default": ["v8"], "v8": ["dep:rusty_v8"], "simdutf": ["rusty_v8?/simdutf"],
            "use_custom_libcxx": ["rusty_v8?/use_custom_libcxx"],
            "v8_enable_pointer_compression": ["rusty_v8?/v8_enable_pointer_compression"],
            "v8_enable_sandbox": ["rusty_v8?/v8_enable_sandbox"],
            "v8_enable_v8_checks": ["rusty_v8?/v8_enable_v8_checks"]})

    def test_facade_feature_drift_is_not_silently_rewritten(self):
        changed = self.FACADE.replace('default = ["v8"]', 'default = ["v8", "simdutf"]', 1)
        with self.assertRaisesRegex(ValueError, "facade features changed"):
            prepare.manifest(changed, self.package("deno_v8"))
        stray = self.FACADE.replace('name = "deno_v8"\nversion = "0.4.0"', 'name = "serde_v8"\nversion = "0.321.0"')
        with self.assertRaises(ValueError):
            prepare.manifest(stray, self.package("serde_v8"))

    def test_core_and_serde_drop_quickjs_forwarding_and_pin_publish_versions(self):
        for upstream, forwards in prepare.QUICKJS_FORWARDS.items():
            package = self.package(upstream)
            with self.subTest(package=upstream):
                source = f'''[package]
name = "{upstream}"
version = "{package.version}"
repository = "https://github.com/denoland/deno"

[features]
default = ["v8"]
quickjs = {json.dumps(forwards)}
v8 = ["v8/v8"]

[lib]
name = "{upstream}"

[dependencies.v8]
version = "=0.4.0"
features = ["simdutf"]
default-features = false
package = "deno_v8"

[dependencies.serde_v8]
version = "=0.321.0"
'''
                result = tomllib.loads(prepare.manifest(source, package))
                self.assertEqual(result["features"], {"default": ["v8"], "v8": ["v8/v8"]})
                self.assertEqual(result["package"]["version"], package.publish)
                self.assertEqual(result["dependencies"]["v8"]["package"], "turbo-surf-deno-v8")
                self.assertEqual(result["dependencies"]["v8"]["version"], "=0.4.1")
                self.assertEqual(result["dependencies"]["v8"]["features"], ["simdutf"])
                self.assertEqual(result["dependencies"]["serde_v8"]["version"], "=0.321.1")
                with self.assertRaisesRegex(ValueError, "quickjs forwarding changed"):
                    prepare.manifest(source.replace("quickjs = ", "quickjs_old = "), package)

    def test_closure_and_pins_name_seven_distributions_without_native_v8(self):
        pins = json.loads((ROOT / "source-pins.json").read_text())["packages"]
        self.assertEqual(len(prepare.PACKAGES), 7)
        self.assertNotIn("v8", pins)
        self.assertEqual({p.upstream: (p.version, p.name, p.publish) for p in prepare.PACKAGES},
                         {k: (v["version"], v["candidateName"], v["publishVersion"]) for k, v in pins.items()})
        for package in prepare.PACKAGES:
            major, minor, patch_level = package.version.split(".")
            self.assertEqual(package.publish, f"{major}.{minor}.{int(patch_level) + 1}")
        self.assertEqual({k for k, v in pins.items() if "embeddedSources" in v}, prepare.EMBEDDED_LEAVES)
        self.assertFalse((ROOT / "v8-teardown-tests.patch").exists())

    def test_each_standalone_leaf_selects_v8_only_through_own_default_feature(self):
        leaves = {"deno_bench_util", "deno_crypto", "deno_webidl", "deno_web"}
        self.assertEqual(prepare.V8_DEFAULT_LEAVES, leaves)
        for package in prepare.PACKAGES:
            old, version, new = package.upstream, package.version, package.name
            if old not in leaves:
                continue
            with self.subTest(package=old):
                source = f'''[package]
name = "{old}"
version = "{version}"
repository = "https://github.com/denoland/deno"
license = "MIT"
[lib]
name = "{old}"
[dependencies.deno_core]
version = "0.412.0"
default-features = false
features = ["reactor-tokio", "v8_use_custom_libcxx"]
'''
                result = tomllib.loads(prepare.manifest(source, package))
                self.assertEqual(result["features"], {"default": ["v8"], "v8": ["deno_core/v8"]})
                core = result["dependencies"]["deno_core"]
                self.assertFalse(core["default-features"])
                self.assertEqual(core["features"], ["reactor-tokio", "v8_use_custom_libcxx"])
                self.assertEqual(core["package"], "turbo-surf-deno-core")
                self.assertEqual(core["version"], "=0.412.1")
                self.assertEqual(core["path"], "../turbo-surf-deno-core")
                self.assertEqual(result["package"]["license"], "MIT")
                self.assertEqual(result["package"]["version"], package.publish)
                self.assertEqual(result["lib"]["name"], old)

    def test_new_upstream_leaf_feature_policy_is_not_silently_overwritten(self):
        with self.assertRaisesRegex(ValueError, "features changed"):
            prepare.backend_features({"features": {"default": ["future-backend"]}}, "deno_web")

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
            with patch.object(archive_check.prepare, "PACKAGES", [prepare.Package("example", "0.9.0", "example", "1.0.0", None)]):
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
            with patch.object(archive_check.prepare, "PACKAGES", [prepare.Package("example", "0.9.0", "example", "1.0.0", None)]):
                with self.assertRaisesRegex(ValueError, "Packaged source changed"):
                    archive_check.check(prepared, archives)

    def test_actual_archive_requires_license_and_embedded_javascript(self):
        for missing, message in [("LICENSE.md", "license missing"), ("extension.js", "source missing")]:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as directory:
                prepared, archives = self.packaged_fixture(Path(directory), lambda files: files.pop(missing))
                with patch.object(archive_check.prepare, "PACKAGES", [prepare.Package("example", "0.9.0", "example", "1.0.0", None)]):
                    with self.assertRaisesRegex(ValueError, message):
                        archive_check.check(prepared, archives)



class EmbeddedSourceContracts(unittest.TestCase):
    def fixture(self, root):
        target = root / "deno_x"
        target.mkdir()
        files = {"lib.rs": b'deno_core::extension!(deno_x,\n  esm = [ "a.js" ],\n  lazy_loaded_js = [ "c.js", "b.js" ],\n);\n',
                 "a.js": b"export const a = 1;\n", "b.js": b"globalThis.b = 2;\n", "c.js": b"globalThis.c = 3;\n",
                 "LICENSE.md": b"MIT License\n"}
        for name, payload in files.items():
            (target / name).write_bytes(payload)
        pin = {"declarationSha256": hashlib.sha256(files["lib.rs"]).hexdigest(), "licenseFile": "LICENSE.md",
               "licenseSha256": hashlib.sha256(files["LICENSE.md"]).hexdigest(),
               "sources": [{"kind": kind, "file": name, "sha256": hashlib.sha256(files[name]).hexdigest(), "bytes": len(files[name])}
                           for kind, name in [("esm", "a.js"), ("lazy_loaded_js", "c.js"), ("lazy_loaded_js", "b.js")]]}
        return target, files, pin

    def test_module_lists_declared_sources_in_order_and_keeps_upstream_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            target, files, pin = self.fixture(Path(directory))
            prepare.embedded_module("deno_x", pin, target)
            module = (target / "embedded_sources.rs").read_text()
            rows = [line for line in module.splitlines() if line.startswith("    EmbeddedSource {")]
            self.assertEqual(len(rows), 3)
            for row, (kind, name) in zip(rows, [("Esm", "a.js"), ("LazyJs", "c.js"), ("LazyJs", "b.js")]):
                self.assertIn(f'kind: SourceKind::{kind}, specifier: "ext:deno_x/{name}", code: deno_core::ascii_str_include!("{name}")', row)
                self.assertIn(f'sha256: "{hashlib.sha256(files[name]).hexdigest()}", bytes: {len(files[name])} }}', row)
            self.assertIn('pub const EXTENSION: &str = "deno_x";', module)
            self.assertIn('pub const LICENSE: &str = include_str!("LICENSE.md");', module)
            self.assertEqual((target / "lib.rs").read_bytes(), files["lib.rs"] + prepare.LIB_SUFFIX.encode())
            for name in ["a.js", "b.js", "c.js", "LICENSE.md"]:
                self.assertEqual((target / name).read_bytes(), files[name])

    def test_inventory_bytes_declaration_and_license_drift_are_rejected(self):
        cases = [
            ("inventory", lambda target, pin: pin["sources"].reverse()),
            ("source bytes", lambda target, pin: (target / "b.js").write_bytes(b"globalThis.b = 9;\n")),
            ("source bytes", lambda target, pin: pin["sources"][0].update(bytes=1)),
            ("declaration", lambda target, pin: (target / "lib.rs").write_text("// changed\n")),
            ("license", lambda target, pin: (target / "LICENSE.md").write_text("changed\n")),
            ("already has", lambda target, pin: (target / "embedded_sources.rs").write_text("")),
        ]
        for message, mutate in cases:
            with self.subTest(message=message), tempfile.TemporaryDirectory() as directory:
                target, _, pin = self.fixture(Path(directory))
                mutate(target, pin)
                with self.assertRaisesRegex(ValueError, message):
                    prepare.embedded_module("deno_x", pin, target)

    def test_non_ascii_source_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            target, _, pin = self.fixture(Path(directory))
            payload = "globalThis.b = '\u00e9';\n".encode()
            (target / "b.js").write_bytes(payload)
            pin["sources"][2].update(sha256=hashlib.sha256(payload).hexdigest(), bytes=len(payload))
            with self.assertRaisesRegex(ValueError, "source bytes"):
                prepare.embedded_module("deno_x", pin, target)

    def test_pins_match_the_reviewed_render_pins(self):
        # turbo-surf-render re-checks the distributions' embedded tables against its
        # own reviewed SOURCE_PINS / LICENSE_SHA256; both pin sets must agree.
        text = RENDER_PINS.read_text()
        render = [(s, h, int(n)) for s, h, n in re.findall(r'\("(ext:[^"]+)", "([0-9a-f]{64})", (\d+)\),', text)]
        license_pin = re.search(r'const LICENSE_SHA256: &str = "([0-9a-f]{64})";', text)[1]
        pins = json.loads((ROOT / "source-pins.json").read_text())["packages"]
        expected = []
        for old in ["deno_web", "deno_webidl", "deno_crypto"]:
            embedded = pins[old]["embeddedSources"]
            self.assertEqual(embedded["licenseSha256"], license_pin)
            expected += [(f"ext:{old}/{row['file']}", row["sha256"], row["bytes"]) for row in embedded["sources"]]
        self.assertEqual(set(pins) & prepare.EMBEDDED_LEAVES, {"deno_web", "deno_webidl", "deno_crypto"})
        self.assertEqual(render, expected)


class CandidateTreeContracts(unittest.TestCase):
    MANIFEST = '''[package]
name = "turbo-surf-deno-web"
version = "0.290.1"

[lib]
name = "deno_web"
path = "lib.rs"

[[bench]]
name = "encoding"
path = "benches/encoding.rs"

[dependencies.deno_core]
version = "=0.412.1"
default-features = false
package = "turbo-surf-deno-core"
path = "../turbo-surf-deno-core"

[target.'cfg(windows)'.dev-dependencies.deno_webidl]
version = "=0.259.1"
package = "turbo-surf-deno-webidl"
path = "../turbo-surf-deno-webidl"
'''

    def test_registry_form_drops_only_dependency_paths(self):
        result = tomllib.loads(candidate_tree.without_paths(self.MANIFEST))
        self.assertEqual(result["lib"]["path"], "lib.rs")
        self.assertEqual(result["bench"][0]["path"], "benches/encoding.rs")
        self.assertNotIn("path", result["dependencies"]["deno_core"])
        self.assertEqual(result["dependencies"]["deno_core"]["version"], "=0.412.1")
        self.assertNotIn("path", result["target"]["cfg(windows)"]["dev-dependencies"]["deno_webidl"])
        with self.assertRaisesRegex(ValueError, "more than dependency paths"):
            # An inline dependency path would survive line removal; refuse it.
            candidate_tree.without_paths(self.MANIFEST + '[dev-dependencies]\nfoo = { version = "1", path = "../foo" }\n')

    def test_duplicate_package_names_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for folder in ["a", "b/nested"]:
                (root / folder).mkdir(parents=True)
                (root / folder / "Cargo.toml").write_text('[package]\nname = "same"\nversion = "1.0.0"\n')
            with self.assertRaisesRegex(ValueError, "Duplicate candidate package name"):
                candidate_tree.manifests(root)

    def test_patch_snippet_pins_every_distribution_to_one_revision(self):
        snippet = tomllib.loads(candidate_tree.patch_snippet("a" * 40))["patch"]["crates-io"]
        self.assertEqual(set(snippet), {p.name for p in prepare.PACKAGES})
        self.assertEqual({(v["git"], v["rev"]) for v in snippet.values()}, {(candidate_tree.REPOSITORY, "a" * 40)})


if __name__ == "__main__":
    unittest.main()
