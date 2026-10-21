#!/usr/bin/env python3
"""Prepare licensed transport distributions with explicit registry dependencies."""

import argparse
import copy
import fnmatch
import hashlib
import json
import posixpath
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PINS = {
    'wreq': ('https://github.com/greg-flux/wreq', '4ed3cf0cd4c14c58c557fdd3f3ff6afa4ae24762'),
    'wreq-util': ('https://github.com/0x676e67/wreq-util', '5715529002738f46eea86a7271f918f9374d3582'),
    'btls': ('https://github.com/0x676e67/btls', 'f5a8a483eca2e0b3f539f944604da3f8e14a910e'),
}
BORINGSSL = 'f1f2556a5dfa59e147d9d47279cc3f7f8a18b433'
PACKAGES = {'btls-sys': '0.5.6', 'btls': '0.5.6', 'tokio-btls': '0.5.6',
            'wreq': '0.16.1', 'wreq-util': '0.2.0'}


def git(source, *args):
    return subprocess.check_output(['git', '-C', str(source), *args])


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def tree(path):
    result = {}
    for p in sorted(path.rglob('*')):
        if p.is_symlink():
            raise ValueError(f'Unexpected source link: {p}')
        if p.is_file():
            result[p.relative_to(path).as_posix()] = sha(p)
    return result


def inline(value):
    if isinstance(value, dict):
        return '{' + ', '.join(json.dumps(k) + ' = ' + inline(v) for k, v in value.items()) + '}'
    if isinstance(value, list):
        return '[' + ', '.join(inline(v) for v in value) + ']'
    if isinstance(value, (str, int, float, bool)):
        return json.dumps(value, ensure_ascii=True)
    raise ValueError(f'Unsupported TOML value: {type(value)}')


def dump(value):
    result = '\n'.join(json.dumps(k) + ' = ' + inline(v) for k, v in value.items()) + '\n'
    assert tomllib.loads(result) == value
    return result


def inherited(value, workspace):
    if not isinstance(value, dict) or not value.get('workspace'):
        return copy.deepcopy(value)
    result = copy.deepcopy(workspace)
    extra = {k: v for k, v in value.items() if k != 'workspace'}
    if isinstance(result, str):
        result = {'version': result}
    if 'features' in extra:
        extra['features'] = list(dict.fromkeys(result.get('features', []) + extra['features']))
    result.update(extra)
    return result


def rewrite(manifest, workspace, old):
    d = copy.deepcopy(manifest)
    p = d['package']
    for key, value in list(p.items()):
        if isinstance(value, dict) and value.get('workspace'):
            p[key] = copy.deepcopy(workspace['package'][key])
    assert p['name'] == old and p['version'] == PACKAGES[old]
    upstream_repository = p['repository']
    p['name'] = 'turbo-surf-' + old
    p['repository'] = 'https://github.com/miaskiewicz/turbo-surf'
    p['description'] = 'Reviewed turbo-surf distribution of ' + old + ': ' + p['description']
    p.setdefault('metadata', {})['turbo-surf-upstream'] = {
        'name': old, 'version': PACKAGES[old], 'repository': upstream_repository,
        'source-revision': PINS['btls' if old in ['btls', 'btls-sys', 'tokio-btls'] else old][1],
    }
    if 'include' in p:
        p['include'] += ['/TURBO_SURF_UPSTREAM.json']
    d.setdefault('lib', {})['name'] = old.replace('-', '_')
    sections = [d] + list(d.get('target', {}).values())
    for section in sections:
        for category in ['dependencies', 'dev-dependencies', 'build-dependencies']:
            for alias, value in list(section.get(category, {}).items()):
                value = inherited(value, workspace.get('dependencies', {}).get(alias))
                package = value.get('package', alias) if isinstance(value, dict) else alias
                if package in PACKAGES:
                    if isinstance(value, str):
                        value = {'version': value}
                    value.update(package='turbo-surf-' + package, version='=' + PACKAGES[package],
                                 path='../turbo-surf-' + package)
                section[category][alias] = value
    for key in ['patch', 'workspace', 'profile']:
        d.pop(key, None)
    return d


def export(source, revision, destination, paths=None):
    destination.mkdir(parents=True)
    with tempfile.TemporaryFile() as stream:
        subprocess.run(['git', '-C', str(source), 'archive', revision, '--', *(paths or [])],
                       stdout=stream, check=True)
        stream.seek(0)
        with tarfile.open(fileobj=stream) as archive:
            for member in archive.getmembers():
                if not (member.isdir() or member.isfile() or member.issym() or member.islnk()):
                    raise ValueError(f'Unexpected Git archive member: {member.name}')
                if member.name.startswith('/') or '..' in Path(member.name).parts:
                    raise ValueError(f'Escaping Git archive member: {member.name}')
                if member.issym() or member.islnk():
                    joined = posixpath.normpath(posixpath.join(posixpath.dirname(member.name), member.linkname)) if member.issym() else posixpath.normpath(member.linkname)
                    if joined.startswith('../') or joined.startswith('/') or joined == '..':
                        raise ValueError(f'Escaping Git archive link: {member.name}')
            archive.extractall(destination, filter='data')


def matches(path, patterns):
    for pattern in patterns:
        variants = [pattern.lstrip('/')]
        while '**/' in variants[-1]:
            variants.append(variants[-1].replace('**/', '', 1))
        if any(fnmatch.fnmatchcase(path, variant) for variant in variants):
            return True
    return False


def find_source(cargo_home, name, revision):
    candidates = list((cargo_home / 'git/checkouts').glob(name + '-*/*'))
    for path in candidates:
        if (path / 'Cargo.toml').is_file() and git(path, 'rev-parse', 'HEAD').decode().strip() == revision:
            return path
    raise ValueError(f'Exact {name} Git checkout missing: {revision}; provide --{name}')


def fetch_sources(destination, cargo_home):
    destination = destination.resolve()
    if destination == cargo_home.resolve() or cargo_home.resolve() in destination.parents:
        raise ValueError('Source acquisition must not write into Cargo home')
    if destination.exists():
        raise ValueError('Refusing to overwrite existing source acquisition directory')
    destination.mkdir(parents=True)
    result = {}
    for name, (repository, revision) in PINS.items():
        target = destination / name
        subprocess.run(['git', 'init', '--quiet', str(target)], check=True)
        subprocess.run(['git', '-C', str(target), 'fetch', '--quiet', '--depth=1', repository, revision], check=True)
        subprocess.run(['git', '-C', str(target), 'checkout', '--quiet', '--detach', 'FETCH_HEAD'], check=True)
        result[name] = target
    subprocess.run(['git', '-C', str(result['btls']), 'submodule', 'update', '--init', '--depth=1', '--', 'btls-sys/deps/boringssl'], check=True)
    assert git(result['btls'] / 'btls-sys/deps/boringssl', 'rev-parse', 'HEAD').decode().strip() == BORINGSSL
    return result


def prepare(output, sources):
    if output.exists():
        raise ValueError('Refusing to overwrite output; choose a fresh directory')
    for name, source in sources.items():
        assert git(source, 'rev-parse', 'HEAD').decode().strip() == PINS[name][1]
    boringssl = sources['btls'] / 'btls-sys/deps/boringssl'
    assert git(boringssl, 'rev-parse', 'HEAD').decode().strip() == BORINGSSL
    output.mkdir(parents=True)
    receipt = {'scope': 'Prepared sources only; not compiled, published or registry-consumer verified.',
               'sources': {k: {'repository': v[0], 'revision': v[1]} for k, v in PINS.items()},
               'boringsslRevision': BORINGSSL, 'packages': []}
    with tempfile.TemporaryDirectory(prefix='turbo-surf-transport-') as scratch:
        exported = Path(scratch)
        for name, source in sources.items():
            export(source, PINS[name][1], exported / name)
        workspace = tomllib.loads((exported / 'btls/Cargo.toml').read_text())['workspace']
        for old, version in PACKAGES.items():
            original = exported / ('btls/' + old if old in ['btls', 'btls-sys', 'tokio-btls'] else old)
            target = output / 'packages' / ('turbo-surf-' + old)
            shutil.copytree(original, target)
            if old == 'btls-sys':
                include = tomllib.loads((target / 'Cargo.toml').read_text())['package']['include']
                names = git(boringssl, 'ls-tree', '-r', '--name-only', BORINGSSL).decode().splitlines()
                selected = [name for name in names if matches('deps/boringssl/' + name, include)]
                native = target / 'deps/boringssl'
                if native.exists():
                    native.rmdir()  # Git archives contain the empty submodule entry only.
                export(boringssl, BORINGSSL, native, selected)
                assert (native / 'LICENSE').is_file() and (native / 'CMakeLists.txt').is_file()
            before = tree(target)
            parsed = tomllib.loads((target / 'Cargo.toml').read_text())
            final = rewrite(parsed, workspace if old in ['btls', 'btls-sys', 'tokio-btls'] else {}, old)
            (target / 'Cargo.toml').write_text(dump(final))
            provenance = {'upstream': old, 'version': version, 'source': final['package']['metadata']['turbo-surf-upstream']}
            if old == 'btls-sys':
                provenance['boringsslRevision'] = BORINGSSL
            (target / 'TURBO_SURF_UPSTREAM.json').write_text(json.dumps(provenance, indent=2) + '\n')
            after = tree(target)
            changed = [k for k in before if before[k] != after[k]]
            assert changed == ['Cargo.toml'], changed
            assert set(after) - set(before) == {'TURBO_SURF_UPSTREAM.json'}
            receipt['packages'].append({'name': 'turbo-surf-' + old, 'version': version,
                                       'inputFiles': before, 'candidateFiles': after,
                                       'candidateBytes': sum(p.stat().st_size for p in target.rglob('*') if p.is_file())})
    members = ['packages/turbo-surf-' + name for name in PACKAGES]
    (output / 'Cargo.toml').write_text('[workspace]\nresolver = "2"\nmembers = ' + json.dumps(members) + '\n')
    (output / 'prepared-source-identity.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cargo-home', type=Path, default=Path.home() / '.cargo')
    parser.add_argument('--fetch-sources', type=Path)
    for name in PINS:
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    if args.fetch_sources:
        if any(getattr(args, name.replace('-', '_')) for name in PINS):
            parser.error('--fetch-sources cannot be combined with source checkout overrides')
        sources = fetch_sources(args.fetch_sources, args.cargo_home)
    else:
        sources = {name: getattr(args, name.replace('-', '_')) or find_source(args.cargo_home, name, pin[1])
                   for name, pin in PINS.items()}
    receipt = prepare(args.output, sources)
    print(json.dumps({'packages': len(receipt['packages']), 'sourceOnly': True,
                      'candidateBytes': sum(p['candidateBytes'] for p in receipt['packages'])}))


if __name__ == '__main__':
    main()
