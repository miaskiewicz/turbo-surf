#!/usr/bin/env python3
"""Run the exact offline admission controls with a verified managed native SDK."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('sdk', type=Path)
parser.add_argument('--mode', choices=['preimage', 'recovery', 'rejection', 'all'], default='all')
parser.add_argument('--toolchain', default='1.98.0')
args = parser.parse_args()
sdk = args.sdk.resolve()
here = Path(__file__).resolve().parent
artifact = json.loads((sdk / 'native-cache/artifact.json').read_text())
archive = sdk / 'native-cache/librusty_v8.a'
bindings = sdk / 'native-cache/src_binding.rs'
for path, field in [(archive, 'archiveSha256'), (bindings, 'bindingsSha256')]:
    if hashlib.sha256(path.read_bytes()).hexdigest() != artifact[field]:
        raise SystemExit('Managed native artifact hash mismatch: ' + path.name)
env = {**os.environ,
    'CARGO_TARGET_DIR': str(sdk / 'target'),
    'CARGO_NET_OFFLINE': 'true',
    'CARGO_INCREMENTAL': '0',
    'RUSTY_V8_ARCHIVE': str(archive),
    'RUSTY_V8_SRC_BINDING_PATH': str(bindings),
}
modes = ['preimage', 'recovery', 'rejection'] if args.mode == 'all' else [args.mode]
for mode in modes:
    apply = [sys.executable, str(here / 'apply_async_admission.py'), str(sdk)]
    if mode == 'preimage':
        apply.append('--reverse')
    else:
        apply.append('--record-provenance')
    subprocess.run(apply, check=True)
    command = ['cargo', '+' + args.toolchain, 'run', '--locked',
        '--manifest-path', str(here / 'async-admission-probe/Cargo.toml'),
        '--config', str(sdk / 'cargo-config.toml'), '--']
    if mode == 'preimage':
        command.append('--expect-missing')
    elif mode == 'rejection':
        command.append('--characterize-rejection')
    subprocess.run(command, env=env, check=True)
