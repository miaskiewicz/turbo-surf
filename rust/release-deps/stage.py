#!/usr/bin/env python3
"""Stage candidate distributions as a directory source without Cargo patches.

This is a prepublication check, not proof that crates.io serves these packages.
The staging helper never writes cache files. Links retain their original source
permissions; a build executor must verify the stage before and after execution.
"""

import argparse
import hashlib
import importlib.util
import json
import shutil
import tarfile
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('transport_prepare', ROOT / 'transport/prepare.py')
transport = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transport)


def normalize(manifest):
    d = tomllib.loads(manifest)
    for section in [d] + list(d.get('target', {}).values()):
        for key in ['dependencies', 'dev-dependencies', 'build-dependencies']:
            for dependency in section.get(key, {}).values():
                if isinstance(dependency, dict):
                    for removed in ['path', 'git', 'rev', 'branch', 'tag']:
                        dependency.pop(removed, None)
                    if dependency.get('workspace'):
                        raise ValueError('Flatten workspace dependencies before staging')
    if not isinstance(d['package']['version'], str):
        raise ValueError('Flatten workspace package version before staging')
    for key in ['workspace', 'patch', 'profile']:
        d.pop(key, None)
    return transport.dump(d)


def stage_candidate(source, directory, archive_sha=None):
    manifest = tomllib.loads((source / 'Cargo.toml').read_text())
    package = manifest['package']
    name, version = package['name'], package['version']
    for component in [name, version]:
        if not isinstance(component, str) or '/' in component or '\\' in component or component in ['.', '..']:
            raise ValueError('Invalid package identity')
    target = directory / (name + '-' + version)
    if target.exists():
        raise ValueError(f'Duplicate candidate identity: {name}-{version}')
    shutil.copytree(source, target, ignore=shutil.ignore_patterns('target', '.git', '.cargo-checksum.json', '.cargo-ok'))
    if archive_sha is None:
        (target / 'Cargo.toml').write_text(normalize((target / 'Cargo.toml').read_text()))
    checksums = transport.tree(target)
    (target / '.cargo-checksum.json').write_text(json.dumps({'files': checksums, 'package': archive_sha}, sort_keys=True) + '\n')
    return {'name': name, 'version': version, 'files': checksums, 'archiveSha256': archive_sha,
            'kind': 'actual-package-archive' if archive_sha else 'bootstrap-source-not-cargo-package'}


def stage_archive(archive, directory):
    with tempfile.TemporaryDirectory(prefix='turbo-surf-crate-stage-') as scratch:
        scratch = Path(scratch)
        with tarfile.open(archive) as tar:
            roots = {Path(m.name).parts[0] for m in tar.getmembers()}
            if len(roots) != 1:
                raise ValueError('Package archive must have one top-level directory')
            for m in tar.getmembers():
                if not (m.isdir() or m.isfile()) or m.name.startswith('/') or '..' in Path(m.name).parts:
                    raise ValueError('Unexpected or escaping package entry')
            tar.extractall(scratch, filter='data')
        return stage_candidate(scratch / roots.pop(), directory, transport.sha(archive))


def populate_cache(directory, cargo_home):
    linked = []
    for cache in sorted((cargo_home / 'registry/src').glob('*/*')):
        if not (cache / 'Cargo.toml').is_file():
            continue
        target = directory / cache.name
        if target.exists():
            continue
        archive = cargo_home / 'registry/cache' / cache.parent.name / (cache.name + '.crate')
        if not archive.is_file():
            continue
        target.mkdir()
        expected = {}
        tops = set()
        with tarfile.open(archive) as tar:
            for member in tar:
                relative = Path(member.name).relative_to(cache.name)
                if not relative.parts:
                    continue
                if '..' in relative.parts or not (member.isdir() or member.isfile()):
                    raise ValueError('Unexpected cached registry archive entry')
                tops.add(relative.parts[0])
                if member.isfile():
                    with tar.extractfile(member) as stream:
                        expected[relative.as_posix()] = hashlib.file_digest(stream, 'sha256').hexdigest()
        for name in sorted(tops):
            source = cache / name
            if not source.exists():
                raise ValueError(f'Cached source is missing archive input: {source}')
            (target / name).symlink_to(source.resolve(), target_is_directory=source.is_dir())
        (target / '.cargo-checksum.json').write_text(json.dumps({'files': expected, 'package': transport.sha(archive)}, sort_keys=True) + '\n')
        linked.append({'source': str(cache.resolve()), 'archiveSha256': transport.sha(archive), 'files': len(expected)})
    return linked



def verify_stage(directory):
    files = 0
    checked = hashlib.sha256()
    for package in sorted(directory.iterdir()):
        checksum = package / '.cargo-checksum.json'
        if not checksum.is_file():
            raise ValueError(f'Missing staged checksum record: {package.name}')
        expected = json.loads(checksum.read_text())['files']
        for name, digest in sorted(expected.items()):
            if Path(name).is_absolute() or '..' in Path(name).parts:
                raise ValueError('Escaping staged checksum path')
            actual = transport.sha(package / name)
            if actual != digest:
                raise ValueError(f'Stage source mismatch: {package.name}/{name}')
            checked.update((package.name + '/' + name + ':' + actual + '\n').encode())
            files += 1
    return {'files': files, 'sourceMapSha256': checked.hexdigest(), 'allExpectedFileBytesMatch': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--output', type=Path)
    mode.add_argument('--verify-stage', type=Path)
    parser.add_argument('--cargo-home', type=Path, default=Path.home() / '.cargo')
    parser.add_argument('--candidate-source', type=Path, action='append', default=[])
    parser.add_argument('--candidate-archive', type=Path, action='append', default=[])
    args = parser.parse_args()
    if args.verify_stage:
        print(json.dumps(verify_stage(args.verify_stage / 'sources')))
        return
    if args.output.exists():
        raise ValueError('Refusing to overwrite an existing stage')
    directory = args.output.resolve() / 'sources'
    directory.mkdir(parents=True)
    candidates = [stage_candidate(p, directory) for p in args.candidate_source]
    candidates += [stage_archive(p, directory) for p in args.candidate_archive]
    linked = populate_cache(directory, args.cargo_home)
    config = args.output / 'config.toml'
    config.write_text('[source.crates-io]\nreplace-with = "release-stage"\n\n[source.release-stage]\ndirectory = ' + json.dumps(str(directory)) + '\n')
    receipt = {'scope': 'Prepublication staged source; not a published registry-consumer verdict.',
               'candidates': candidates, 'cachedSourceLinks': linked,
               'workspacePatches': False,
               'cacheGuard': 'Not measured here; run --verify-stage before and after execution. Source link permissions are unchanged.'}
    (args.output / 'stage.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'candidates': len(candidates), 'cacheLinks': len(linked), 'config': str(config)}))


if __name__ == '__main__':
    main()
