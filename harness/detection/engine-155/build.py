#!/usr/bin/env python3
"""Use the shared native SDK helper for the exact Chrome 155 engine profile."""
from pathlib import Path
import runpy
import sys
if '--profile' in sys.argv:
    raise SystemExit('This entry point requires Chrome 155; use engine-154/build.py to choose another profile.')
sys.argv[1:1] = ['--profile', '155']
shared = Path(__file__).resolve().parents[1] / 'engine-154'
sys.path.insert(0, str(shared))
runpy.run_path(str(shared / 'build.py'), run_name='__main__')
