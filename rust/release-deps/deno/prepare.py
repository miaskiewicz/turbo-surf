#!/usr/bin/env python3
"""Prepare the pinned, renamed Deno dependency closure without Cargo patches."""

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parent
SURF = ROOT.parents[2]
PACKAGES = [
    ("v8", "150.4.0", "turbo-surf-v8", "rusty_v8"),
    ("deno_v8", "0.4.0", "turbo-surf-deno-v8", None),
    ("serde_v8", "0.321.0", "turbo-surf-serde-v8", None),
    ("deno_core", "0.412.0", "turbo-surf-deno-core", None),
    ("deno_bench_util", "0.253.0", "turbo-surf-deno-bench-util", None),
    ("deno_crypto", "0.273.0", "turbo-surf-deno-crypto", "deno_crypto"),
    ("deno_web", "0.290.0", "turbo-surf-deno-web", None),
    ("deno_webidl", "0.259.0", "turbo-surf-deno-webidl", None),
]
NAMES = {old: (version, new) for old, version, new, _ in PACKAGES}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def tree(path):
    result = {}
    for item in sorted(path.rglob("*")):
        if item.is_symlink():
            raise ValueError(f"Unexpected source symlink: {item}")
        if item.is_file():
            result[item.relative_to(path).as_posix()] = digest(item)
    return result


def checked_archive(path, expected):
    if path.is_symlink() or not path.is_file() or digest(path) != expected:
        raise ValueError(f"Missing or modified exact source archive: {path.name}")
    return path


def source_archive(old, version, pin, cargo_home, archive_dir=None, download=False):
    name = f"{old}-{version}.crate"
    expected_url = f"https://static.crates.io/crates/{old}/{name}"
    if pin["sourceUrl"] != expected_url:
        raise ValueError("Unexpected upstream archive URL")
    if archive_dir is not None:
        archive_dir = archive_dir.resolve()
        if archive_dir.is_relative_to(cargo_home.resolve()):
            raise ValueError("Private archives must be outside Cargo's cache")
        private = archive_dir / name
        if private.exists() or private.is_symlink():
            return checked_archive(private, pin["archiveSha256"])
    else:
        private = None
    cached = next(cargo_home.glob(f"registry/cache/*/{name}"), None)
    if cached is not None:
        checked_archive(cached, pin["archiveSha256"])
        if private is None or not download:
            return cached
        payload = cached.read_bytes()
    elif download and private is not None:
        request = urllib.request.Request(expected_url, headers={"User-Agent": "turbo-surf-release-preparation"})
        with urllib.request.urlopen(request, timeout=60) as response:
            payload = response.read(64 * 1024 * 1024 + 1)
        if len(payload) > 64 * 1024 * 1024:
            raise ValueError("Upstream archive exceeds preparation limit")
    else:
        raise ValueError(f"Missing exact source archive for {old}; supply --archive-dir --download")
    if hashlib.sha256(payload).hexdigest() != pin["archiveSha256"]:
        raise ValueError("Downloaded archive checksum mismatch")
    archive_dir.mkdir(parents=True, exist_ok=True)
    with private.open("xb") as stream:
        stream.write(payload)
    return checked_archive(private, pin["archiveSha256"])


def replace_field(body, key, value):
    replacement = f"{key} = {json.dumps(value)}"
    pattern = rf"(?m)^{re.escape(key)} = .*?$"
    return re.sub(pattern, lambda _: replacement, body) if re.search(pattern, body) else body + replacement + "\n"


def manifest(source, old, version, new):
    parsed = tomllib.loads(source)
    if (parsed["package"]["name"], parsed["package"]["version"]) != (old, version):
        raise ValueError("Unexpected upstream manifest identity")
    sections = re.split(r"(?m)^(\[[^\n]+\]\n)", source)
    for index in range(1, len(sections), 2):
        header, body = sections[index:index + 2]
        if header == "[package]\n":
            # The fork must retain its license in the actual archive; upstream
            # V8's broad exclusion otherwise filters this source file out.
            body = body.replace('    "LICENSE*",\n', '')
            body = replace_field(body, "name", new)
            body = replace_field(body, "repository", "https://github.com/miaskiewicz/turbo-surf")
            description = parsed["package"].get("description", old)
            body = replace_field(body, "description", f"turbo-surf reviewed distribution of {old}: {description}")
        match = re.search(r"(?:^|\.)(?:dev-|build-)?dependencies\.([^\]]+)\]", header[1:])
        if match:
            alias = match[1].strip('"')
            fields = tomllib.loads(body)
            upstream = fields.get("package", alias)
            if upstream in NAMES:
                required, package = NAMES[upstream]
                body = replace_field(body, "package", package)
                body = replace_field(body, "version", "=" + required)
                body = replace_field(body, "path", "../" + package)
        sections[index + 1] = body
    result = "".join(sections)
    return result + "\n[package.metadata.turbo-surf-upstream]\n" + f"name = {json.dumps(old)}\nversion = {json.dumps(version)}\nrepository = {json.dumps(parsed['package']['repository'])}\n"


def unpack(archive, destination, prefix):
    with tarfile.open(archive) as source:
        for member in source.getmembers():
            relative = Path(member.name).relative_to(prefix)
            if member.issym() or member.islnk() or ".." in relative.parts:
                raise ValueError("Unexpected archive link or escaping member")
            if not member.isdir() and not member.isfile():
                raise ValueError("Unexpected archive entry type")
        source.extractall(destination, filter="data")


def prepare(destination, cargo_home, archive_dir=None, download=False):
    pins = json.loads((ROOT / "source-pins.json").read_text())
    receipt = {"packages": [], "scope": "Prepared sources only; not packaged, compiled, published or clean-registry-consumer verified."}
    for old, version, new, vendor in PACKAGES:
        pin = pins["packages"][old]
        if pin["version"] != version or pin["candidateName"] != new:
            raise ValueError("Source pin package identity changed")
        archive = source_archive(old, version, pin, cargo_home, archive_dir, download)
        target = destination / "packages" / new
        target.parent.mkdir(parents=True, exist_ok=True)
        if vendor:
            upstream = SURF / "rust/vendor" / vendor
            if tree(upstream) != pin["reviewedVendorFiles"]:
                raise ValueError(f"Reviewed vendor source changed: {old}")
            shutil.copytree(upstream, target)
        else:
            unpack(archive, target.parent, f"{old}-{version}")
            (target.parent / f"{old}-{version}").rename(target)
        original = tree(target)
        if old == "v8":
            test_patch = ROOT / "v8-teardown-tests.patch"
            if digest(test_patch) != pin["releaseTestPatchSha256"]:
                raise ValueError("Release teardown test patch changed")
            subprocess.run(["patch", "-p1", "--batch", "--fuzz=0", "-i", str(test_patch)], cwd=target, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            if digest(target / "tests/test_api.rs") != pin["releaseTestFileSha256"]:
                raise ValueError("Release teardown tests changed")
            patched = tree(target)
            if {name for name in original if original[name] != patched.get(name)} != {"tests/test_api.rs"} or set(original) != set(patched):
                raise ValueError("Release test patch changed non-test source")
        # These are upstream packaging records, not the renamed distribution's
        # generated manifest/lock/VCS records. Preserve their exact bytes by name.
        for old_record, new_record in [
            ("Cargo.toml.orig", "UPSTREAM-Cargo.toml.txt"),
            ("Cargo.lock", "UPSTREAM-Cargo.lock.txt"),
            (".cargo_vcs_info.json", "UPSTREAM-vcs-info.json"),
        ]:
            reserved = target / old_record
            if reserved.exists():
                reserved.rename(target / new_record)
        (target / "Cargo.toml").write_text(manifest((target / "Cargo.toml").read_text(), old, version, new))
        if not (target / "LICENSE").exists() and not (target / "LICENSE.md").exists():
            shutil.copyfile(SURF / "rust/vendor/deno_crypto/LICENSE.md", target / "LICENSE.md")
        final = tree(target)
        receipt["packages"].append({"upstreamName": old, "name": new, "version": version, "archiveSha256": pin["archiveSha256"], "inputFiles": original, "candidateFiles": final, "candidateBytes": sum(p.stat().st_size for p in target.rglob("*") if p.is_file())})
    members = [f"packages/{new}" for _, _, new, _ in PACKAGES]
    (destination / "Cargo.toml").write_text('[workspace]\nresolver = "2"\nmembers = ' + json.dumps(members, indent=2) + "\n")
    (destination / "prepared-source-identity.json").write_text(json.dumps(receipt, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-home", type=Path, default=Path.home() / ".cargo")
    parser.add_argument("--archive-dir", type=Path, help="Private pinned archive directory; never Cargo's cache")
    parser.add_argument("--download", action="store_true", help="Populate private archives from verified cache bytes or exact upstream URLs")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.download and args.archive_dir is None:
        parser.error("--download requires a private --archive-dir")
    if not args.check and args.output.exists():
        raise ValueError("Refusing to overwrite existing output")
    with tempfile.TemporaryDirectory(prefix="turbo-surf-deno-prepare-") as scratch:
        generated = Path(scratch)
        prepare(generated, args.cargo_home, args.archive_dir, args.download)
        if args.check:
            for name in ["packages", "Cargo.toml", "prepared-source-identity.json"]:
                expected, actual = generated / name, args.output / name
                same = tree(expected) == tree(actual) if expected.is_dir() else expected.read_bytes() == actual.read_bytes()
                if not same:
                    raise ValueError(f"Prepared candidate differs: {name}")
        else:
            shutil.copytree(generated, args.output)
    print("Prepared source candidates match pins." if args.check else "Prepared eight source candidates; no Cargo or publishing invoked.")


if __name__ == "__main__":
    main()
