"""Offline guards for registry-name aliases over the verified native SDK."""
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

from release_aliases import manifest, sdk_config, frame_read_arguments


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
        self.renderer.write_text('[dependencies]\ndeno_core={package="turbo-surf-deno-core",version="=0.412.1"}\n')
        self.sdk.mkdir()
        (self.sdk / 'Cargo.toml').write_text('[package]\nname="sdk-experiment"\nversion="0.0.0"\n')
        for directory, name, version, dependencies, features in [
            ('deno_core', 'deno_core', '0.412.0', {'v8': {'package': 'deno_v8', 'version': '0.4.0'}, 'serde_v8': '0.321.0'},
             {'default': ['v8'], 'quickjs': ['v8/quickjs', 'serde_v8/quickjs'], 'v8': ['v8/v8', 'serde_v8/v8']}),
            ('deno_v8', 'deno_v8', '0.4.0', {'rusty_v8': {'package': 'v8', 'path': '../v8-exact', 'optional': True},
                                             'v8x_backend': {'package': 'v8x', 'version': '=149.4.0-rc.4', 'optional': True}},
             {'default': ['v8'], 'v8': ['dep:rusty_v8'], 'quickjs': ['dep:v8x_backend', 'v8x_backend/quickjs'],
              'simdutf': ['rusty_v8?/simdutf', 'v8x_backend?/simdutf']}),
            ('v8-exact', 'v8', '152.2.0', {'bitflags': '2'}, {'simdutf': []}),
        ]:
            directory = self.sdk / directory
            directory.mkdir()
            package = {'name': name, 'version': version, 'edition': '2024'}
            if name == 'v8':
                package['build'] = 'build.rs'
            (directory / 'Cargo.toml').write_text(manifest({
                'package': package, 'lib': {'name': name, 'path': 'lib.rs'},
                'dependencies': dependencies, 'features': features,
            }))
            (directory / 'lib.rs').write_text('// unchanged SDK source\n')
            if name == 'v8':
                (directory / 'build.rs').write_text('fn main() {}\n')
        self.before = {str(p.relative_to(self.sdk)): p.read_bytes() for p in self.sdk.rglob('*') if p.is_file()}

    def test_aliases_preserve_sources_and_native_backend_and_are_repeatable(self):
        config = sdk_config(self.source, self.sdk, self.workspace)
        self.assertEqual(sdk_config(self.source, self.sdk, self.workspace), config)
        patches = tomllib.loads(config.read_text())['patch']['crates-io']
        self.assertEqual(set(patches), {'turbo-surf-deno-core', 'turbo-surf-deno-v8', 'turbo-dom-v8'})
        core = Path(patches['turbo-surf-deno-core']['path'])
        facade = Path(patches['turbo-surf-deno-v8']['path'])
        native_alias = Path(patches['turbo-dom-v8']['path'])
        metadata = tomllib.loads((core / 'Cargo.toml').read_text())
        self.assertEqual(metadata['package']['version'], '0.412.1')
        self.assertEqual(metadata['lib']['name'], 'deno_core')
        self.assertEqual(metadata['dependencies']['v8'], {'package': 'turbo-surf-deno-v8', 'version': '=0.4.1'})
        self.assertEqual(metadata['dependencies']['serde_v8'], {'version': '=0.321.1', 'package': 'turbo-surf-serde-v8'})
        self.assertEqual(metadata['features'], {'default': ['v8'], 'v8': ['v8/v8', 'serde_v8/v8']})
        self.assertEqual(metadata['workspace'], {})
        facade_metadata = tomllib.loads((facade / 'Cargo.toml').read_text())
        self.assertEqual(facade_metadata['package']['version'], '0.4.1')
        self.assertEqual(facade_metadata['dependencies'], {'rusty_v8': {
            'package': 'turbo-dom-v8', 'path': str(native_alias), 'optional': True}})
        self.assertEqual(facade_metadata['features'], {'default': ['v8'], 'v8': ['dep:rusty_v8'], 'simdutf': ['rusty_v8?/simdutf']})
        v8 = tomllib.loads((native_alias / 'Cargo.toml').read_text())
        self.assertEqual((v8['package']['name'], v8['package']['version'], v8['package']['links']), ('turbo-dom-v8', '150.4.0', 'rusty_v8'))
        self.assertEqual(v8['lib']['name'], 'v8')
        self.assertEqual(v8['dependencies'], {'bitflags': '2'})
        self.assertEqual((native_alias / 'build.rs').resolve(), self.sdk / 'v8-exact/build.rs')
        self.assertEqual((core / 'lib.rs').resolve(), self.sdk / 'deno_core/lib.rs')
        for name, expected in self.before.items():
            self.assertEqual((self.sdk / name).read_bytes(), expected)
        bridge = json.loads((config.parent / 'metadata-bridge.json').read_text())['manifests']
        self.assertEqual({k: v['aliasPackage'] for k, v in bridge.items()},
                         {'deno_core': 'turbo-surf-deno-core 0.412.1', 'deno_v8': 'turbo-surf-deno-v8 0.4.1',
                          'v8-exact': 'turbo-dom-v8 150.4.0'})
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
        self.renderer.write_text('[dependencies]\ndeno_core={package="unknown",version="=0.412.1"}\n')
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
        # Originals are verified at the SDK's own versions, never the alias versions.
        for directory, old, new in [('deno_core', '0.412.0', '0.412.1'), ('deno_v8', '0.4.0', '0.4.1'),
                                    ('v8-exact', '152.2.0', '150.4.0')]:
            with self.subTest(directory=directory):
                path = self.sdk / directory / 'Cargo.toml'
                original = path.read_text()
                path.write_text(original.replace(old, new, 1))
                with self.assertRaisesRegex(ValueError, 'SDK dependency identity'):
                    sdk_config(self.source, self.sdk, self.workspace)
                path.write_text(original)

    def test_render_requirement_must_match_alias_versions(self):
        self.renderer.write_text('[dependencies]\ndeno_core={package="turbo-surf-deno-core",version="=0.412.0"}\n')
        with self.assertRaisesRegex(ValueError, 'target turbo-surf-deno-core =0.412.1'):
            sdk_config(self.source, self.sdk, self.workspace)
        self.assertFalse(self.workspace.exists())

    def test_facade_accepts_renamed_native_package_but_not_others(self):
        path = self.sdk / 'deno_v8/Cargo.toml'
        path.write_text(path.read_text().replace('"package" = "v8"', '"package" = "turbo-dom-v8"', 1))
        config = sdk_config(self.source, self.sdk, self.workspace)
        facade = tomllib.loads((config.parent / 'deno_v8/Cargo.toml').read_text())
        self.assertEqual(facade['dependencies']['rusty_v8']['package'], 'turbo-dom-v8')
        path.write_text(path.read_text().replace('"package" = "turbo-dom-v8"', '"package" = "v8-stock"', 1))
        (config.parent / 'deno_v8/Cargo.toml').unlink()
        with self.assertRaisesRegex(ValueError, 'native SDK backend alias'):
            sdk_config(self.source, self.sdk, self.workspace)

    def test_unexpected_backend_features_fail(self):
        path = self.sdk / 'deno_core/Cargo.toml'
        path.write_text(path.read_text().replace('"v8/quickjs", "serde_v8/quickjs"', '"v8/quickjs"', 1))
        with self.assertRaisesRegex(ValueError, 'backend features'):
            sdk_config(self.source, self.sdk, self.workspace)

    def surf_workspace(self, features):
        rust = self.source / 'rust'
        (rust / 'Cargo.toml').write_text('[workspace]\nmembers = ["crates/turbo-surf-core", "crates/turbo-surf-render", "crates/turbo-surf-napi"]\n')
        for name in ['turbo-surf-core', 'turbo-surf-render', 'turbo-surf-napi']:
            crate = rust / 'crates' / name
            crate.mkdir(parents=True, exist_ok=True)
            table = '[features]\nnative-sdk-frame-read = []\n' if name in features else ''
            (crate / 'Cargo.toml').write_text(f'[package]\nname = "{name}"\nversion = "0.6.0"\n' + table)

    def test_frame_read_feature_follows_selected_forwarding_members(self):
        self.surf_workspace({'turbo-surf-render', 'turbo-surf-napi'})
        enabled = ['--features', 'native-sdk-frame-read']
        for arguments, expected in [
                (['test', '-p', 'turbo-surf-render'], enabled), (['build', '--package=turbo-surf-napi'], enabled),
                (['test', '-pturbo-surf-render', '--', '--nocapture'], enabled), (['test', '--workspace'], enabled),
                (['test'], enabled), (['test', '-p', 'turbo-surf-core'], []),
                (['test', '-p', 'turbo-surf-core', '-p', 'turbo-surf-napi'], enabled)]:
            with self.subTest(arguments=arguments):
                self.assertEqual(frame_read_arguments(arguments[1:], self.source), expected)

    def test_frame_read_feature_must_be_declared_somewhere(self):
        self.surf_workspace(set())
        with self.assertRaisesRegex(ValueError, 'forwarding native-sdk-frame-read'):
            frame_read_arguments(['-p', 'turbo-surf-render'], self.source)

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
