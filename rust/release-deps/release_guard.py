#!/usr/bin/env python3
"""Tag-time release guard for the Surf crates (run by rust-crates-publish.yml).

The pre-publication workspace resolves the turbo-dom crates and the Deno family
0.x.1 from Git through `[patch.crates-io]` (PUBLISHING.md). A tagged release must
resolve everything from crates.io instead, so before the five Surf crates publish
this checks that:

- `rust/Cargo.toml` has no `[patch.*]` table and `rust/Cargo.lock` no
  `[[patch.unused]]` entry;
- no locked package comes from a Git or path source;
- every pre-publication dependency (the turbo-dom crates, turbo-dom-v8 and the
  Deno family) is locked from the crates.io registry at a version the crates.io
  sparse index lists and has not yanked.

Usage: python3 rust/release-deps/release_guard.py [--offline]
(`--offline` skips the registry lookups; the source checks still run).
"""

import argparse
import json
import re
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

RUST = Path(__file__).resolve().parents[1]
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"

# Published before Surf: from turbo-dom (v0.6.0 tag), then the Deno family from
# release-dependencies.yml. These must be locked; turbo-surf-deno-bench-util is
# a Deno-family member outside Surf's graph, checked only when locked.
TURBO_DOM = ("turbo-dom", "turbo-dom-layout", "turbo-dom-raster", "turbo-dom-browser-env", "turbo-dom-v8")
DENO_FAMILY = (
    "turbo-surf-deno-v8",
    "turbo-surf-serde-v8",
    "turbo-surf-deno-core",
    "turbo-surf-deno-crypto",
    "turbo-surf-deno-web",
    "turbo-surf-deno-webidl",
)
OPTIONAL = ("turbo-surf-deno-bench-util",)
PREREQUISITES = TURBO_DOM + DENO_FAMILY + OPTIONAL


def index_key(name):
    """The crates.io sparse-index path of a package name."""
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def registry_versions(name):
    """{version: yanked} for `name` from the crates.io sparse index ({} if unknown)."""
    request = urllib.request.Request(
        "https://index.crates.io/" + index_key(name),
        headers={"User-Agent": "turbo-surf-release-guard"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            lines = response.read().decode().splitlines()
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return {}
        raise
    return {entry["vers"]: bool(entry.get("yanked")) for entry in map(json.loads, lines) if entry}


def problems(manifest_text, lock, lookup):
    """Every reason the release tree is not registry-only; empty when it is.

    `lookup(name)` returns {version: yanked} from the registry, or None to skip
    the registry check (offline).
    """
    found = []
    for line in manifest_text.splitlines():
        if re.match(r"\s*\[patch[.\]]", line):
            found.append(f"rust/Cargo.toml still has a patch table: {line.strip()}")
    for entry in lock.get("patch", {}).get("unused", []):
        found.append(f"rust/Cargo.lock still records an unused patch: {entry['name']} {entry['version']}")
    locked = {}
    for package in lock.get("package", []):
        source = package.get("source")
        if source is not None and not source.startswith("registry+"):
            found.append(f"{package['name']} {package['version']} is locked from {source}")
        if package["name"] in PREREQUISITES:
            locked.setdefault(package["name"], []).append(package)
    for name in TURBO_DOM + DENO_FAMILY:
        if name not in locked:
            found.append(f"{name} is not in rust/Cargo.lock")
    for name, packages in sorted(locked.items()):
        if len(packages) != 1:
            versions = ", ".join(package["version"] for package in packages)
            found.append(f"rust/Cargo.lock must contain exactly one {name}, found {versions}")
        for package in packages:
            version = package["version"]
            if package.get("source") != REGISTRY:
                found.append(f"{name} {version} must come from crates.io, not {package.get('source')}")
                continue
            published = lookup(name)
            if published is None:
                continue
            if version not in published:
                found.append(f"{name} {version} is not on crates.io (publish it first)")
            elif published[version]:
                found.append(f"{name} {version} is yanked on crates.io")
    return found


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--offline", action="store_true", help="skip the crates.io lookups")
    parser.add_argument("--rust", type=Path, default=RUST, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    manifest = (args.rust / "Cargo.toml").read_text()
    lock = tomllib.loads((args.rust / "Cargo.lock").read_text())
    found = problems(manifest, lock, (lambda name: None) if args.offline else registry_versions)
    for problem in found:
        print(f"::error::{problem}", file=sys.stderr)
    if found:
        print("Release guard failed: drop the patch table and re-lock (PUBLISHING.md).", file=sys.stderr)
        return 1
    checked = "sources" if args.offline else "sources and crates.io availability"
    print(f"Release guard passed ({checked}): {', '.join(TURBO_DOM + DENO_FAMILY)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
