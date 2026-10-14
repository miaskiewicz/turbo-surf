#!/usr/bin/env python3
"""Build/check a prepared engine and optionally run a surf Cargo command with it."""
import argparse
import hashlib
import json
import os
import platform
import shutil
from pathlib import Path
import subprocess
import sys
from prepare import verify_engine, verify_engine_patch
from surf_workspace import prepare_workspace, cargo_arguments

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('destination',type=Path)
parser.add_argument('--profile',choices=['154','155'],help='Require this prepared engine profile')
parser.add_argument('--jobs',type=int,default=4)
parser.add_argument('--archive',type=Path,help='Reuse an already source-built native archive; requires --bindings and --archive-sha256')
parser.add_argument('--bindings',type=Path)
parser.add_argument('--archive-sha256')
parser.add_argument('--surf',nargs=argparse.REMAINDER,help='Cargo arguments, e.g. test --manifest-path rust/Cargo.toml -p turbo-surf-render')
argv=sys.argv[1:]
surf=None
if '--surf' in argv:
    index=argv.index('--surf')
    surf=argv[index+1:]
    argv=argv[:index]
    if not surf or surf[0].startswith('-'):
        parser.error('--surf requires a Cargo subcommand')
args=parser.parse_args(argv)
args.surf=surf
root=args.destination.resolve()
if not (root/'provenance.json').is_file():
    sys.exit('Run prepare.py first.')
record=json.loads((root/'provenance.json').read_text())
verify_engine(root/'v8-exact/v8',record)
verify_engine_patch(root/'v8-exact/v8')
engine_version=record['engineVersion']
engine_major=str(record.get('chromeMajor', {'15.4':154, '15.5':155}.get('.'.join(engine_version.split('.')[:2]), 0)))
if engine_major not in ['154', '155']:
    sys.exit('Unsupported native engine profile.')
if args.profile and args.profile != engine_major:
    sys.exit('The prepared engine differs from the requested profile.')
if not 1 <= args.jobs <= 32:
    sys.exit('Jobs must be between 1 and 32.')
environment={**os.environ,'V8_FROM_SOURCE':'1','CARGO_BUILD_JOBS':str(args.jobs),'GN_ARGS':'is_debug=false symbol_level=0'}
environment['CARGO_TARGET_DIR']=str(root/'target')
tools=root/'target/debug/ninja_gn_binaries'
local_bin=root/'tool-bin'
local_bin.mkdir(exist_ok=True)
if sys.platform != 'win32' and not (local_bin/'python').exists():
    (local_bin/'python').symlink_to(sys.executable)
environment['PATH']=os.pathsep.join([str(local_bin),str(tools/'gn'),str(tools/'ninja'),environment.get('PATH','')])
environment.setdefault('CARGO_INCREMENTAL','0')
# The pinned Chromium Rust toolchain includes the matching modern libclang.
# Prefer it over an older system Xcode library when it is already downloaded.
bundled_clang=root/'v8-exact/third_party/rust-toolchain/lib'
if sys.platform == 'darwin':
    environment.setdefault('LIBCLANG_PATH',str(bundled_clang))
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

native_sources={name:sha(root/'v8-exact'/name) for name in ['src/binding.cc','src/binding.hpp']}
cache_dir=root/'native-cache'
cache_manifest=cache_dir/'artifact.json'
artifact=None
if args.archive or args.bindings or args.archive_sha256:
    if not all([args.archive,args.bindings,args.archive_sha256]):
        sys.exit('Provide --archive, --bindings and --archive-sha256 together.')
    if sha(args.archive)!=args.archive_sha256:
        sys.exit('Native archive integrity mismatch.')
    cache_dir.mkdir(exist_ok=True)
    shutil.copyfile(args.archive,cache_dir/'librusty_v8.a')
    shutil.copyfile(args.bindings,cache_dir/'src_binding.rs')
    artifact={'archiveSha256':args.archive_sha256,'bindingsSha256':sha(args.bindings),'nativeSources':native_sources,'platform':sys.platform,'machine':platform.machine()}
    # Keep imported cache integrity after a Rust SDK failure; every reuse still
    # runs the native version smoke before any surf command is permitted.
    imported_manifest=cache_dir/'artifact.json.partial'
    imported_manifest.write_text(json.dumps(artifact,indent=2)+'\n')
    imported_manifest.replace(cache_manifest)
elif cache_manifest.exists():
    candidate=json.loads(cache_manifest.read_text())
    if candidate['nativeSources']==native_sources and candidate['platform']==sys.platform and candidate['machine']==platform.machine() and candidate.get('engineRevision',record['engineRevision'])==record['engineRevision']:
        if sha(cache_dir/'librusty_v8.a')!=candidate['archiveSha256'] or sha(cache_dir/'src_binding.rs')!=candidate['bindingsSha256']:
            sys.exit('Cached native artifact integrity mismatch.')
        artifact=candidate
if artifact:
    environment.pop('V8_FROM_SOURCE',None)
    environment.update({'RUSTY_V8_ARCHIVE':str(cache_dir/'librusty_v8.a'),'RUSTY_V8_ARCHIVE_SHA256':artifact['archiveSha256'],'RUSTY_V8_SRC_BINDING_PATH':str(cache_dir/'src_binding.rs')})

command=['cargo','run','--manifest-path',str(root/'Cargo.toml')]
with (root/'build.log').open('w') as log:
    process=subprocess.run(command,env=environment,stdout=log,stderr=subprocess.STDOUT)
if process.returncode:
    sys.exit(f'Build failed; inspect {root / "build.log"}')
log=(root/'build.log').read_text()
native_version_line=next((line for line in log.splitlines() if line.startswith('V8 ')), '')
if native_version_line.removeprefix('V8 ').split('-')[0] != engine_version:
    sys.exit('Native engine did not report the expected version; do not use this build.')
binary=root/'target/debug/turbo-surf-v8-sdk-experiment'
record.update({'nativeVersionLine':native_version_line,'smokeBinarySha256':hashlib.sha256(binary.read_bytes()).hexdigest()})
if artifact is None:
    cache_dir.mkdir(exist_ok=True)
    native=root/'target/debug/gn_out'
    shutil.copyfile(native/'obj/librusty_v8.a',cache_dir/'librusty_v8.a')
    shutil.copyfile(native/'src_binding.rs',cache_dir/'src_binding.rs')
    artifact={'archiveSha256':sha(cache_dir/'librusty_v8.a'),'bindingsSha256':sha(cache_dir/'src_binding.rs'),'nativeSources':native_sources,'platform':sys.platform,'machine':platform.machine()}
artifact['nativeVersionLine']=record['nativeVersionLine']
artifact['engineRevision']=record['engineRevision']
cache_manifest.write_text(json.dumps(artifact,indent=2)+'\n')
environment.pop('V8_FROM_SOURCE',None)
environment.update({'RUSTY_V8_ARCHIVE':str(cache_dir/'librusty_v8.a'),'RUSTY_V8_ARCHIVE_SHA256':artifact['archiveSha256'],'RUSTY_V8_SRC_BINDING_PATH':str(cache_dir/'src_binding.rs')})
record['nativeArtifact']=artifact

with (root/'native-window-proxy.log').open('w') as log:
    process=subprocess.run(['cargo','test','--manifest-path',str(root/'Cargo.toml'),'--test','native_window_proxy','--','--nocapture'],env=environment,stdout=log,stderr=subprocess.STDOUT)
if process.returncode:
    sys.exit(f'Native WindowProxy regressions failed; inspect {root / "native-window-proxy.log"}')
record['nativeWindowProxyTests']='passed'
with (root/'native-module-units.log').open('w') as log:
    process=subprocess.run(['cargo','test','--manifest-path',str(root/'Cargo.toml'),'--test','native_module_units','--','--nocapture'],env=environment,stdout=log,stderr=subprocess.STDOUT)
if process.returncode:
    sys.exit(f'Native module unit regressions failed; inspect {root / "native-module-units.log"}')
record['nativeModuleUnitsTests']='passed'

(root/'validated.json').write_text(json.dumps(record,indent=2)+'\n')
print(record['nativeVersionLine'])
if args.surf:
    # Reuse this engine's object cache; keep production Cargo manifests intact.
    environment['TURBO_SURF_ENGINE']=engine_major
    environment['TURBO_SURF_ENGINE_VERSION']=record['nativeVersionLine'].removeprefix('V8 ')
    # External Cargo plugins (notably clippy) only forward arguments after the
    # subcommand to their inner Cargo invocation. Keep the profile there.
    source=Path(__file__).resolve().parents[3]
    workspace=prepare_workspace(source,root/'surf-workspace')
    arguments=cargo_arguments(args.surf,source,workspace)
    sys.exit(subprocess.call(['cargo',arguments[0],'--config',str(root/'cargo-config.toml'),*arguments[1:]],env=environment))
