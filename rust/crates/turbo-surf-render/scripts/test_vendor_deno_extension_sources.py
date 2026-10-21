#!/usr/bin/env python3
"""Offline safety and drift controls for the extension-source reproducer."""
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("vendor_deno", Path(__file__).with_name("vendor-deno-extension-sources.py"))
vendor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vendor)


def sha(data):
    return hashlib.sha256(data).hexdigest()


class VendorTests(unittest.TestCase):
    def fixture(self, entries=None):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        declaration = b'deno_core::extension!(deno_webidl, lazy_loaded_js = ["00_webidl.js"],);\n'
        source, license_text = b'// exact upstream bytes\nconst value = 1;\n', b'MIT License\nFull license bytes\n'
        package = {"package": "turbo-surf-deno-webidl", "version": "0.259.0", "alias": "deno_webidl", "licenseFile": "LICENSE.md", "licenseSha256": sha(license_text), "declarationSha256": sha(declaration), "sources": [{"kind": "lazy_loaded_js", "file": "00_webidl.js", "specifier": "ext:deno_webidl/00_webidl.js", "sha256": sha(source), "bytes": len(source)}]}
        archive = root / 'source.crate'
        prefix = package['package'] + '-' + package['version'] + '/'
        with tarfile.open(archive, 'w:gz') as tar:
            for name, data in [('lib.rs', declaration), ('LICENSE.md', license_text), ('00_webidl.js', source)]:
                entry = tarfile.TarInfo(prefix + name);entry.size = len(data);tar.addfile(entry, io.BytesIO(data))
            for entry, data in entries or []:
                if entry.isfile() and entry.size and data is None:
                    # Deliberately header-only: size admission must reject before body reads.
                    tar.fileobj.write(entry.tobuf())
                    tar.offset += tarfile.BLOCKSIZE
                else:
                    tar.addfile(entry, io.BytesIO(data) if data is not None else None)
        package['archiveSha256'] = sha(archive.read_bytes())
        return root, archive, package, source

    def test_reproduces_exact_source_license_and_static_specifier(self):
        _, archive, package, source = self.fixture()
        files, table = vendor.reproduce([package], {package['package']: archive})
        self.assertEqual(files[Path('deno_webidl/00_webidl.js')], source)
        self.assertEqual(sha(files[Path('deno_webidl/LICENSE.md')]), package['licenseSha256'])
        self.assertIn(b'deno_core::ascii_str_include!("vendor/deno_extensions/deno_webidl/00_webidl.js")', table)
        self.assertIn(package['sources'][0]['sha256'].encode(), table)

    def test_archive_checksum_is_checked_before_extraction(self):
        _, archive, package, _ = self.fixture();archive.write_bytes(b'wrong archive')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            vendor.archive_inputs(package, archive)

    def test_declared_inventory_and_hash_drift_rejected(self):
        _, archive, package, _ = self.fixture()
        for field in ['declarationSha256', 'licenseSha256']:
            changed = copy.deepcopy(package);changed[field] = '0' * 64
            with self.assertRaises(ValueError): vendor.archive_inputs(changed, archive)
        for field, value in [('sha256', '0' * 64), ('bytes', 1000), ('specifier', 'ext:other/00_webidl.js')]:
            changed = copy.deepcopy(package);changed['sources'][0][field] = value
            with self.assertRaises(ValueError): vendor.archive_inputs(changed, archive)
        changed = copy.deepcopy(package);changed['sources'].append(copy.deepcopy(changed['sources'][0]))
        with self.assertRaisesRegex(ValueError, 'inventory'):
            vendor.archive_inputs(changed, archive)

    def test_linked_duplicate_traversal_and_oversized_entries_rejected(self):
        prefix = 'turbo-surf-deno-webidl-0.259.0/'
        cases = []
        linked = tarfile.TarInfo(prefix+'linked');linked.type=tarfile.SYMTYPE;linked.linkname='/outside';cases.append((linked,None))
        duplicate = tarfile.TarInfo(prefix+'lib.rs');cases.append((duplicate,b''))
        traversal = tarfile.TarInfo(prefix+'../outside');cases.append((traversal,b''))
        oversized = tarfile.TarInfo(prefix+'oversized');oversized.size=8*1024**2+1;cases.append((oversized,None))
        for entry, data in cases:
            _, archive, package, _ = self.fixture([(entry,data)])
            with self.assertRaises(ValueError): vendor.archive_inputs(package,archive)

    def test_generated_root_link_rejected(self):
        root, _, _, _ = self.fixture()
        outside = root / 'outside';outside.mkdir()
        linked = root / 'generated';linked.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'root'):
            vendor.existing(linked)

    def test_dependency_version_alias_and_lock_drift_rejected(self):
        root, _, package, _ = self.fixture()
        manifest = root/'rust/crates/turbo-surf-render/Cargo.toml';manifest.parent.mkdir(parents=True)
        def write_manifest(version='=0.259.0', name='turbo-surf-deno-webidl'):
            manifest.write_text('[dependencies]\ndeno_webidl = { package = '+json.dumps(name)+', version = '+json.dumps(version)+' }\n')
        write_manifest();lock=root/'rust/Cargo.lock'
        lock.write_text('[[package]]\nname = "turbo-surf-deno-webidl"\nversion = "0.259.0"\nchecksum = '+json.dumps(package['archiveSha256'])+'\n')
        vendor.check_dependencies(root,[package])
        for version, name in [('=0.259.1','turbo-surf-deno-webidl'),('=0.259.0','other')]:
            write_manifest(version,name)
            with self.assertRaisesRegex(ValueError,'drift'): vendor.check_dependencies(root,[package])
        write_manifest();lock.write_text('package = []\n')
        with self.assertRaisesRegex(ValueError,'drift'): vendor.check_dependencies(root,[package])


if __name__ == '__main__':
    unittest.main()
