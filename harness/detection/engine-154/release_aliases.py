"""Prepare metadata-only SDK aliases without changing the verified SDK sources.

Surf's production graph names the Deno family `turbo-surf-deno-*` 0.x.1 and the
native binding `turbo-dom-v8` 150.4. The overlay keeps the verified SDK sources
(patched deno_core 0.412.0, deno_v8 0.4.0, rusty_v8 152.2.0 built against the
exact native engine) and only writes alias manifests that give those sources the
registry identities Surf requires, plus a `[patch.crates-io]` table passed with
`--config` (which takes precedence over any manifest/PR patch for those names).
"""

import hashlib
import json
from pathlib import Path
import tomllib


# SDK directory: (verified SDK package, verified SDK version, alias package, alias version)
ALIASES = {'deno_core': ('deno_core', '0.412.0', 'turbo-surf-deno-core', '0.412.1'),
           'deno_v8': ('deno_v8', '0.4.0', 'turbo-surf-deno-v8', '0.4.1'),
           'v8-exact': ('v8', '152.2.0', 'turbo-dom-v8', '150.4.0')}
# Exact internal edges of the 0.x.1 family the aliases must satisfy.
CORE_EDGES = {'v8': ('turbo-surf-deno-v8', '0.4.1'), 'serde_v8': ('turbo-surf-serde-v8', '0.321.1')}
NATIVE_PACKAGES = {'v8', 'turbo-dom-v8'}
# The SDK's native engine lacks turbo-dom-v8's ShouldThrowOnError accessor fix.
# Surf crates declare this forwarding feature; the binding then reads the frame.
FRAME_READ_FEATURE = 'native-sdk-frame-read'


def inline(value):
    if isinstance(value, dict):
        return '{' + ', '.join(json.dumps(k) + ' = ' + inline(v) for k, v in value.items()) + '}'
    if isinstance(value, list):
        return '[' + ', '.join(inline(v) for v in value) + ']'
    if isinstance(value, (str, int, float, bool)):
        return json.dumps(value)
    raise ValueError('Unsupported SDK manifest value')


def manifest(value):
    result = '\n'.join(json.dumps(k) + ' = ' + inline(v) for k, v in value.items()) + '\n'
    assert tomllib.loads(result) == value
    return result


def write_owned(path, content):
    if path.is_symlink() or (path.exists() and path.read_text() != content):
        raise ValueError('Refusing to replace SDK alias manifest content')
    path.write_text(content)


def without_second_backend(parsed, directory):
    """Mirror the 0.x.1 family: no quickjs/v8x option, which no longer resolves."""
    features = parsed.get('features', {})
    if directory == 'deno_core':
        if features.pop('quickjs', ['v8/quickjs', 'serde_v8/quickjs']) != ['v8/quickjs', 'serde_v8/quickjs']:
            raise ValueError('Unexpected SDK backend features: deno_core')
    elif directory == 'deno_v8':
        if features.pop('quickjs', ['dep:v8x_backend', 'v8x_backend/quickjs']) != ['dep:v8x_backend', 'v8x_backend/quickjs']:
            raise ValueError('Unexpected SDK backend features: deno_v8')
        for name, values in features.items():
            features[name] = [v for v in values if not v.startswith(('v8x_backend', 'dep:v8x_backend'))]
        parsed['dependencies'].pop('v8x_backend', None)


def rewrite_edges(parsed, directory, root):
    dependencies = parsed.get('dependencies', {})
    if directory == 'deno_core':
        for key, (renamed, expected) in CORE_EDGES.items():
            value = dependencies[key]
            if isinstance(value, str):
                value = {'version': value}
            value.update(package=renamed, version='=' + expected)
            value.pop('path', None)
            dependencies[key] = value
    elif directory == 'deno_v8':
        # The facade must still use the prepared native SDK, never the stock150
        # registry turbo-dom-v8; it points at that SDK's alias directly.
        native = dependencies['rusty_v8']
        if native.get('package') not in NATIVE_PACKAGES:
            raise ValueError('Unexpected native SDK backend alias')
        native.pop('version', None)
        native.update(package='turbo-dom-v8', path=str(root / 'v8-exact'))
    else:
        # Metadata-only rename: the build script uses CARGO_PKG_VERSION only for
        # download URLs, which RUSTY_V8_ARCHIVE / RUSTY_V8_SRC_BINDING_PATH bypass.
        library = parsed.setdefault('lib', {})
        if library.setdefault('name', 'v8') != 'v8':
            raise ValueError('SDK library alias differs: v8-exact')
        if parsed['package'].setdefault('links', 'rusty_v8') != 'rusty_v8':
            raise ValueError('Unexpected native SDK links identity')


def frame_read_arguments(arguments, source):
    """`--features native-sdk-frame-read` when a selected Surf member forwards it."""
    rust = Path(source).resolve() / 'rust'
    members = {}
    for member in tomllib.loads((rust / 'Cargo.toml').read_text())['workspace']['members']:
        parsed = tomllib.loads((rust / member / 'Cargo.toml').read_text())
        members[parsed['package']['name']] = FRAME_READ_FEATURE in parsed.get('features', {})
    if not any(members.values()):
        raise ValueError('Surf crates must declare a forwarding native-sdk-frame-read feature for the native SDK overlay')
    selected, everything = [], False
    for index, argument in enumerate(arguments):
        if argument in {'--workspace', '--all'}:
            everything = True
        elif argument in {'-p', '--package'} and index + 1 < len(arguments):
            selected.append(arguments[index + 1])
        elif argument.startswith('--package='):
            selected.append(argument.split('=', 1)[1])
        elif argument.startswith('-p') and len(argument) > 2:
            selected.append(argument[2:])
    names = members if everything or not selected else selected
    return ['--features', FRAME_READ_FEATURE] if any(members.get(name) for name in names) else []


def sdk_config(source, sdk, workspace):
    """Use renamed registry identities only when the current Surf manifest does."""
    source, sdk, workspace = (Path(p).resolve() for p in [source, sdk, workspace])
    renderer = tomllib.loads((source / 'rust/crates/turbo-surf-render/Cargo.toml').read_text())
    core = renderer['dependencies']['deno_core']
    package = core.get('package', 'deno_core') if isinstance(core, dict) else 'deno_core'
    if package == 'deno_core':
        return sdk / 'cargo-config.toml'
    if package != 'turbo-surf-deno-core':
        raise ValueError('Unsupported render runtime identity')
    if core.get('version') != '=' + ALIASES['deno_core'][3]:
        # A different requirement would leave the alias unused and silently
        # resolve the stock registry engine instead of the native SDK.
        raise ValueError('SDK aliases target turbo-surf-deno-core =' + ALIASES['deno_core'][3])
    if workspace == sdk or not workspace.is_relative_to(sdk):
        raise ValueError('SDK aliases must stay inside the private Surf workspace')
    if any(workspace.is_relative_to(sdk / name) for name in ALIASES):
        raise ValueError('SDK aliases must not overlap original SDK packages')
    root = workspace / 'sdk-release-aliases'
    if root.is_symlink():
        raise ValueError('Refusing a linked SDK alias directory')
    root.mkdir(parents=True, exist_ok=True)
    records = {}
    for directory, (old, version, name, alias_version) in ALIASES.items():
        original = sdk / directory
        raw = (original / 'Cargo.toml').read_bytes()
        parsed = tomllib.loads(raw.decode())
        if parsed['package']['name'] != old or parsed['package']['version'] != version:
            raise ValueError('Unexpected verified SDK dependency identity: ' + directory)
        if directory != 'v8-exact' and parsed['lib']['name'] != old:
            raise ValueError('SDK library alias differs: ' + old)
        target = root / directory
        if target.is_symlink():
            raise ValueError('Refusing a linked SDK alias directory')
        target.mkdir(exist_ok=True)
        for child in original.iterdir():
            if child.name in {'Cargo.toml', 'Cargo.lock', 'target', '.git'}:
                continue
            link = target / child.name
            if link.is_symlink():
                if link.readlink() != child:
                    raise ValueError('Unexpected SDK source alias link')
            elif link.exists():
                raise ValueError('Refusing to replace SDK source alias content')
            else:
                link.symlink_to(child, target_is_directory=child.is_dir())
        parsed['package']['name'] = name
        parsed['package']['version'] = alias_version
        # These aliases sit below the SDK experiment, which is itself a Cargo
        # package. Give each alias an explicit workspace boundary so Cargo never
        # treats it as an undeclared member of that experiment.
        parsed['workspace'] = {}
        without_second_backend(parsed, directory)
        rewrite_edges(parsed, directory, root)
        output = manifest(parsed)
        write_owned(target / 'Cargo.toml', output)
        records[directory] = {'sdkPackage': old + ' ' + version, 'aliasPackage': name + ' ' + alias_version,
                              'sdkManifestSha256': hashlib.sha256(raw).hexdigest(),
                              'aliasManifestSha256': hashlib.sha256(output.encode()).hexdigest()}
    config = root / 'cargo-config.toml'
    write_owned(config, '[patch.crates-io]\n' + ''.join(
        json.dumps(name) + ' = { path = ' + json.dumps(str(root / directory)) + ' }\n'
        for directory, (_, _, name, _) in ALIASES.items()))
    write_owned(root / 'metadata-bridge.json', json.dumps({
        'scope': 'Metadata aliases only; original SDK source and manifests are unchanged.',
        'manifests': records,
    }, indent=2) + '\n')
    return config
