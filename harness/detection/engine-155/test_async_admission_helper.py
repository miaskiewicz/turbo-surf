"""Offline application/provenance tests using synthetic pinned source, no compiler."""
import difflib
import hashlib
import json
import runpy
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent

class ManagedPatchTests(unittest.TestCase):
    def test_forwarded_cargo_help_does_not_skip_verification(self):
        execute = runpy.run_path
        with patch.object(sys, 'argv', ['build.py', 'managed-sdk', '--surf', 'test', '--help']), \
             patch.object(sys, 'path', sys.path.copy()), \
             patch('subprocess.run') as verify, patch('runpy.run_path') as shared:
            execute(str(HERE / 'build.py'), run_name='__main__')
        self.assertEqual(verify.call_count, 1)
        self.assertIn('--record-provenance', verify.call_args.args[0])
        self.assertEqual(shared.call_count, 1)

    def test_apply_idempotency_provenance_reverse_and_unknown_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helper = root / 'helper'
            helper.mkdir()
            shutil.copy(HERE / 'apply_async_admission.py', helper)
            sdk = root / 'sdk'
            source = sdk / 'deno_core'
            source.mkdir(parents=True)
            (sdk / 'cargo-config.toml').write_text('')
            (sdk / 'provenance.json').write_text(json.dumps({'chromeMajor':155,'denoCoreVersion':'0.412.0'}))
            before, after = 'before\n', 'after\n'
            (source / '00_infra.js').write_text(before)
            patch = ''.join(difflib.unified_diff(before.splitlines(True), after.splitlines(True), fromfile='a/00_infra.js', tofile='b/00_infra.js'))
            (helper / 'async-admission.patch').write_text(patch)
            (helper / 'async-admission-sources.json').write_text(json.dumps({'00_infra.js': {
                'before':hashlib.sha256(before.encode()).hexdigest(),
                'after':hashlib.sha256(after.encode()).hexdigest(),
            }}))
            command = [sys.executable, str(helper / 'apply_async_admission.py'), str(sdk)]
            subprocess.run([*command, '--record-provenance'], check=True, capture_output=True)
            subprocess.run([*command, '--record-provenance'], check=True, capture_output=True)
            self.assertEqual((source / '00_infra.js').read_text(), after)
            self.assertIn('asyncAdmissionTransaction', json.loads((sdk / 'provenance.json').read_text()))
            # Matching source alone must not attest a different or malformed patch.
            (helper / 'async-admission.patch').write_text('invalid patch\n')
            failed = subprocess.run([*command, '--record-provenance'], capture_output=True)
            self.assertNotEqual(failed.returncode, 0)
            (helper / 'async-admission.patch').write_text(patch)
            subprocess.run([*command, '--reverse'], check=True, capture_output=True)
            self.assertEqual((source / '00_infra.js').read_text(), before)
            self.assertNotIn('asyncAdmissionTransaction', json.loads((sdk / 'provenance.json').read_text()))
            (source / '00_infra.js').write_text('unknown\n')
            failed = subprocess.run(command, capture_output=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual((source / '00_infra.js').read_text(), 'unknown\n')

if __name__ == '__main__':
    unittest.main()
