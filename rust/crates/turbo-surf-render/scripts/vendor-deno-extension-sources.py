#!/usr/bin/env python3
"""Reproduce portable Deno extension JS from checksum-pinned published archives.

Candidate mode (pre-publication): when the render manifest and lockfile select a
named distribution from a Git or path source (a `[patch.crates-io]` candidate),
`--check` reads that candidate's sources instead of a registry archive. The JS
and license bytes must still equal the pins; only the lock/archive checksum is
skipped, because Git/path sources have none.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[4]
DESTINATION = ROOT / "rust/crates/turbo-surf-render/src/vendor/deno_extensions"
PINS = DESTINATION / "pins.json"
TABLE = ROOT / "rust/crates/turbo-surf-render/src/deno_extension_source_table.rs"
KINDS = {"js": "Js", "esm": "Esm", "lazy_loaded_js": "LazyJs", "lazy_loaded_esm": "LazyEsm"}
# Candidate distributions append exactly this module declaration to the official
# lib.rs (rust/release-deps/deno/prepare.py LIB_SUFFIX); nothing else may differ.
CANDIDATE_LIB_SUFFIX = ("\n// turbo-surf distribution: compile-time copies of this extension's JavaScript.\n"
                        "pub mod embedded_sources;\n").encode()
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(value, message):
    if not value:
        raise ValueError(message)


def needed_files(package):
    return ["lib.rs", package["licenseFile"], *[row["file"] for row in package["sources"]]]


def archive_inputs(package, archive):
    require(not archive.is_symlink() and archive.is_file(), "Archive must be a regular file")
    require(archive.stat().st_size <= 32 * 1024**2, "Archive exceeds size bound")
    require(digest(archive.read_bytes()) == package["archiveSha256"], "Published archive checksum changed")
    prefix = package["package"] + "-" + package["version"]
    entries = {}
    with tarfile.open(archive, "r:gz") as tar:
        total = 0
        for member in tar:
            name = PurePosixPath(member.name)
            require(not name.is_absolute() and name.parts and name.parts[0] == prefix and ".." not in name.parts and "\\" not in member.name, "Unsafe archive member")
            require(member.isdir() or member.isfile(), "Linked or special archive member")
            require(member.name not in entries, "Duplicate archive member")
            entries[member.name] = member
            total += member.size
            require(0 <= member.size <= 8 * 1024**2 and total <= 64 * 1024**2, "Expanded archive exceeds bounds")
        contents = {}
        for relative in needed_files(package):
            entry = entries.get(prefix + "/" + relative)
            require(entry is not None and entry.isfile(), "Published source missing: " + relative)
            with tar.extractfile(entry) as stream:
                contents[relative] = stream.read()
    return verified_inputs(package, contents)


def candidate_inputs(package, directory):
    """Read a Git/path candidate checkout; its bytes face the same pins."""
    directory = Path(directory)
    require(not directory.is_symlink() and directory.is_dir(), "Candidate source must be a directory")
    contents = {}
    for relative in needed_files(package):
        path = directory / relative
        require(not path.is_symlink() and path.is_file(), "Candidate source missing: " + relative)
        require(path.stat().st_size <= 8 * 1024**2, "Candidate source exceeds bounds")
        contents[relative] = path.read_bytes()
    declaration = contents["lib.rs"]
    # The candidate's only lib.rs change is the appended embedded-source module.
    if digest(declaration) != package["declarationSha256"] and declaration.endswith(CANDIDATE_LIB_SUFFIX):
        contents["lib.rs"] = declaration[:-len(CANDIDATE_LIB_SUFFIX)]
    return verified_inputs(package, contents)


def verified_inputs(package, contents):
    require(digest(contents["lib.rs"]) == package["declarationSha256"], "Official extension declaration changed")
    require(digest(contents[package["licenseFile"]]) == package["licenseSha256"], "Upstream license changed")
    actual = []
    for kind, listing in re.findall(r"(lazy_loaded_js|lazy_loaded_esm|esm|js)\s*=\s*\[([^]]*)\]", contents["lib.rs"].decode(), re.S):
        actual.extend((kind, name) for name in re.findall(r'"([^"\n]+\.js)"', listing))
    require(actual == [(row["kind"], row["file"]) for row in package["sources"]], "Official extension source inventory changed")
    for row in package["sources"]:
        data = contents[row["file"]]
        require(row["specifier"] == "ext:" + package["alias"] + "/" + row["file"], "Unexpected source specifier")
        require(data.isascii() and len(data) == row["bytes"] and digest(data) == row["sha256"], "Official extension source bytes changed")
    return contents


def check_dependencies(root, packages):
    """Return {package: (version, source)} for every candidate-sourced extension."""
    manifest = tomllib.loads((root / "rust/crates/turbo-surf-render/Cargo.toml").read_text())
    lock = tomllib.loads((root / "rust/Cargo.lock").read_text())
    candidates = {}
    for package in packages:
        selected = manifest["dependencies"][package["alias"]]
        require(isinstance(selected, dict) and selected.get("package") == package["package"], "Render extension version/package drift")
        locked = [row for row in lock["package"] if row["name"] == package["package"]]
        require(len(locked) == 1 and selected.get("version") == "=" + locked[0]["version"], "Render extension version/package drift")
        source = locked[0].get("source")
        if source is None or source.startswith("git+"):
            # A [patch] candidate (Git rev or local path): no archive checksum exists.
            require("checksum" not in locked[0], "Locked extension archive drift")
            candidates[package["package"]] = (locked[0]["version"], source)
            continue
        require(source == REGISTRY and locked[0]["version"] == package["version"] and locked[0].get("checksum") == package["archiveSha256"], "Locked extension archive drift")
    return candidates


def candidate_directories(root, candidates):
    """Locate candidate manifests through Cargo's own resolution (offline)."""
    output = subprocess.run(["cargo", "metadata", "--offline", "--locked", "--format-version", "1",
                             "--manifest-path", str(root / "rust/Cargo.toml")],
                            check=True, stdout=subprocess.PIPE, text=True).stdout
    found = {}
    for row in json.loads(output)["packages"]:
        if row["name"] in candidates and row["version"] == candidates[row["name"]][0]:
            require(row["name"] not in found, "Duplicate candidate package: " + row["name"])
            found[row["name"]] = Path(row["manifest_path"]).parent
    require(found.keys() == candidates.keys(), "Candidate sources are not available offline; build first or pass --candidate-dir")
    return found


def reproduce(packages, archives, candidates=None):
    expected, table, hashes = {}, [], []
    seen = set()
    candidates = candidates or {}
    for package in packages:
        if package["package"] in candidates:
            content = candidate_inputs(package, candidates[package["package"]])
        else:
            content = archive_inputs(package, archives[package["package"]])
        directory = package["alias"]
        expected[Path(directory) / package["licenseFile"]] = content[package["licenseFile"]]
        for row in package["sources"]:
            require(row["specifier"] not in seen, "Duplicate pinned source specifier")
            seen.add(row["specifier"])
            relative = Path(directory) / row["file"]
            expected[relative] = content[row["file"]]
            table.append('    EmbeddedSource { extension: "' + directory + '", kind: SourceKind::' + KINDS[row["kind"]] + ', specifier: "' + row["specifier"] + '", code: deno_core::ascii_str_include!("vendor/deno_extensions/' + relative.as_posix() + '") },')
            hashes.append('    ("' + row["specifier"] + '", "' + row["sha256"] + '"),')
    generated = "// Generated by rust/crates/turbo-surf-render/scripts/vendor-deno-extension-sources.py; do not edit.\n// Exact published Deno JS, Copyright 2018-2026 the Deno authors, MIT.\n#[rustfmt::skip]\nconst EMBEDDED: &[EmbeddedSource] = &[\n" + "\n".join(table) + "\n];\n\n#[cfg(test)]\n#[rustfmt::skip]\nconst SOURCE_HASHES: &[(&str, &str)] = &[\n" + "\n".join(hashes) + "\n];\n"
    return expected, generated.encode()


def existing(directory):
    require(not directory.is_symlink(), "Linked generated source root")
    result = {}
    if directory.exists():
        for path in directory.rglob("*"):
            require(not path.is_symlink(), "Linked generated source")
            if path.is_file():
                result[path.relative_to(directory)] = path.read_bytes()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive-dir", type=Path, help="Directory containing the three exact .crate archives; no downloads")
    parser.add_argument("--candidate-dir", type=Path, help="Prepared candidate tree (packages/<name>/) for Git/path-sourced extensions; default: Cargo metadata")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    args = parser.parse_args()
    pins = json.loads(PINS.read_text())
    require(pins["schema"] == 1, "Unknown source pin schema")
    packages = pins["packages"]
    require(len(packages) == 3 and len({row["package"] for row in packages}) == 3, "Expected three official extensions")
    candidate_versions = check_dependencies(ROOT, packages)
    require(not candidate_versions or args.check, "Candidate sources are verified with --check only; --write reproduces published archives")
    if not candidate_versions:
        candidates = {}
    elif args.candidate_dir:
        candidates = {name: args.candidate_dir / "packages" / name for name in candidate_versions}
    else:
        candidates = candidate_directories(ROOT, candidate_versions)
    archives = {}
    for package in packages:
        if package["package"] in candidates:
            continue
        name = package["package"] + "-" + package["version"] + ".crate"
        if args.archive_dir:
            archive = args.archive_dir / name
        else:
            cargo = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
            matches = list((cargo / "registry/cache").glob("*/" + name))
            require(len(matches) == 1, "Pass --archive-dir containing " + name)
            archive = matches[0]
        archives[package["package"]] = archive
    expected, table = reproduce(packages, archives, candidates)
    expected[Path("pins.json")] = PINS.read_bytes()
    actual = existing(DESTINATION)
    require(not TABLE.is_symlink(), "Linked generated source table")
    if args.check:
        differences = sorted(str(path) for path in actual.keys() | expected.keys() if actual.get(path) != expected.get(path))
        require(not differences, "Embedded sources differ: " + ", ".join(differences))
        require(TABLE.is_file() and not TABLE.is_symlink() and TABLE.read_bytes() == table, "Generated extension source table changed")
    else:
        for path in actual.keys() - expected.keys():
            (DESTINATION / path).unlink()
        for relative, data in expected.items():
            path = DESTINATION / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        TABLE.write_bytes(table)
    origin = "candidate (" + ", ".join(sorted(candidates)) + ") and published" if candidates else "published"
    print("Verified exact " + origin + " Deno extension sources and licenses: " + str(len(expected) - len(packages) - 1) + " modules")


if __name__ == "__main__":
    main()
