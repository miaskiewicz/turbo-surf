#!/usr/bin/env python3
"""Prepare an isolated, revision-pinned native V8 SDK; never edits Cargo's cache."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

HERE = Path(__file__).resolve().parent
VERSIONS = [('deno_core', '0.412.0', 'deno_core'), ('deno_v8', '0.4.0', 'deno_v8'), ('v8', '152.2.0', 'v8-exact')]

def profile_for(major):
    name = '155-8059.79.json' if str(major) == '155' else str(major) + '.json'
    profile = json.loads((HERE / 'profiles' / name).read_text())
    if profile['chromeMajor'] != int(major):
        raise ValueError('Engine profile major mismatch')
    return profile

def verify_engine(engine, profile):
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=engine, text=True).strip()
    if revision != profile['engineRevision']:
        raise SystemExit('Engine checkout differs from the pinned source revision')
    version = (engine / 'include/v8-version.h').read_text()
    import re
    actual = '.'.join(re.search(r'#define ' + field + r' (\d+)', version)[1] for field in ['V8_MAJOR_VERSION', 'V8_MINOR_VERSION', 'V8_BUILD_NUMBER', 'V8_PATCH_LEVEL'])
    if actual != profile['engineVersion']:
        raise SystemExit('Engine version headers differ from the selected profile')
    deps = (engine / 'DEPS').read_text()
    for field in ['buildRevision', 'icuRevision', 'rustDependenciesRevision']:
        if profile[field] not in deps:
            raise SystemExit('Engine DEPS differ from ' + field)

def verify_engine_patch(engine):
    patch = HERE / 'patches/engine.patch'
    expected_paths = {line[len('+++ b/'):] for line in patch.read_text().splitlines() if line.startswith('+++ b/')}
    changed = set(subprocess.check_output(['git', 'diff', '--name-only', 'HEAD'], cwd=engine, text=True).splitlines())
    extra = subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard'], cwd=engine, text=True).splitlines()
    if changed != expected_paths or extra:
        raise SystemExit('Engine checkout contains changes outside the committed preparation patch')
    with tempfile.TemporaryDirectory(prefix='turbo-engine-verify-') as temporary:
        baseline = Path(temporary)
        for name in expected_paths:
            target = baseline / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(subprocess.check_output(['git', 'show', 'HEAD:' + name], cwd=engine))
        subprocess.run(['patch', '--batch', '--forward', '--fuzz=0', '-p1', '-i', str(patch)], cwd=baseline, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        for name in expected_paths:
            if (engine / name).read_bytes() != (baseline / name).read_bytes():
                raise SystemExit('Engine source differs from the committed preparation patch: ' + name)

def install_platform_artifacts(root, profile, cache):
    variants = profile.get('libclangPackages')
    if not variants:
        return
    machine = {'aarch64':'arm64', 'amd64':'x86_64'}.get(platform.machine().lower(), platform.machine().lower())
    key = platform.system().lower() + '-' + machine
    if key not in variants:
        raise SystemExit('No pinned upstream libclang package for ' + key)
    artifact = variants[key]
    archive = download(artifact['url'], cache / ('llvm-libclang-' + key + '-' + artifact['sha256'] + '.tar.xz'))
    if hashlib.sha256(archive.read_bytes()).hexdigest() != artifact['sha256']:
        raise SystemExit('Upstream libclang archive integrity mismatch')
    target = root / 'v8-exact/third_party/llvm-libclang'
    marker = target / '.turbo-source-artifact.json'
    if not marker.exists() or json.loads(marker.read_text()) != artifact:
        unpack(archive, target)
        marker.write_text(json.dumps(artifact, indent=2) + '\n')
    if not (target / 'bindings/python/clang/cindex.py').is_file():
        raise SystemExit('The pinned libclang package lacks its required Python bindings')

def run(*args, cwd=None):
    subprocess.run(args, cwd=cwd, check=True)

def download(url, target):
    if not target.exists():
        temporary = target.with_suffix(target.suffix + '.partial')
        with urllib.request.urlopen(url, timeout=120) as response, temporary.open('wb') as output:
            shutil.copyfileobj(response, output)
        temporary.replace(target)
    return target

def unpack(archive, target, strip=''):
    target.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as bundle:
        for member in bundle.getmembers():
            if strip:
                if not member.name.startswith(strip + '/'):
                    continue
                member.name = member.name[len(strip) + 1:]
            if not member.name:
                continue
            destination = (target / member.name).resolve()
            if not destination.is_relative_to(target.resolve()):
                raise ValueError('Archive path escapes destination')
            bundle.extract(member, target, filter='data')

def apply(target, name):
    run('patch', '--batch', '--forward', '-p1', '-i', str(HERE / 'patches' / name), cwd=target)

def verify_applied_patch(target, name):
    result = subprocess.run(['patch', '--dry-run', '--batch', '--reverse', '--fuzz=0', '-p1', '-i', str(HERE / 'patches' / name)], cwd=target, capture_output=True, text=True)
    if result.returncode:
        raise SystemExit('Prepared source does not match ' + name + ':\n' + result.stdout + result.stderr)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--profile', choices=['154', '155'], default='154')
    parser.add_argument('--profile-file', type=Path, help='Use an additional revision-pinned profile with the selected Chrome major')
    parser.add_argument('--cache', type=Path, default=Path.home() / '.cache' / 'turbo-surf-engine')
    parser.add_argument('--adopt', action='store_true', help='Verify and record an existing prepared experiment')
    args = parser.parse_args()
    profile = json.loads(args.profile_file.read_text()) if args.profile_file else profile_for(args.profile)
    if profile['chromeMajor'] != int(args.profile):
        raise ValueError('Engine profile major mismatch')
    root = args.destination.resolve()
    args.cache.mkdir(parents=True, exist_ok=True)
    if not args.adopt:
        if root.exists() and any(root.iterdir()):
            raise SystemExit('Use a new empty destination; existing files are never replaced.')
        root.mkdir(parents=True, exist_ok=True)
        hashes = json.loads((HERE / 'crates.json').read_text())
        for name, version, target in VERSIONS:
            label = name + '-' + version
            archive = download(f'https://static.crates.io/crates/{name}/{label}.crate', args.cache / (label + '.crate'))
            if hashlib.sha256(archive.read_bytes()).hexdigest() != hashes[label]:
                raise SystemExit('Crate integrity mismatch: ' + label)
            unpack(archive, root / target, label)
        apply(root / 'deno_core', 'deno_core.patch')
        apply(root / 'v8-exact', 'sdk.patch')
        apply(root / 'v8-exact', 'code_generation.patch')
        engine = root / 'v8-exact' / 'v8'
        shutil.rmtree(engine)
        run('git', 'init', str(engine))
        run('git', 'remote', 'add', 'origin', 'https://chromium.googlesource.com/v8/v8', cwd=engine)
        run('git', 'fetch', '--depth=1', 'origin', profile['engineRevision'], cwd=engine)
        run('git', 'checkout', '--detach', profile['engineRevision'], cwd=engine)
        apply(engine, 'engine.patch')
        build_revision, icu_revision, rust_revision = (profile[field] for field in ['buildRevision', 'icuRevision', 'rustDependenciesRevision'])
        dependencies = [
            (f'https://chromium.googlesource.com/chromium/src/build/+archive/{build_revision}/rust/std/rules.tar.gz', root / 'v8-exact/build/rust/std/rules', 'std-rules-' + build_revision),
            (f'https://chromium.googlesource.com/chromium/deps/icu/+archive/{icu_revision}/common.tar.gz', root / 'v8-exact/third_party/icu/common', 'icu-common-' + icu_revision),
            (f'https://chromium.googlesource.com/chromium/src/third_party/rust/+archive/{rust_revision}.tar.gz', root / 'v8-exact/third_party/rust', 'rust-deps-' + rust_revision),
        ]
        for url, target, label in dependencies:
            unpack(download(url, args.cache / (label + '.tar.gz')), target)
        if platform.system() == 'Darwin' and not shutil.which('xcodebuild'):
            apply(root / 'v8-exact', 'clt.patch')
            shutil.copyfile(HERE / 'clt_sdk_info.py', root / 'v8-exact/build/config/apple/clt_sdk_info.py')
        elif platform.system() == 'Darwin':
            # xcodebuild may be present while only Command Line Tools are selected.
            developer = subprocess.check_output(['xcode-select', '-p'], text=True).strip()
            if 'CommandLineTools' in developer:
                apply(root / 'v8-exact', 'clt.patch')
                shutil.copyfile(HERE / 'clt_sdk_info.py', root / 'v8-exact/build/config/apple/clt_sdk_info.py')
        facade = root / 'deno_v8/Cargo.toml'
        source = facade.read_text()
        start = source.index('[dependencies.rusty_v8]')
        end = source.index('\n[', start + 1)
        source = source[:start] + '[dependencies.rusty_v8]\npath = "../v8-exact"\noptional = true\ndefault-features = false\npackage = "v8"\n' + source[end:]
        facade.write_text(source)
        shutil.copyfile(HERE / 'smoke.toml', root / 'Cargo.toml')
        (root / 'src').mkdir()
        shutil.copyfile(HERE / 'smoke.rs', root / 'src/main.rs')
    (root / 'tests').mkdir(exist_ok=True)
    install_platform_artifacts(root, profile, args.cache)
    shutil.copyfile(HERE / 'native_window_proxy.rs', root / 'tests/native_window_proxy.rs')
    shutil.copyfile(HERE / 'native_module_units.rs', root / 'tests/native_module_units.rs')
    shutil.copyfile(HERE / 'native_code_generation.rs', root / 'tests/native_code_generation.rs')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root / 'v8-exact/v8', text=True).strip()
    if revision != profile['engineRevision']:
        raise SystemExit('Engine checkout does not match the pinned revision')
    verify_engine(root / 'v8-exact/v8', profile)
    verify_engine_patch(root / 'v8-exact/v8')
    for target, name in [(root / 'deno_core', 'deno_core.patch'), (root / 'v8-exact', 'sdk.patch'), (root / 'v8-exact', 'code_generation.patch'), (root / 'v8-exact/v8', 'engine.patch')]:
        verify_applied_patch(target, name)
    if (root / 'v8-exact/build/config/apple/clt_sdk_info.py').exists():
        verify_applied_patch(root / 'v8-exact', 'clt.patch')
        if (root / 'v8-exact/build/config/apple/clt_sdk_info.py').read_bytes() != (HERE / 'clt_sdk_info.py').read_bytes():
            raise SystemExit('Prepared CLT adapter differs from the saved source')
    config = '[patch.crates-io]\n' + ''.join(name + ' = { path = ' + json.dumps(str(root / name)) + ' }\n' for name in ['deno_core', 'deno_v8'])
    (root / 'cargo-config.toml').write_text(config)
    (root / 'provenance.json').write_text(json.dumps({**profile, 'sdkVersion': '152.2.0', 'denoCoreVersion': '0.412.0', 'patches': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (HERE / 'patches').glob('*.patch')}}, indent=2) + '\n')
    print(root / 'cargo-config.toml')

if __name__ == '__main__':
    main()
