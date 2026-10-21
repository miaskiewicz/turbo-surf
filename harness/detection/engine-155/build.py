#!/usr/bin/env python3
"""Use the shared native SDK helper for the exact Chrome 155 engine profile."""
import argparse
from pathlib import Path
import subprocess
import runpy
import sys
if '--profile' in sys.argv:
    raise SystemExit('This entry point requires Chrome 155; use engine-154/build.py to choose another profile.')
# Verify the experimental JS transaction before any native SDK or Surf compilation.
wrapper_args = sys.argv[1:]
if '--surf' in wrapper_args:
    wrapper_args = wrapper_args[:wrapper_args.index('--surf')]
if '--help' not in wrapper_args and '-h' not in wrapper_args:
    admission_args = argparse.ArgumentParser(add_help=False)
    admission_args.add_argument('destination', type=Path)
    admission_args.add_argument('--jobs')
    admission_args.add_argument('--archive')
    admission_args.add_argument('--bindings')
    admission_args.add_argument('--archive-sha256')
    admission_args.add_argument('--surf', nargs=argparse.REMAINDER)
    selected, _ = admission_args.parse_known_args(sys.argv[1:])
    subprocess.run([
        sys.executable, str(Path(__file__).with_name('apply_async_admission.py')),
        str(selected.destination), '--record-provenance',
    ], check=True)
sys.argv[1:1] = ['--profile', '155']
shared = Path(__file__).resolve().parents[1] / 'engine-154'
sys.path.insert(0, str(shared))
runpy.run_path(str(shared / 'build.py'), run_name='__main__')
