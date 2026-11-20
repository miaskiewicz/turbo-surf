#!/usr/bin/env python3
"""Install verified local release archives normally, then run the installed launcher."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile



def install(thin, platform, output):
    output.mkdir(parents=True)
    project = output / 'project'
    project.mkdir()
    (project / 'package.json').write_text(json.dumps({'name': 'release-install-proof', 'private': True}))
    for name in ('user.npmrc', 'global.npmrc'):
        (output / name).write_text('')
    env = {key: value for key, value in os.environ.items() if not key.startswith(('NODE_', 'NPM_', 'TURBO_SURF_', 'DYLD_', 'LD_')) and not key.endswith('_TOKEN')}
    env.update(NPM_CONFIG_USERCONFIG=str(output / 'user.npmrc'), NPM_CONFIG_GLOBALCONFIG=str(output / 'global.npmrc'), NPM_CONFIG_CACHE=str(output / 'cache'))
    command = ['npm', 'install', '--offline', '--no-audit', '--no-fund', '--save-exact', str(Path(thin).resolve()), str(Path(platform).resolve())]
    result = subprocess.run(command, cwd=project, env=env, capture_output=True, text=True, timeout=180)
    (output / 'npm-install.stdout.log').write_text(result.stdout)
    (output / 'npm-install.stderr.log').write_text(result.stderr)
    result.check_returncode()
    proof = {'command': command, 'exitCode': result.returncode, 'stdoutSha256': hashlib.sha256(result.stdout.encode()).hexdigest(), 'stderrSha256': hashlib.sha256(result.stderr.encode()).hexdigest()}
    return project, env, proof


def verify_installed(archive, destination):
    with tarfile.open(archive, 'r:gz') as stream:
        for member in stream.getmembers():
            if not member.isfile():
                continue
            relative = Path(member.name).relative_to('package')
            if relative.is_absolute() or '..' in relative.parts:
                raise ValueError('Escaping installed archive member')
            if (destination / relative).read_bytes() != stream.extractfile(member).read():
                raise ValueError('Installed npm payload differs: ' + str(relative))


def smoke(thin, platform, output, version, commit, runner):
    archives = [Path(thin), Path(platform)]
    hashes = [hashlib.sha256(path.read_bytes()).hexdigest() for path in archives]
    project, env, install_proof = install(thin, platform, output)
    identity = subprocess.run(['node', '-e', "const p=require('turbo-surf').binaryPath();if(!p)throw Error('No installed binary');console.log(p)"], cwd=project, env=env, capture_output=True, text=True, check=True, timeout=30)
    binary = Path(identity.stdout.strip()).resolve()
    if not binary.is_relative_to(project / 'node_modules/@miaskiewicz'):
        raise ValueError('Launcher resolved outside installed platform dependency')
    if os.name != 'nt' and binary.stat().st_mode & 0o111 != 0o111:
        raise ValueError('Normally installed native binary is not executable')
    cli = project / 'node_modules/turbo-surf/cli.js'
    verify_installed(thin, cli.parent)
    verify_installed(platform, binary.parent)
    receipt = runner([shutil.which('node'), str(cli)], version, commit, output / 'private-cwd', output / 'mcp-smoke')
    if not receipt.get('initializeVerified') or not receipt.get('javascriptVerified'):
        raise ValueError('Normally installed launcher failed its runtime smoke')
    verify_installed(thin, cli.parent)
    verify_installed(platform, binary.parent)
    if hashes != [hashlib.sha256(path.read_bytes()).hexdigest() for path in archives]:
        raise ValueError('Verified archive changed during normal install')
    result = {'install': install_proof, 'archiveSha256': hashes, 'installedPayloadUnchanged': True, 'normalLocalArchiveInstall': True, 'permissionRepair': False, 'project': str(project), 'binary': str(binary), 'binaryMode': oct(binary.stat().st_mode & 0o777), 'receipt': receipt}
    (output / 'FINAL.json').write_text(json.dumps(result, indent=2) + '\n')
    return result
