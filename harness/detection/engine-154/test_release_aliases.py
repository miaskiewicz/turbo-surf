"""Offline guards for registry-name aliases over the verified native SDK."""
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

from release_aliases import manifest, sdk_config


class ReleaseAliasTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / 'source'
        self.sdk = self.root / 'sdk'
        self.workspace = self.sdk / 'surf-workspace'
        renderer = self.source / 'rust/crates/turbo-surf-render'
        renderer.mkdir(parents=True)
        self.renderer = renderer / 'Cargo.toml'
        self.renderer.write_text('[dependencies]\ndeno_core={package="turbo-surf-deno-core",version="0.412.0"}\n')
        self.sdk.mkdir()
        (self.sdk / 'Cargo.toml').write_text('[package]\nname="sdk-experiment"\nversion="0.0.0"\n')
        for name, version, dependencies in [
            ('deno_core', '0.412.0', {'v8': {'package': 'deno_v8', 'version': '0.4.0'}, 'serde_v8': '0.321.0'}),
            ('deno_v8', '0.4.0', {'rusty_v8': {'package': 'v8', 'path': '../v8-exact', 'optional': True}}),
        ]:
            directory = self.sdk / name
            directory.mkdir()
            (directory / 'Cargo.toml').write_text(manifest({
                'package': {'name': name, 'version': version, 'edition': '2024'},
                'lib': {'name': name, 'path': 'lib.rs'}, 'dependencies': dependencies,
            }))
            (directory / 'lib.rs').write_text('// unchanged SDK source\n')
        self.before = {str(p.relative_to(self.sdk)): p.read_bytes() for p in self.sdk.rglob('*') if p.is_file()}

    def test_aliases_preserve_sources_and_native_backend_and_are_repeatable(self):
        config = sdk_config(self.source, self.sdk, self.workspace)
        self.assertEqual(sdk_config(self.source, self.sdk, self.workspace), config)
        patches = tomllib.loads(config.read_text())['patch']['crates-io']
        core = Path(patches['turbo-surf-deno-core']['path'])
        facade = Path(patches['turbo-surf-deno-v8']['path'])
        metadata = tomllib.loads((core / 'Cargo.toml').read_text())
        self.assertEqual(metadata['lib']['name'], 'deno_core')
        self.assertEqual(metadata['dependencies']['v8']['package'], 'turbo-surf-deno-v8')
        self.assertEqual(metadata['dependencies']['serde_v8']['version'], '=0.321.0')
        self.assertEqual(metadata['workspace'], {})
        native = tomllib.loads((facade / 'Cargo.toml').read_text())['dependencies']['rusty_v8']
        self.assertEqual(native, {'package': 'v8', 'path': str(self.sdk / 'v8-exact'), 'optional': True})
        self.assertEqual((core / 'lib.rs').resolve(), self.sdk / 'deno_core/lib.rs')
        for name, expected in self.before.items():
            self.assertEqual((self.sdk / name).read_bytes(), expected)
        self.assertEqual(len(json.loads((config.parent / 'metadata-bridge.json').read_text())['manifests']), 2)
        # locate-project exercises Cargo's workspace discovery without fetching
        # unpublished dependencies or compiling anything.
        result = subprocess.check_output(['cargo', 'locate-project', '--workspace', '--manifest-path', str(core / 'Cargo.toml')])
        self.assertEqual(Path(json.loads(result)['root']), core / 'Cargo.toml')

    def test_legacy_profile_keeps_original_config(self):
        self.renderer.write_text('[dependencies]\ndeno_core="0.412.0"\n')
        self.assertEqual(sdk_config(self.source, self.sdk, self.workspace), self.sdk / 'cargo-config.toml')
        self.assertFalse(self.workspace.exists())

    def test_unknown_identity_and_outside_workspace_fail(self):
        with self.assertRaisesRegex(ValueError, 'private Surf workspace'):
            sdk_config(self.source, self.sdk, self.root / 'outside')
        self.renderer.write_text('[dependencies]\ndeno_core={package="unknown",version="0.412.0"}\n')
        with self.assertRaisesRegex(ValueError, 'runtime identity'):
            sdk_config(self.source, self.sdk, self.workspace)

    def test_foreign_alias_content_is_not_overwritten(self):
        config = sdk_config(self.source, self.sdk, self.workspace)
        alias = config.parent / 'deno_core/Cargo.toml'
        alias.write_text('foreign content\n')
        with self.assertRaisesRegex(ValueError, 'alias manifest content'):
            sdk_config(self.source, self.sdk, self.workspace)
        self.assertEqual(alias.read_text(), 'foreign content\n')

    def test_wrong_sdk_identity_fails(self):
        path = self.sdk / 'deno_core/Cargo.toml'
        path.write_text(path.read_text().replace('0.412.0', '0.411.0'))
        with self.assertRaisesRegex(ValueError, 'SDK dependency identity'):
            sdk_config(self.source, self.sdk, self.workspace)

    def test_original_package_cannot_be_used_as_alias_workspace(self):
        with self.assertRaisesRegex(ValueError, 'overlap original SDK packages'):
            sdk_config(self.source, self.sdk, self.sdk / 'deno_core/private')
        self.assertFalse((self.sdk / 'deno_core/private').exists())

    def test_alias_directories_cannot_escape_through_existing_links(self):
        outside = self.root / 'outside'
        outside.mkdir()
        self.workspace.mkdir()
        alias_root = self.workspace / 'sdk-release-aliases'
        alias_root.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'linked SDK alias directory'):
            sdk_config(self.source, self.sdk, self.workspace)
        alias_root.unlink()
        alias_root.mkdir()
        (alias_root / 'deno_core').symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'linked SDK alias directory'):
            sdk_config(self.source, self.sdk, self.workspace)
        self.assertEqual(list(outside.iterdir()), [])

    def test_generated_config_cannot_overwrite_a_link_target(self):
        config = sdk_config(self.source, self.sdk, self.workspace)
        outside = self.root / 'sentinel'
        outside.write_text('preserve me\n')
        config.unlink()
        config.symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'alias manifest content'):
            sdk_config(self.source, self.sdk, self.workspace)
        self.assertEqual(outside.read_text(), 'preserve me\n')


if __name__ == '__main__':
    unittest.main()
