"""Opt-in source contract for the existing 152-to-154/155 native SDK overlay.

These checks establish prepared source identity, not native ABI/link/runtime parity.
The C++ wrappers construct the selected engine's public handler configurations;
never substitute the default150 fork's Rust POD layout here.
"""
import hashlib
import json
from pathlib import Path
import tomllib

# Checksum-pinned v8-152.2.0.crate after the reviewed sdk.patch and
# code_generation.patch. Updating either patch requires a new source review.
EXPECTED_SOURCES = {
    'src/context.rs': 'e46a2d039a196295e2558fb540c8e4fe2c7f90c5abd4c15383ba96340c6696ec',
    'src/function.rs': '863829980224a6b86c1af164f561cc5707a03dcfab8d8d067b0ad5b19b730ff5',
    'src/isolate.rs': 'aee814bcf9c17fa5213f33ab4080625a4e03595c2f5ed07f64add7f76f922c83',
    'src/template.rs': '216c46482417f77b54c0dfaebedddf70c934cfcf3b3cda6b8abe7957a3035418',
    'src/binding.cc': 'e7df09e41ba08f64eaac66ef02b0e6b167018e001ab31c45247d26625575ee5a',
    'src/lib.rs': '28167e99e3df7d29eb8a8ce96c207d92e33da3f54f020b86ab2c3353f3cd221e',
}
EXPECTED_PROFILES = {154: 'b3d6849efaf11696198e15b32290e6d6b0d540c1',
                     155: 'd89479ac5446a374c55a35be19b5a23437558d99'}
REQUIRED = {
    'src/context.rs': ['pub fn detach_global(', 'pub global_template:', 'pub global_object:'],
    'src/isolate.rs': ['pub fn set_failed_access_check_callback('],
    'src/template.rs': ['pub fn set_access_check_callback_and_handler(',
                        'pub fn set_internal_field_count(', 'pub fn set_immutable_proto(',
                        'pub fn read_only_prototype(', 'pub fn instance_template<',
                        'pub fn set_class_name('],
    'src/function.rs': ['pub type AccessCheckCallback', 'pub enum AccessType', 'pub type FailedAccessCheckCallback'],
    'src/binding.cc': ['void v8__Context__DetachGlobal(',
                       'void v8__Isolate__SetFailedAccessCheckCallbackFunction(',
                       'void v8__ObjectTemplate__SetAccessCheckCallbackAndHandler('],
}
MARKER = ('\n/// Native WindowProxy primitives in this source-verified 154/155 SDK overlay.\n'
          '/// Native ABI, linking and runtime acceptance remain separate build gates.\n'
          'pub const FORK_WINDOW_PROXY: bool = true;\n')


def profile_identity(profile):
    if EXPECTED_PROFILES.get(profile.get('chromeMajor')) != profile.get('engineRevision') or profile.get('chromeMajor') not in EXPECTED_PROFILES:
        raise ValueError('WindowProxy compatibility requires a pinned 154/155 profile')
    return {name: profile[name] for name in ('chromeMajor', 'engineRevision')}


def unmarked_library(text):
    if 'FORK_WINDOW_PROXY' not in text:
        return text
    if text.endswith(MARKER) and text.count('FORK_WINDOW_PROXY') == 1:
        return text[:-len(MARKER)]
    raise ValueError('Unreviewed or duplicate SDK WindowProxy marker')


def source_record(sdk):
    sdk = Path(sdk)
    manifest = tomllib.loads((sdk / 'Cargo.toml').read_text())
    package = manifest['package']
    if (package['name'], package['version']) != ('v8', '152.2.0'):
        raise ValueError('WindowProxy compatibility requires the original v8 152.2.0 SDK')
    if manifest.get('lib', {}).get('path', 'src/lib.rs') != 'src/lib.rs':
        raise ValueError('WindowProxy compatibility requires the reviewed SDK library path')
    hashes = {}
    for name, expected in EXPECTED_SOURCES.items():
        text = (sdk / name).read_bytes().decode()
        for token in REQUIRED.get(name, []):
            if text.count(token) != 1:
                raise ValueError('Expected exactly one SDK WindowProxy API/type: ' + token)
        original = unmarked_library(text) if name == 'src/lib.rs' else text
        if hashlib.sha256(original.encode()).hexdigest() != expected:
            raise ValueError('Unreviewed SDK WindowProxy source: ' + name)
        hashes[name] = hashlib.sha256(text.encode()).hexdigest()
    return hashes


def enable(sdk, profile):
    identity = profile_identity(profile)
    source_record(sdk)
    library = Path(sdk) / 'src/lib.rs'
    text = library.read_bytes().decode()
    if not text.endswith(MARKER):
        library.write_bytes((text + MARKER).encode())
    return {'profile': identity, 'sources': source_record(sdk),
            'validation': 'source-only; native SDK ABI/link/runtime gates required'}


def verify(sdk, profile, record):
    identity = profile_identity(profile)
    if not record or record.get('profile') != identity:
        raise ValueError('WindowProxy compatibility provenance differs from the engine profile')
    if not (Path(sdk) / 'src/lib.rs').read_text().endswith(MARKER):
        raise ValueError('SDK WindowProxy capability marker is missing')
    if record.get('sources') != source_record(sdk):
        raise ValueError('WindowProxy compatibility source provenance differs')


def verify_window_proxy_alias(root):
    root = Path(root)
    provenance = root / 'provenance.json'
    if not provenance.is_file():
        raise ValueError('WindowProxy alias requires prepared capability provenance')
    record = json.loads(provenance.read_text())
    verify(root / 'v8-exact', record, record.get('windowProxyCompatibility'))
