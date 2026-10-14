#!/usr/bin/env python3
"""Reproduce the licensed provider patch from the exact published crate archive."""
import argparse
import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile

ARCHIVE = "deno_crypto-0.273.0.crate"
SHA256 = "d9931d22328961b9a9c8d3b666a27312c1f3ddcc7ea78d8610ddfbc934ac61a0"
ROOT = Path(__file__).resolve().parent


def files(directory):
    return {
        path.relative_to(directory): path.read_bytes()
        for path in directory.rglob("*")
        if path.is_file() and "target" not in path.relative_to(directory).parts
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--check", action="store_true")
    modes.add_argument("--write", action="store_true")
    args = parser.parse_args()
    archive = args.archive
    if archive is None:
        matches = list((Path.home() / ".cargo/registry/cache").glob("*/" + ARCHIVE))
        if len(matches) != 1:
            parser.error("pass --archive with the downloaded published " + ARCHIVE)
        archive = matches[0]
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        parser.error("published crate archive SHA-256 does not match")
    with tempfile.TemporaryDirectory(prefix="turbo-deno-crypto-") as temporary:
        source = Path(temporary)
        with tarfile.open(archive) as package:
            for entry in package:
                if not entry.isfile():
                    continue
                relative = Path(entry.name).relative_to("deno_crypto-0.273.0")
                if ".." in relative.parts or relative.is_absolute():
                    parser.error("unsafe package entry")
                path = source / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(package.extractfile(entry).read())
        subprocess.run(
            ["patch", "--batch", "-p1", "-i", str(ROOT / "deno_crypto.patch")],
            cwd=source,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        expected = files(source)
        destination = ROOT / "deno_crypto"
        if args.check:
            actual = files(destination)
            differences = sorted(
                str(path)
                for path in expected.keys() | actual.keys()
                if expected.get(path) != actual.get(path)
            )
            if differences:
                parser.error("provider differs from reproduced patch: " + ", ".join(differences))
        else:
            actual = files(destination) if destination.exists() else {}
            for relative in actual.keys() - expected.keys():
                (destination / relative).unlink()
            for relative, data in expected.items():
                path = destination / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
    print("Verified deno_crypto 0.273.0 archive and reproducible native hardening patch")


if __name__ == "__main__":
    main()
