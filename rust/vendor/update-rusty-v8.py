#!/usr/bin/env python3
"""Reproduce the reviewed Rusty V8 source patch without modifying Cargo's cache."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent
PINS = json.loads((ROOT / "RUSTY_V8_SOURCE_PINS.json").read_text())


def files(directory):
    return {
        path.relative_to(directory): path.read_bytes()
        for path in directory.rglob("*")
        if path.is_file()
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--full-source", action="store_true")
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--check", action="store_true")
    modes.add_argument("--write", action="store_true")
    args = parser.parse_args()
    archive = args.archive
    if archive is None:
        matches = list((Path.home() / ".cargo/registry/cache").glob("*/" + PINS["archive"]))
        if len(matches) != 1:
            parser.error("pass --archive with the published " + PINS["archive"])
        archive = matches[0]
    if hashlib.sha256(archive.read_bytes()).hexdigest() != PINS["archive_sha256"]:
        parser.error("published archive SHA-256 does not match")
    patch = ROOT / "rusty-v8-150.patch"
    if hashlib.sha256(patch.read_bytes()).hexdigest() != PINS["patch_sha256"]:
        parser.error("reviewed patch SHA-256 does not match")
    with tempfile.TemporaryDirectory(prefix="turbo-rusty-v8-") as temporary:
        source = Path(temporary)
        with tarfile.open(archive) as package:
            for entry in package:
                if not entry.isfile():
                    continue
                relative = Path(entry.name).relative_to("v8-" + PINS["version"])
                if ".." in relative.parts or relative.is_absolute():
                    parser.error("unsafe package entry")
                if not args.full_source and len(relative.parts) > 1:
                    if relative.parts[0] not in PINS["included_directories"]:
                        continue
                path = source / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(package.extractfile(entry).read())
                path.chmod(entry.mode & 0o777)
        isolate = source / "src/isolate.rs"
        if hashlib.sha256(isolate.read_bytes()).hexdigest() != PINS["original_isolate_sha256"]:
            parser.error("upstream isolate source SHA-256 does not match")
        function = source / "src/function.rs"
        if hashlib.sha256(function.read_bytes()).hexdigest() != PINS["original_function_sha256"]:
            parser.error("upstream callback source SHA-256 does not match")
        subprocess.run(
            ["patch", "--batch", "-p1", "-i", str(patch)],
            cwd=source,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        if hashlib.sha256(isolate.read_bytes()).hexdigest() != PINS["patched_isolate_sha256"]:
            parser.error("patched isolate source SHA-256 does not match")
        if hashlib.sha256(function.read_bytes()).hexdigest() != PINS["patched_function_sha256"]:
            parser.error("patched callback source SHA-256 does not match")
        if hashlib.sha256((source / "LICENSE").read_bytes()).hexdigest() != PINS["license_sha256"]:
            parser.error("retained license SHA-256 does not match")
        expected = files(source)
        destination = ROOT / "rusty_v8"
        if args.check:
            actual = files(destination)
            differences = sorted(
                str(path) for path in expected.keys() | actual.keys()
                if expected.get(path) != actual.get(path)
            )
            if differences:
                parser.error("vendor differs from reproduced patch: " + ", ".join(differences))
        else:
            for path in destination.iterdir() if destination.exists() else []:
                if path.is_dir():
                    shutil.rmtree(path)
                else:
                    path.unlink()
            destination.mkdir(exist_ok=True)
            for relative, data in expected.items():
                path = destination / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                path.chmod((source / relative).stat().st_mode & 0o777)
    print("Verified Rusty V8 " + PINS["version"] + " archive, license, weak teardown and interceptor mode patches")


if __name__ == "__main__":
    main()
