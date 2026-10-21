"""Isolate experimental Cargo manifests/locks while using the current Surf sources."""
from pathlib import Path
import shutil


def link(source, target):
    if target.is_symlink():
        if target.readlink() != source:
            raise ValueError('Unexpected workspace link: ' + str(target))
        return
    if target.exists():
        raise ValueError('Refusing to replace workspace content: ' + str(target))
    target.symlink_to(source, target_is_directory=source.is_dir())


def prepare_workspace(source, destination):
    source = source.resolve()
    destination = destination.resolve()
    if destination == source or not destination.is_relative_to(source / '.native-builds'):
        raise ValueError('Experimental workspace must be inside the ignored native builds directory')
    destination.mkdir(parents=True, exist_ok=True)
    for child in source.iterdir():
        if child.name not in {'.git', '.native-builds', 'node_modules', 'rust'}:
            link(child, destination / child.name)
    native = destination / 'rust'
    native.mkdir(exist_ok=True)
    for child in (source / 'rust').iterdir():
        if child.name not in {'crates', 'target', 'Cargo.toml', 'Cargo.lock'}:
            link(child, native / child.name)
    shutil.copyfile(source / 'rust/Cargo.toml', native / 'Cargo.toml')
    if not (native / 'Cargo.lock').exists():
        shutil.copyfile(source / 'rust/Cargo.lock', native / 'Cargo.lock')
    crates = native / 'crates'
    crates.mkdir(exist_ok=True)
    for child in (source / 'rust/crates').iterdir():
        if not child.is_dir():
            link(child, crates / child.name)
            continue
        crate = crates / child.name
        crate.mkdir(exist_ok=True)
        for entry in child.iterdir():
            if entry.name == 'Cargo.toml':
                shutil.copyfile(entry, crate / entry.name)
            else:
                link(entry, crate / entry.name)
    return destination


def cargo_arguments(arguments, source, destination):
    arguments = list(arguments)
    source = source.resolve()
    found = False
    for index, argument in enumerate(arguments):
        if argument == '--manifest-path':
            if index + 1 == len(arguments):
                raise ValueError('--manifest-path requires a path')
            path = Path(arguments[index + 1]).resolve().relative_to(source / 'rust')
            arguments[index + 1] = str(destination / 'rust' / path)
            found = True
        elif argument.startswith('--manifest-path='):
            path = Path(argument.split('=', 1)[1]).resolve().relative_to(source / 'rust')
            arguments[index] = '--manifest-path=' + str(destination / 'rust' / path)
            found = True
    if not found:
        # Cargo plugins forward manifest arguments only after the subcommand.
        arguments[1:1] = ['--manifest-path', str(destination / 'rust/Cargo.toml')]
    return arguments
