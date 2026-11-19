#!/usr/bin/env python3
"""Artifact routing/guard controls: executable protocol fixtures, not engine proof."""
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('prepare_npm_release', Path(__file__).with_name('prepare-npm-release.py'))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
VERSION = '0.8.4'
COMMIT = 'a' * 40
CLI = r'''const path = require('node:path');
const {spawnSync}=require('node:child_process');
const child=spawnSync(path.join(__dirname,'node_modules/@miaskiewicz/turbo-surf-linux-x64/turbo-surf-mcp'),[],{stdio:'inherit'});
process.exit(child.status === null ? 1 : child.status);
'''
FAKE = r'''#!/usr/bin/env python3
import json,sys,urllib.request
rows=[json.loads(line) for line in sys.stdin if line.strip()]
url=rows[1]['params']['arguments']['url']
with urllib.request.urlopen(url,timeout=5) as response:
 assert response.status==200 and b'portable-runtime' in response.read()
def tool(number,value):return {'jsonrpc':'2.0','id':number,'result':{'content':[{'type':'text','text':json.dumps(value)}]}}
answers=[{'jsonrpc':'2.0','id':1,'result':{'serverInfo':{'name':'turbo-surf','version':'0.8.4'}}},tool(2,{'url':url,'status':200}),tool(3,{'text':'portable-runtime','sum':42,'crypto':True,'streams':True,'private':True})]
for answer in answers:print(json.dumps(answer),flush=True)
'''


def archive(root, name, files, binary_mode=0o755):
    path = root / (name + '.tgz')
    with tarfile.open(path, 'w:gz') as stream:
        for relative, content in files.items():
            data = content.encode()
            member = tarfile.TarInfo('package/' + relative)
            member.size = len(data)
            member.mode = binary_mode if relative == 'turbo-surf-mcp' else 0o644
            stream.addfile(member, io.BytesIO(data))
    return {'archive': str(path), 'sha256': MODULE.sha(path)}


class ArchiveRoutingTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='npm-packed-proof-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        self.checkout = self.root / 'checkout'
        self.checkout.mkdir()
        (self.checkout / 'cli.js').write_text(CLI)
        package = self.checkout / 'node_modules/@miaskiewicz/turbo-surf-linux-x64'
        package.mkdir(parents=True)
        (package / 'package.json').write_text(json.dumps({'name': '@miaskiewicz/turbo-surf-linux-x64', 'version': VERSION}))
        binary = package / 'turbo-surf-mcp'
        binary.write_text(FAKE)
        binary.chmod(0o755)
        self.platform = archive(self.root, 'platform', {'package.json': json.dumps({'name': '@miaskiewicz/turbo-surf-linux-x64', 'version': VERSION}), 'turbo-surf-mcp': FAKE})

    def test_exact_tarball_runs_when_checkout_launcher_is_poisoned_after_pack(self):
        thin = archive(self.root, 'thin-good', {'package.json': json.dumps({'name': 'turbo-surf', 'version': VERSION}), 'cli.js': CLI})
        (self.checkout / 'cli.js').write_text('process.exit(91);')
        output = self.root / 'proof-good'
        output.mkdir()
        result = MODULE.smoke_tarballs(thin, self.platform, output, VERSION, COMMIT, self.checkout)
        self.assertTrue(result['receipt']['javascriptVerified'])
        self.assertTrue(Path(result['entrypoint']).is_relative_to(output / 'installed-launcher'))
        self.assertEqual((self.checkout / 'cli.js').read_text(), 'process.exit(91);')
        self.assertNotEqual(Path(result['receipt']['cwd']), self.checkout)

    def test_poisoned_packed_launcher_fails_even_when_checkout_launcher_is_healthy(self):
        thin = archive(self.root, 'thin-bad', {'package.json': json.dumps({'name': 'turbo-surf', 'version': VERSION}), 'cli.js': 'process.exit(92);'})
        output = self.root / 'proof-bad'
        output.mkdir()
        with self.assertRaisesRegex(ValueError, 'Actual packed launcher'):
            MODULE.smoke_tarballs(thin, self.platform, output, VERSION, COMMIT, self.checkout)
        self.assertEqual((self.checkout / 'cli.js').read_text(), CLI)

    def test_changed_tarball_is_rejected_before_it_can_execute(self):
        thin = archive(self.root, 'thin-changed', {'cli.js': CLI})
        Path(thin['archive']).write_bytes(b'not the verified archive')
        output = self.root / 'changed-proof'
        output.mkdir()
        with self.assertRaisesRegex(ValueError, 'changed before execution'):
            MODULE.smoke_tarballs(thin, self.platform, output, VERSION, COMMIT, self.checkout)
        self.assertFalse((output / 'installed-launcher').exists())

    def test_nonexecutable_packed_binary_is_rejected_without_repair(self):
        thin = archive(self.root, 'thin-mode', {'package.json': json.dumps({'name': 'turbo-surf', 'version': VERSION}), 'cli.js': CLI})
        broken = archive(self.root, 'platform-mode', {'package.json': json.dumps({'name': '@miaskiewicz/turbo-surf-linux-x64', 'version': VERSION}), 'turbo-surf-mcp': FAKE}, binary_mode=0o644)
        output = self.root / 'mode-proof'
        output.mkdir()
        with self.assertRaisesRegex(ValueError, 'executable mode'):
            MODULE.smoke_tarballs(thin, broken, output, VERSION, COMMIT, self.checkout)
        installed = output / 'installed-launcher/package/node_modules/@miaskiewicz/turbo-surf-linux-x64/turbo-surf-mcp'
        self.assertEqual(installed.stat().st_mode & 0o777, 0o644)

    def test_actual_npm_pack_rejects_nonexecutable_unix_payload(self):
        for platform in ('darwin-arm64', 'linux-x64', 'linux-arm64'):
            with self.subTest(platform=platform):
                source = self.root / ('bad-mode-' + platform)
                source.mkdir()
                name = '@miaskiewicz/turbo-surf-' + platform
                (source / 'package.json').write_text(json.dumps({'name': name, 'version': VERSION, 'files': ['turbo-surf-mcp']}))
                binary = source / 'turbo-surf-mcp'
                binary.write_text(FAKE)
                binary.chmod(0o644)
                with self.assertRaisesRegex(ValueError, 'executable mode'):
                    MODULE.pack(source, self.root / ('bad-packed-' + platform), name, VERSION, {'package.json', 'turbo-surf-mcp'})

    def test_downloaded_modes_are_fixed_only_before_packing_unix_platforms(self):
        artifacts = self.root / 'artifacts'
        root = self.root / 'platform-root'
        root.mkdir()
        (root / 'LICENSE').write_text('fixture license')
        (root / 'THIRD_PARTY_NOTICES.md').write_text('fixture notices')
        output = self.root / 'staged'
        for platform, (os_name, arch, binary_name, addon_name) in MODULE.PLATFORMS.items():
            with self.subTest(platform=platform):
                metadata = root / 'npm' / platform
                metadata.mkdir(parents=True)
                (metadata / 'package.json').write_text(json.dumps({'name': '@miaskiewicz/turbo-surf-' + platform, 'version': VERSION, 'os': [os_name], 'cpu': [arch], 'files': [binary_name, addon_name, 'LICENSE', 'THIRD_PARTY_NOTICES.md']}))
                artifact = artifacts / ('bin-' + platform)
                artifact.mkdir(parents=True)
                binary = artifact / ('turbo-surf-mcp-' + platform + ('.exe' if os_name == 'win32' else ''))
                addon = artifact / addon_name
                binary.write_bytes(b'actual-downloaded-binary-fixture')
                addon.write_bytes(b'actual-downloaded-addon-fixture')
                binary.chmod(0o644)
                mcp = {'sourceCommit': COMMIT, 'expectedVersion': VERSION, 'initializeVerified': True, 'javascriptVerified': True, 'artifactUnchanged': True, 'commandFileSha256': {'producer': MODULE.sha(binary)}}
                napi = {'passed': True, 'sourceCommit': COMMIT, 'expectedVersion': VERSION, 'addonSha256': MODULE.sha(addon), 'platform': os_name, 'arch': arch, 'result': {'answer': 42, 'text': 'Native 42', 'crypto': True, 'stream': True, 'private': True}}
                (artifact / 'mcp-smoke.json').write_text(json.dumps(mcp))
                (artifact / 'napi-smoke.json').write_text(json.dumps(napi))
                result = MODULE.prepare_platform(root, artifacts, output, platform, VERSION, COMMIT)
                with tarfile.open(result['archive']) as packed:
                    self.assertEqual(packed.getmember('package/' + binary_name).mode & 0o777, 0o644 if os_name == 'win32' else 0o755)
                    self.assertEqual(packed.extractfile('package/' + binary_name).read(), binary.read_bytes())
                    self.assertEqual(packed.extractfile('package/' + addon_name).read(), addon.read_bytes())
                self.assertEqual(binary.stat().st_mode & 0o777, 0o644)

    def test_actual_npm_pack_binds_bytes_and_skips_package_scripts(self):
        source = self.root / 'pack-source'
        source.mkdir()
        metadata = {'name': 'turbo-surf', 'version': VERSION, 'files': ['cli.js'], 'scripts': {'prepack': "node -e \"require('fs').writeFileSync('forbidden','ran')\""}}
        (source / 'package.json').write_text(json.dumps(metadata))
        (source / 'cli.js').write_text(CLI)
        result = MODULE.pack(source, self.root / 'actual-packed', 'turbo-surf', VERSION, {'package.json', 'cli.js'})
        self.assertEqual(result['files']['cli.js'], MODULE.sha(source / 'cli.js'))
        self.assertEqual(result['sha256'], MODULE.sha(result['archive']))
        self.assertFalse((source / 'forbidden').exists())

    def test_escaping_or_duplicate_tar_entries_fail_before_execution(self):
        for name, rows in [('escape', [('package/../../outside', b'bad')]), ('duplicate', [('package/cli.js', b'first'), ('package/cli.js', b'second')])]:
            target = self.root / (name + '.tgz')
            with tarfile.open(target, 'w:gz') as stream:
                for path, data in rows:
                    info = tarfile.TarInfo(path)
                    info.size = len(data)
                    stream.addfile(info, io.BytesIO(data))
            with self.assertRaises(ValueError):
                MODULE.extract(target, self.root / ('extract-' + name))
        self.assertFalse((self.root / 'outside').exists())

    def test_build_receipts_bind_both_actual_native_files_and_runtime_results(self):
        platform = 'linux-x64'
        artifact = self.root / 'native-artifact'
        artifact.mkdir()
        binary = artifact / 'turbo-surf-mcp-linux-x64'
        addon = artifact / 'turbo-surf.linux-x64-gnu.node'
        binary.write_bytes(b'fixture-binary-not-executed')
        addon.write_bytes(b'fixture-addon-not-executed')
        mcp = {'sourceCommit': COMMIT, 'expectedVersion': VERSION, 'initializeVerified': True, 'javascriptVerified': True, 'artifactUnchanged': True, 'commandFileSha256': {'producer-binary': MODULE.sha(binary)}}
        napi = {'passed': True, 'sourceCommit': COMMIT, 'expectedVersion': VERSION, 'addonSha256': MODULE.sha(addon), 'platform': 'linux', 'arch': 'x64', 'result': {'answer': 42, 'text': 'Native 42', 'crypto': True, 'stream': True, 'private': True}}
        (artifact / 'mcp-smoke.json').write_text(json.dumps(mcp))
        (artifact / 'napi-smoke.json').write_text(json.dumps(napi))
        self.assertEqual(MODULE.verify_build(artifact, platform, VERSION, COMMIT)[:2], (binary, addon))
        napi['result']['crypto'] = False
        (artifact / 'napi-smoke.json').write_text(json.dumps(napi))
        with self.assertRaisesRegex(ValueError, 'Node ABI/runtime'):
            MODULE.verify_build(artifact, platform, VERSION, COMMIT)
        napi['result']['crypto'] = True
        (artifact / 'napi-smoke.json').write_text(json.dumps(napi))
        addon.write_bytes(b'another-addon')
        with self.assertRaisesRegex(ValueError, 'Node ABI/runtime'):
            MODULE.verify_build(artifact, platform, VERSION, COMMIT)
        addon.write_bytes(b'fixture-addon-not-executed')
        mcp['javascriptVerified'] = False
        (artifact / 'mcp-smoke.json').write_text(json.dumps(mcp))
        with self.assertRaisesRegex(ValueError, 'MCP artifact'):
            MODULE.verify_build(artifact, platform, VERSION, COMMIT)

    def test_manual_dispatch_never_supplies_a_publishable_artifact(self):
        for event, ref in [('workflow_dispatch', 'refs/heads/release/v0.8.4'), ('workflow_dispatch', 'refs/tags/v0.8.4'), ('push', 'refs/heads/release/v0.8.4'), ('push', 'refs/tags/v0.8.3')]:
            with self.subTest(event=event, ref=ref), self.assertRaisesRegex(ValueError, 'actual push of the exact'):
                MODULE.publication_archive(self.root / 'absent.json', 'thin', event, ref, VERSION, COMMIT)

    def test_publication_rechecks_all_five_exact_archives(self):
        records = {}
        for platform in ['thin', *MODULE.PLATFORMS]:
            file = self.root / (platform + '.tgz')
            file.write_bytes(platform.encode())
            records[platform] = {'archive': file.name, 'sha256': MODULE.sha(file)}
        receipt = {'passed': True, 'allNativeBuildsVerified': True, 'sourceCommit': COMMIT, 'version': VERSION, 'thin': records['thin'], 'platforms': {name: records[name] for name in MODULE.PLATFORMS}}
        path = self.root / 'FINAL.json'
        path.write_text(json.dumps(receipt))
        self.assertEqual(MODULE.publication_archive(path, 'thin', 'push', 'refs/tags/v' + VERSION, VERSION, COMMIT), self.root / 'thin.tgz')
        (self.root / 'win32-x64.tgz').write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError, 'archive changed'):
            MODULE.publication_archive(path, 'thin', 'push', 'refs/tags/v' + VERSION, VERSION, COMMIT)


class WorkflowGuards(unittest.TestCase):
    def test_all_npm_uploads_wait_for_actual_assembled_proof_and_tag_push(self):
        text = Path(__file__).resolve().parents[1].joinpath('.github/workflows/release.yml').read_text()
        jobs = {}
        current = None
        for line in text.splitlines():
            if line.startswith('  ') and not line.startswith('   ') and line.rstrip().endswith(':'):
                current = line.strip()[:-1]
                jobs[current] = []
            if current:
                jobs[current].append(line)
        proof = '\n'.join(jobs['prepare-packages'])
        self.assertIn('needs: build', proof)
        self.assertIn('scripts/prepare-npm-release.py', proof)
        for name in ('publish-platform', 'publish'):
            job = '\n'.join(jobs[name])
            self.assertIn("if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')", job)
            self.assertIn('prepare-packages', job)
            self.assertIn('--publication-receipt', job)
            self.assertIn('npm publish "$RELEASE_TARBALL"', job)
        for name, lines in jobs.items():
            if any('npm publish ' in line for line in lines):
                self.assertIn(name, ('publish-platform', 'publish'))


if __name__ == '__main__':
    unittest.main()
