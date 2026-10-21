"""Offline controls for distributable dependency metadata and source staging."""

import importlib.util
import io
import json
import tarfile
import tempfile
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


transport = load('transport', ROOT / 'transport/prepare.py')
stage = load('stage', ROOT / 'stage.py')
deno = load('deno', ROOT / 'deno/prepare.py')


class DistributionContracts(unittest.TestCase):
    def test_internal_aliases_target_dependencies_and_native_links_survive(self):
        original = {
            'package': {'name': 'btls-sys', 'version': {'workspace': True},
                        'repository': {'workspace': True}, 'edition': {'workspace': True},
                        'description': 'FFI', 'links': 'btls', 'include': ['/src']},
            'build-dependencies': {'btls': {'workspace': True, 'features': ['b']}},
            'target': {'cfg(windows)': {'dependencies': {'btls-sys': '0.5.6'}}},
            'features': {'prefix-symbols': ['btls/prefix-symbols']},
            'patch': {'crates-io': {'btls': {'path': '../somewhere'}}},
        }
        workspace = {'package': {'version': '0.5.6', 'repository': 'https://upstream.invalid', 'edition': '2021'},
                     'dependencies': {'btls': {'version': '0.5.6', 'features': ['a']}}}
        result = transport.rewrite(original, workspace, 'btls-sys')
        self.assertEqual(result['package']['links'], 'btls')
        self.assertEqual(result['lib']['name'], 'btls_sys')
        self.assertEqual(result['build-dependencies']['btls']['package'], 'turbo-surf-btls')
        self.assertEqual(result['build-dependencies']['btls']['features'], ['a', 'b'])
        self.assertEqual(result['target']['cfg(windows)']['dependencies']['btls-sys']['version'], '=0.5.6')
        self.assertEqual(result['features'], original['features'])
        self.assertNotIn('patch', result)
        self.assertIn('patch', original)
        self.assertEqual(tomllib.loads(transport.dump(result)), result)

    def test_deno_production_and_benchmark_edges_share_one_core_and_v8(self):
        self.assertIn('deno_bench_util', deno.NAMES)
        source = """[package]
name = "deno_web"
version = "0.290.0"
repository = "https://upstream.invalid"
description = "web"
[lib]
name = "deno_web"
[dependencies.deno_core]
version = "0.412.0"
[dev-dependencies.deno_bench_util]
version = "0.253.0"
[target.'cfg(windows)'.dev-dependencies.deno_webidl]
version = "0.259.0"
"""
        result = tomllib.loads(deno.manifest(source, 'deno_web', '0.290.0', 'turbo-surf-deno-web'))
        self.assertEqual(result['lib']['name'], 'deno_web')
        self.assertEqual(result['dependencies']['deno_core']['package'], 'turbo-surf-deno-core')
        self.assertEqual(result['dev-dependencies']['deno_bench_util']['package'], 'turbo-surf-deno-bench-util')
        self.assertEqual(result['target']['cfg(windows)']['dev-dependencies']['deno_webidl']['package'], 'turbo-surf-deno-webidl')
        self.assertEqual(result['dev-dependencies']['deno_bench_util']['version'], '=0.253.0')

    def test_source_acquisition_rejects_existing_paths_and_cargo_home(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); cargo_home = root / 'cargo'; cargo_home.mkdir()
            with self.assertRaisesRegex(ValueError, 'Cargo home'):
                transport.fetch_sources(cargo_home / 'checkouts', cargo_home)
            existing = root / 'existing'; existing.mkdir()
            with self.assertRaisesRegex(ValueError, 'overwrite'):
                transport.fetch_sources(existing, cargo_home)
            self.assertEqual(list(cargo_home.iterdir()), [])
            self.assertEqual(list(existing.iterdir()), [])

    def test_native_include_patterns_retain_root_and_nested_build_inputs(self):
        patterns = ['/deps/boringssl/**/CMakeLists.txt', '/deps/boringssl/**/*.[chS]']
        for path in ['deps/boringssl/CMakeLists.txt', 'deps/boringssl/crypto/CMakeLists.txt',
                     'deps/boringssl/a.h', 'deps/boringssl/crypto/a.c', 'deps/boringssl/asm/a.S']:
            self.assertTrue(transport.matches(path, patterns), path)
        self.assertFalse(transport.matches('deps/boringssl/testdata/private.pem', patterns))

    def test_bootstrap_removes_git_paths_and_patches_without_changing_alias(self):
        manifest = {'package': {'name': 'example', 'version': '1.0.0'},
                    'dependencies': {'v8': {'package': 'turbo-surf-v8', 'version': '=150.4.0',
                                            'path': '../v8', 'git': 'https://upstream.invalid', 'rev': 'x'}},
                    'patch': {'crates-io': {}}}
        parsed = tomllib.loads(stage.normalize(transport.dump(manifest)))
        self.assertEqual(parsed['dependencies']['v8'], {'package': 'turbo-surf-v8', 'version': '=150.4.0'})
        self.assertNotIn('patch', parsed)
        with self.assertRaises(ValueError):
            stage.normalize('[package]\nname="example"\nversion.workspace=true\n')

    def test_source_staging_preserves_input_and_rejects_identity_collision(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'input'; source.mkdir()
            manifest = '[package]\nname="example"\nversion="1.0.0"\n[dependencies]\n'
            (source / 'Cargo.toml').write_text(manifest)
            (source / 'lib.rs').write_text('pub fn real_code() {}\n')
            directory = root / 'stage'; directory.mkdir()
            result = stage.stage_candidate(source, directory)
            self.assertEqual((source / 'Cargo.toml').read_text(), manifest)
            self.assertFalse((source / '.cargo-checksum.json').exists())
            self.assertEqual(result['kind'], 'bootstrap-source-not-cargo-package')
            checks = json.loads((directory / 'example-1.0.0/.cargo-checksum.json').read_text())
            self.assertEqual(checks['files']['lib.rs'], transport.sha(source / 'lib.rs'))
            with self.assertRaises(ValueError):
                stage.stage_candidate(source, directory)

    def test_stage_guard_detects_candidate_and_linked_cache_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); source = root / 'input'; source.mkdir()
            (source / 'Cargo.toml').write_text('[package]\nname="example"\nversion="1.0.0"\n')
            (source / 'lib.rs').write_text('original source')
            directory = root / 'sources'; directory.mkdir()
            stage.stage_candidate(source, directory)
            candidate = directory / 'example-1.0.0'
            (candidate / 'lib.rs').unlink()
            (candidate / 'lib.rs').symlink_to(source / 'lib.rs')
            before = stage.verify_stage(directory)
            self.assertTrue(before['allExpectedFileBytesMatch'])
            (source / 'lib.rs').write_text('changed source')
            with self.assertRaisesRegex(ValueError, 'source mismatch'):
                stage.verify_stage(directory)

    def test_actual_archive_keeps_normalized_manifest_and_binds_archive_checksum(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); archive = root / 'example.crate'
            manifest = b'[package]\nname="example"\nversion="1.0.0"\n'
            with tarfile.open(archive, 'w:gz') as output:
                item = tarfile.TarInfo('example-1.0.0/Cargo.toml'); item.size = len(manifest)
                output.addfile(item, io.BytesIO(manifest))
            directory = root / 'stage'; directory.mkdir()
            result = stage.stage_archive(archive, directory)
            self.assertEqual(result['kind'], 'actual-package-archive')
            self.assertEqual((directory / 'example-1.0.0/Cargo.toml').read_bytes(), manifest)
            self.assertEqual(result['archiveSha256'], transport.sha(archive))
            checks = json.loads((directory / 'example-1.0.0/.cargo-checksum.json').read_text())
            self.assertEqual(checks['package'], transport.sha(archive))

    def test_archive_rejects_escaping_members_before_extraction(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); archive = root / 'bad.crate'
            with tarfile.open(archive, 'w:gz') as output:
                item = tarfile.TarInfo('example-1.0.0/../escaped'); item.size = 1
                output.addfile(item, io.BytesIO(b'x'))
            directory = root / 'stage'; directory.mkdir()
            with self.assertRaises(ValueError):
                stage.stage_archive(archive, directory)
            self.assertFalse((root / 'escaped').exists())


if __name__ == '__main__':
    unittest.main()
