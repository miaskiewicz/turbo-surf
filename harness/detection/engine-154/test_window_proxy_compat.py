"""Source-only SDK contracts; never compile or execute V8/Cargo."""
import hashlib
import importlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from release_aliases import sdk_config
import test_release_aliases as legacy

SOURCES = {
    'src/context.rs': ('pub fn detach_global(&self) {}\n'
                       'pub global_template: ();\npub global_object: ();\n'),
    'src/isolate.rs': 'pub fn set_failed_access_check_callback(&self) {}\n',
    'src/template.rs': ('pub fn set_access_check_callback_and_handler(&self) {}\n'
                        'pub fn set_internal_field_count(&self) {}\n'
                        'pub fn set_immutable_proto(&self) {}\n'
                        'pub fn read_only_prototype(&self) {}\n'
                        'pub fn instance_template<T>(&self) {}\n'
                        'pub fn set_class_name(&self) {}\n'),
    'src/function.rs': ('pub type AccessCheckCallback = ();\npub enum AccessType {}\n'
                        'pub type FailedAccessCheckCallback = ();\n'),
    'src/binding.cc': ('void v8__Context__DetachGlobal() {}\n'
                       'void v8__Isolate__SetFailedAccessCheckCallbackFunction() {}\n'
                       'void v8__ObjectTemplate__SetAccessCheckCallbackAndHandler() { '
                       'v8::NamedPropertyHandlerConfiguration(); '
                       'v8::IndexedPropertyHandlerConfiguration(); }\n'),
    'src/lib.rs': '// SDK\n',
}
PROFILE = {'chromeMajor': 154, 'engineRevision': 'b3d6849efaf11696198e15b32290e6d6b0d540c1'}


def write_sources(root):
    (root / 'src').mkdir()
    for name, text in SOURCES.items():
        (root / name).write_text(text)
    return {name: hashlib.sha256(text.encode()).hexdigest() for name, text in SOURCES.items()}


class WindowProxyCompatibilityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.hashes = write_sources(self.root)
        (self.root / 'Cargo.toml').write_text('[package]\nname="v8"\nversion="152.2.0"\n')
        self.module = importlib.import_module('window_proxy_compat')

    def test_marker_is_added_once_only_after_source_verification(self):
        with patch.dict(self.module.EXPECTED_SOURCES, self.hashes, clear=True):
            record = self.module.enable(self.root, PROFILE)
            before = (self.root / 'src/lib.rs').read_bytes()
            self.assertEqual(self.module.enable(self.root, PROFILE), record)
            self.assertEqual((self.root / 'src/lib.rs').read_bytes(), before)
            self.assertEqual(before.count(b'pub const FORK_WINDOW_PROXY'), 1)
            self.module.verify(self.root, PROFILE, record)
            (self.root / 'src/context.rs').write_text('// missing API\n')
            with self.assertRaisesRegex(ValueError, 'detach_global'):
                self.module.verify(self.root, PROFILE, record)

    def test_missing_duplicate_and_modified_apis_cannot_receive_marker(self):
        mutations = [('src/context.rs', '// missing\n'),
                     ('src/context.rs', SOURCES['src/context.rs'] * 2),
                     ('src/function.rs', SOURCES['src/function.rs'].replace('pub enum AccessType', 'enum Other')),
                     ('src/binding.cc', SOURCES['src/binding.cc'] + '// unreviewed\n')]
        for name, replacement in mutations:
            with self.subTest(name=name, replacement=replacement):
                with patch.dict(self.module.EXPECTED_SOURCES, self.hashes, clear=True):
                    (self.root / name).write_text(replacement)
                    with self.assertRaises(ValueError):
                        self.module.enable(self.root, PROFILE)
                    self.assertEqual((self.root / 'src/lib.rs').read_text(), SOURCES['src/lib.rs'])
                    (self.root / name).write_text(SOURCES[name])

    def test_genuine_root_template_surface_is_required_before_marker(self):
        with patch.dict(self.module.EXPECTED_SOURCES, self.hashes, clear=True):
            name = 'src/template.rs'
            (self.root / name).write_text(SOURCES[name].replace('pub fn set_immutable_proto(', 'pub fn other('))
            with self.assertRaisesRegex(ValueError, 'set_immutable_proto'):
                self.module.enable(self.root, PROFILE)
            self.assertEqual((self.root / 'src/lib.rs').read_text(), SOURCES['src/lib.rs'])

    def test_wrong_profile_and_foreign_marker_are_refused(self):
        with patch.dict(self.module.EXPECTED_SOURCES, self.hashes, clear=True):
            for profile in [{'chromeMajor': 150}, {'chromeMajor': 154, 'engineRevision': 'unreviewed'}]:
                with self.assertRaisesRegex(ValueError, '154/155'):
                    self.module.enable(self.root, profile)
            (self.root / 'src/lib.rs').write_text(SOURCES['src/lib.rs'] + 'pub const FORK_WINDOW_PROXY: bool = false;\n')
            with self.assertRaisesRegex(ValueError, 'marker'):
                self.module.enable(self.root, PROFILE)

    def test_record_cannot_be_reused_for_a_different_engine(self):
        other = {'chromeMajor': 155, 'engineRevision': 'd89479ac5446a374c55a35be19b5a23437558d99'}
        with patch.dict(self.module.EXPECTED_SOURCES, self.hashes, clear=True):
            record = self.module.enable(self.root, PROFILE)
            with self.assertRaisesRegex(ValueError, 'provenance'):
                self.module.verify(self.root, other, record)


class RequestedAliasTests(unittest.TestCase):
    setUp = legacy.ReleaseAliasTests.setUp

    def test_default_alias_needs_no_capability_or_source_changes(self):
        config = sdk_config(self.source, self.sdk, self.workspace)
        bridge = json.loads((config.parent / 'metadata-bridge.json').read_text())
        self.assertEqual(bridge['manifests']['v8-exact']['aliasPackage'], 'turbo-dom-v8 150.4.0')
        for name, expected in self.before.items():
            self.assertEqual((self.sdk / name).read_bytes(), expected)

    def test_requested_alias_requires_verified_capability(self):
        with self.assertRaisesRegex(ValueError, 'WindowProxy'):
            sdk_config(self.source, self.sdk, self.workspace, native_alias_version='150.4.2')
        self.assertFalse(self.workspace.exists())

    def test_unknown_alias_is_refused_without_writes(self):
        with self.assertRaisesRegex(ValueError, 'Unsupported'):
            sdk_config(self.source, self.sdk, self.workspace, native_alias_version='150.4.99')
        self.assertFalse(self.workspace.exists())

    def test_requested_alias_keeps_sdk_sources_and_records_distinct_identity(self):
        with patch('release_aliases.verify_window_proxy_alias') as verified:
            config = sdk_config(self.source, self.sdk, self.workspace, native_alias_version='150.4.2')
        verified.assert_called_once_with(self.sdk)
        bridge = json.loads((config.parent / 'metadata-bridge.json').read_text())
        self.assertEqual(bridge['manifests']['v8-exact']['aliasPackage'], 'turbo-dom-v8 150.4.2')
        for name, expected in self.before.items():
            self.assertEqual((self.sdk / name).read_bytes(), expected)

    def test_requested_alias_verifies_prepared_sources_end_to_end(self):
        module = importlib.import_module('window_proxy_compat')
        native = self.sdk / 'v8-exact'
        manifest = native / 'Cargo.toml'
        manifest.write_text(manifest.read_text().replace('"path" = "lib.rs"', '"path" = "src/lib.rs"'))
        hashes = write_sources(native)
        with patch.dict(module.EXPECTED_SOURCES, hashes, clear=True):
            record = module.enable(native, PROFILE)
            (self.sdk / 'provenance.json').write_text(json.dumps({**PROFILE, 'windowProxyCompatibility': record}))
            config = sdk_config(self.source, self.sdk, self.workspace, native_alias_version='150.4.2')
            bridge = json.loads((config.parent / 'metadata-bridge.json').read_text())
            self.assertEqual(bridge['manifests']['v8-exact']['aliasPackage'], 'turbo-dom-v8 150.4.2')
            (native / 'src/template.rs').write_text('// removed API')
            with self.assertRaisesRegex(ValueError, 'set_access_check'):
                sdk_config(self.source, self.sdk, self.workspace, native_alias_version='150.4.2')


if __name__ == "__main__":
    unittest.main()
