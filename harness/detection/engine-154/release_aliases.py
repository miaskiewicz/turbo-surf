"""Prepare metadata-only SDK aliases without changing the verified SDK sources."""

import hashlib
import json
from pathlib import Path
import tomllib


ALIASES = {'deno_core': ('turbo-surf-deno-core', '0.412.0'),
           'deno_v8': ('turbo-surf-deno-v8', '0.4.0')}


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
    if workspace == sdk or not workspace.is_relative_to(sdk):
        raise ValueError('SDK aliases must stay inside the private Surf workspace')
    if any(workspace.is_relative_to(sdk / name) for name in [*ALIASES, 'v8-exact']):
        raise ValueError('SDK aliases must not overlap original SDK packages')
    root = workspace / 'sdk-release-aliases'
    if root.is_symlink():
        raise ValueError('Refusing a linked SDK alias directory')
    root.mkdir(parents=True, exist_ok=True)
    records = {}
    for old, (name, version) in ALIASES.items():
        original = sdk / old
        raw = (original / 'Cargo.toml').read_bytes()
        parsed = tomllib.loads(raw.decode())
        if parsed['package']['name'] != old or parsed['package']['version'] != version:
            raise ValueError('Unexpected verified SDK dependency identity: ' + old)
        if parsed['lib']['name'] != old:
            raise ValueError('SDK library alias differs: ' + old)
        target = root / old
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
        # These aliases sit below the SDK experiment, which is itself a Cargo
        # package. Give each alias an explicit workspace boundary so Cargo never
        # treats it as an undeclared member of that experiment.
        parsed['workspace'] = {}
        dependencies = parsed['dependencies']
        if old == 'deno_core':
            for key, renamed, expected in [('v8', 'turbo-surf-deno-v8', '0.4.0'),
                                            ('serde_v8', 'turbo-surf-serde-v8', '0.321.0')]:
                value = dependencies[key]
                if isinstance(value, str):
                    value = {'version': value}
                value.update(package=renamed, version='=' + expected)
                value.pop('path', None)
                dependencies[key] = value
        else:
            # The facade must still use the prepared native155 SDK, not stock150.
            native = dependencies['rusty_v8']
            if native.get('package') != 'v8':
                raise ValueError('Unexpected native SDK backend alias')
            native['path'] = str(sdk / 'v8-exact')
        output = manifest(parsed)
        write_owned(target / 'Cargo.toml', output)
        records[old] = {'sdkManifestSha256': hashlib.sha256(raw).hexdigest(),
                        'aliasManifestSha256': hashlib.sha256(output.encode()).hexdigest()}
    config = root / 'cargo-config.toml'
    write_owned(config, '[patch.crates-io]\n' + ''.join(
        json.dumps(name) + ' = { path = ' + json.dumps(str(root / old)) + ' }\n'
        for old, (name, _) in ALIASES.items()))
    write_owned(root / 'metadata-bridge.json', json.dumps({
        'scope': 'Metadata aliases only; original SDK source and manifests are unchanged.',
        'manifests': records,
    }, indent=2) + '\n')
    return config
