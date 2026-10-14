"""Offline checks that SDK Cargo resolution never changes production locks."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from surf_workspace import prepare_workspace, cargo_arguments


class WorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.source = Path(self.temporary.name) / 'source'
        crate = self.source / 'rust/crates/fixture'
        (crate / 'src').mkdir(parents=True)
        (self.source / 'rust/Cargo.toml').write_text('[workspace]\nmembers=["crates/fixture"]\nresolver="2"\n')
        self.manifest = crate / 'Cargo.toml'
        self.manifest.write_text('[package]\nname="fixture"\nversion="0.0.1"\nedition="2021"\n')
        (crate / 'src/lib.rs').write_text('pub fn value() -> u8 { 1 }\n')
        self.lock = self.source / 'rust/Cargo.lock'
        self.lock.write_text('# production marker\nversion=4\n\n[[package]]\nname="fixture"\nversion="0.0.1"\n')
        self.destination = self.source / '.native-builds/sdk/surf-workspace'

    def test_real_cargo_resolves_in_isolated_workspace_and_preserves_original_lock(self):
        original = self.lock.read_bytes()
        workspace = prepare_workspace(self.source, self.destination)
        arguments = cargo_arguments(['generate-lockfile', '--offline', '--manifest-path', str(self.source / 'rust/Cargo.toml')], self.source, workspace)
        subprocess.run(['cargo', *arguments], cwd=workspace, check=True, capture_output=True)
        self.assertEqual(self.lock.read_bytes(), original)
        self.assertNotEqual((workspace / 'rust/Cargo.lock').read_bytes(), original)
        metadata = subprocess.check_output(['cargo', 'metadata', '--offline', '--no-deps', '--format-version', '1', '--manifest-path', str(workspace / 'rust/Cargo.toml')], cwd=workspace)
        self.assertEqual(Path(json.loads(metadata)['workspace_root']), workspace / 'rust')

    def test_current_source_and_updated_manifests_are_used_without_resetting_profile_lock(self):
        workspace = prepare_workspace(self.source, self.destination)
        profile_lock = workspace / 'rust/Cargo.lock'
        profile_lock.write_text('profile-owned lock\n')
        self.manifest.write_text(self.manifest.read_text().replace('0.0.1', '0.0.2'))
        source = self.manifest.parent / 'src/lib.rs'
        source.write_text('pub fn value() -> u8 { 2 }\n')
        prepare_workspace(self.source, self.destination)
        self.assertIn('0.0.2', (workspace / 'rust/crates/fixture/Cargo.toml').read_text())
        self.assertEqual((workspace / 'rust/crates/fixture/src/lib.rs').read_bytes(), source.read_bytes())
        self.assertEqual(profile_lock.read_text(), 'profile-owned lock\n')

    def test_manifest_arguments_cover_plugins_members_and_refuse_outside_sources(self):
        workspace = prepare_workspace(self.source, self.destination)
        arguments = cargo_arguments(['clippy', '--manifest-path=' + str(self.manifest), '--', '-D', 'warnings'], self.source, workspace)
        self.assertEqual(arguments[1], '--manifest-path=' + str(workspace / 'rust/crates/fixture/Cargo.toml'))
        self.assertEqual(cargo_arguments(['test', '-p', 'fixture'], self.source, workspace)[1:3], ['--manifest-path', str(workspace / 'rust/Cargo.toml')])
        with self.assertRaises(ValueError):
            cargo_arguments(['test', '--manifest-path', str(self.source.parent / 'Cargo.toml')], self.source, workspace)
        with self.assertRaises(ValueError):
            prepare_workspace(self.source, self.source)


if __name__ == '__main__':
    unittest.main()
