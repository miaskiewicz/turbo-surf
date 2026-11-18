#!/usr/bin/env python3
"""Pack, verify and smoke exact npm artifacts. Publication is a separate guarded job."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import signal
import subprocess
import tarfile

SPEC = importlib.util.spec_from_file_location('release_smoke', Path(__file__).with_name('smoke-mcp-release.py'))
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
PLATFORMS = {
    'darwin-arm64': ('darwin', 'arm64', 'turbo-surf-mcp', 'turbo-surf.darwin-arm64.node'),
    'linux-x64': ('linux', 'x64', 'turbo-surf-mcp', 'turbo-surf.linux-x64-gnu.node'),
    'linux-arm64': ('linux', 'arm64', 'turbo-surf-mcp', 'turbo-surf.linux-arm64-gnu.node'),
    'win32-x64': ('win32', 'x64', 'turbo-surf-mcp.exe', 'turbo-surf.win32-x64-msvc.node'),
}


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def members(archive):
    rows = archive.getmembers()
    names = [row.name for row in rows]
    if len(names) != len(set(names)):
        raise ValueError('Duplicate npm archive entries')
    for row in rows:
        name = PurePosixPath(row.name)
        if name.is_absolute() or '..' in name.parts or '\\' in row.name or not name.parts or name.parts[0] != 'package' or not (row.isfile() or row.isdir()):
            raise ValueError('Unsafe npm archive entry: ' + row.name)
    return rows


def pack(source, output, expected_name, version, required):
    output.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(['npm', 'pack', '--json', '--ignore-scripts', '--pack-destination', str(output)], cwd=source, capture_output=True, text=True, timeout=180)
    (output / 'npm-pack.stdout.json').write_text(result.stdout)
    (output / 'npm-pack.stderr.log').write_text(result.stderr)
    result.check_returncode()
    rows = json.loads(result.stdout)
    if len(rows) != 1 or rows[0]['name'] != expected_name or rows[0]['version'] != version:
        raise ValueError('Wrong actual npm package identity')
    archive = output / rows[0]['filename']
    data = archive.read_bytes()
    integrity = 'sha512-' + base64.b64encode(hashlib.sha512(data).digest()).decode()
    if rows[0]['integrity'] != integrity or rows[0]['shasum'] != hashlib.sha1(data).hexdigest():
        raise ValueError('Actual npm archive integrity differs')
    hashes = {}
    with tarfile.open(archive, 'r:gz') as stream:
        rows = members(stream)
        files = {row.name.removeprefix('package/') for row in rows if row.isfile()}
        if not set(required) <= files:
            raise ValueError('Actual npm archive lacks required payload: ' + str(set(required) - files))
        if expected_name == 'turbo-surf' and any(name.endswith('.node') or name.startswith('bin/turbo-surf-mcp') for name in files):
            raise ValueError('Thin launcher unexpectedly bundles a native artifact')
        identity = json.loads(stream.extractfile('package/package.json').read())
        if identity['name'] != expected_name or identity['version'] != version:
            raise ValueError('Actual archive metadata differs')
        for name in sorted(required):
            payload = stream.extractfile('package/' + name).read()
            if payload != (source / name).read_bytes():
                raise ValueError('Actual archived payload differs: ' + name)
            hashes[name] = hashlib.sha256(payload).hexdigest()
    return {'archive': str(archive), 'name': expected_name, 'version': version, 'sha256': sha(archive), 'integrity': integrity, 'files': hashes}


def extract(archive, destination):
    destination.mkdir(parents=True)
    with tarfile.open(archive, 'r:gz') as stream:
        members(stream)
        stream.extractall(destination, filter='data')
    return destination / 'package'


def verify_build(artifact, platform, version, commit):
    os_name, arch, binary_name, addon_name = PLATFORMS[platform]
    binary = artifact / ('turbo-surf-mcp-' + platform + ('.exe' if platform == 'win32-x64' else ''))
    addon = artifact / addon_name
    for file in (binary, addon):
        if not file.is_file() or file.is_symlink():
            raise ValueError('Missing actual native build artifact: ' + str(file))
    mcp = json.loads((artifact / 'mcp-smoke.json').read_text())
    napi = json.loads((artifact / 'napi-smoke.json').read_text())
    if mcp.get('sourceCommit') != commit or mcp.get('expectedVersion') != version or not all(mcp.get(key) is True for key in ('initializeVerified', 'javascriptVerified', 'artifactUnchanged')):
        raise ValueError('Native MCP artifact did not pass its actual runtime smoke')
    if sha(binary) not in mcp.get('commandFileSha256', {}).values():
        raise ValueError('MCP runtime proof belongs to another binary')
    expected = {'answer': 42, 'text': 'Native 42', 'crypto': True, 'stream': True, 'private': True}
    if napi.get('passed') is not True or napi.get('sourceCommit') != commit or napi.get('expectedVersion') != version or napi.get('addonSha256') != sha(addon) or napi.get('result') != expected or (napi.get('platform'), napi.get('arch')) != (os_name, arch):
        raise ValueError('Native addon artifact did not pass its actual Node ABI/runtime smoke')
    return binary, addon, {'mcpReceiptSha256': sha(artifact / 'mcp-smoke.json'), 'napiReceiptSha256': sha(artifact / 'napi-smoke.json')}


def prepare_platform(root, artifacts, output, platform, version, commit):
    binary, addon, proof = verify_build(artifacts / ('bin-' + platform), platform, version, commit)
    source = output / 'platform-inputs' / platform
    source.mkdir(parents=True)
    for name in ('package.json', 'README.md'):
        original = root / 'npm' / platform / name
        if original.is_file():
            shutil.copy2(original, source / name)
    metadata = json.loads((source / 'package.json').read_text())
    if metadata.get('os') != [PLATFORMS[platform][0]] or metadata.get('cpu') != [PLATFORMS[platform][1]]:
        raise ValueError('Platform package metadata differs')
    shutil.copy2(binary, source / PLATFORMS[platform][2])
    shutil.copy2(addon, source / PLATFORMS[platform][3])
    for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md'):
        shutil.copy2(root / name, source / name)
    required = {'package.json', *metadata['files']}
    receipt = pack(source, output / 'platform-packages' / platform, '@miaskiewicz/turbo-surf-' + platform, version, required)
    receipt['nativeBuildProof'] = proof
    return receipt


def smoke_tarballs(thin, platform, output, version, commit, checkout):
    for record in (thin, platform):
        if sha(record['archive']) != record['sha256']:
            raise ValueError('Actual npm archive changed before execution')
    installed = extract(thin['archive'], output / 'installed-launcher')
    dependency = extract(platform['archive'], output / 'installed-platform')
    target = installed / 'node_modules' / '@miaskiewicz' / 'turbo-surf-linux-x64'
    target.parent.mkdir(parents=True)
    shutil.move(str(dependency), target)
    binary = target / PLATFORMS['linux-x64'][2]
    binary.chmod(binary.stat().st_mode | 0o111)
    # Only the extracted archive is allowed to supply the consumer entrypoint.
    cli = installed / 'cli.js'
    if cli.is_relative_to(checkout):
        raise ValueError('Packed consumer must be outside the checkout')
    receipt = SMOKE.smoke([shutil.which('node'), str(cli)], version, commit, output / 'private-consumer-cwd', output / 'javascript-smoke')
    if not receipt.get('initializeVerified') or not receipt.get('javascriptVerified'):
        raise ValueError('Actual packed launcher failed its runtime smoke')
    if sha(thin['archive']) != thin['sha256'] or sha(platform['archive']) != platform['sha256']:
        raise ValueError('Actual npm archive changed during smoke')
    return {'entrypoint': str(cli), 'extractedPackage': str(installed), 'platformPackage': str(target), 'receipt': receipt}


def prepare(root, artifacts, output, version, commit):
    output.mkdir(parents=True)
    metadata = json.loads((root / 'package.json').read_text())
    if metadata['name'] != 'turbo-surf' or metadata['version'] != version:
        raise ValueError('Wrong release source metadata')
    platforms = {name: prepare_platform(root, artifacts, output, name, version, commit) for name in PLATFORMS}
    scope = json.loads((root / 'scripts/js-coverage-scope.json').read_text())['files']
    required = set(scope) | {'package.json', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'README.md', 'CHANGELOG.md'}
    required.update('rust/crates/turbo-surf-napi/' + name for name in ('index.d.ts', 'package.json', 'LICENSE', 'README.md'))
    thin = pack(root, output / 'thin-launcher', 'turbo-surf', version, required)
    smoke = smoke_tarballs(thin, platforms['linux-x64'], output, version, commit, root)
    napi = output / 'packaged-napi-smoke.json'
    subprocess.run(['node', str(root / 'scripts/smoke-napi-release.mjs'), str(Path(smoke['platformPackage']) / PLATFORMS['linux-x64'][3]), version, commit, str(napi)], check=True, timeout=60)
    for record in [thin, *platforms.values()]:
        record['archive'] = str(Path(record['archive']).relative_to(output))
    receipt = {'passed': True, 'sourceCommit': commit, 'version': version, 'thin': thin, 'platforms': platforms, 'consumer': smoke, 'packagedNapiReceiptSha256': sha(napi), 'allNativeBuildsVerified': True}
    write(output / 'FINAL.json', receipt)
    return receipt


def publication_archive(receipt_path, platform, event, ref, version, commit):
    if event != 'push' or ref != 'refs/tags/v' + version:
        raise ValueError('Publication requires an actual push of the exact release tag')
    receipt = json.loads(receipt_path.read_text())
    if receipt.get('passed') is not True or receipt.get('allNativeBuildsVerified') is not True or receipt.get('sourceCommit') != commit or receipt.get('version') != version or set(receipt.get('platforms', {})) != set(PLATFORMS):
        raise ValueError('Complete matching release artifact proof is required')
    records = [receipt['thin'], *receipt['platforms'].values()]
    for record in records:
        relative = PurePosixPath(record['archive'])
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError('Escaping release archive path')
        if sha(receipt_path.parent / relative) != record['sha256']:
            raise ValueError('Verified release archive changed')
    return receipt_path.parent / (receipt['thin'] if platform == 'thin' else receipt['platforms'][platform])['archive']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path.cwd())
    parser.add_argument('--artifacts', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--version', required=True)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--publication-receipt', type=Path)
    parser.add_argument('--platform', choices=['thin', *PLATFORMS])
    parser.add_argument('--event')
    parser.add_argument('--ref')
    args = parser.parse_args()
    if args.publication_receipt:
        archive = publication_archive(args.publication_receipt.resolve(), args.platform, args.event, args.ref, args.version, args.source_commit)
        with Path(os.environ['GITHUB_ENV']).open('a') as stream:
            stream.write('RELEASE_TARBALL=' + str(archive) + '\n')
    else:
        for name in ('SIGINT', 'SIGTERM', 'SIGHUP'):
            if hasattr(signal, name):
                signal.signal(getattr(signal, name), SMOKE.interrupted)
        print(json.dumps(prepare(args.root.resolve(), args.artifacts.resolve(), args.output.resolve(), args.version, args.source_commit), indent=2))


if __name__ == '__main__':
    main()
