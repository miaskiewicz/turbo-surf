#!/usr/bin/env python3
"""Add a checksum-verified registry archive to a private preparation stage."""

import argparse
import hashlib
import importlib.util
import json
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('stage', ROOT / 'stage.py')
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('name')
    parser.add_argument('version')
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--downloads', type=Path, required=True)
    args = parser.parse_args()
    name = args.name
    if not name or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789_-' for c in name):
        raise ValueError('Invalid registry package name')
    key = '1/' + name if len(name) == 1 else '2/' + name if len(name) == 2 else '3/' + name[0] + '/' + name if len(name) == 3 else name[:2] + '/' + name[2:4] + '/' + name
    request = urllib.request.Request('https://index.crates.io/' + key, headers={'User-Agent': 'turbo-surf-release-preflight'})
    with urllib.request.urlopen(request, timeout=30) as response:
        versions = [json.loads(line) for line in response.read().decode().splitlines()]
    version = next(v for v in versions if v['vers'] == args.version)
    if version.get('yanked'):
        raise ValueError('Refusing a yanked dependency')
    url = f'https://static.crates.io/crates/{name}/{name}-{args.version}.crate'
    with urllib.request.urlopen(url, timeout=60) as response:
        payload = response.read()
    digest = hashlib.sha256(payload).hexdigest()
    if digest != version['cksum']:
        raise ValueError('Registry archive checksum mismatch')
    args.downloads.mkdir(parents=True, exist_ok=True)
    archive = args.downloads / f'{name}-{args.version}.crate'
    with archive.open('xb') as stream:
        stream.write(payload)
    result = stage.stage_archive(archive, args.stage / 'sources')
    receipt = {'name': name, 'version': args.version, 'sourceUrl': url, 'archiveSha256': digest,
               'registryChecksumMatched': True, 'candidateStage': str(args.stage), 'files': len(result['files'])}
    (args.downloads / f'{name}-{args.version}.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
