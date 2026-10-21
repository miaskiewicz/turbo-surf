#!/usr/bin/env python3
"""Verify actual packaged Deno archives against prepared source identities."""

import argparse
import copy
import hashlib
import importlib.util
import json
import tarfile
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("deno_prepare", ROOT / "prepare.py")
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


def normalized(manifest, resolver=None):
    result = copy.deepcopy(manifest)
    if resolver is not None:
        # Cargo materializes the enclosing workspace's resolver in the archive.
        result["package"].setdefault("resolver", resolver)
    for section in [result] + list(result.get("target", {}).values()):
        for category in ["dependencies", "dev-dependencies", "build-dependencies"]:
            for dependency in section.get(category, {}).values():
                if isinstance(dependency, dict):
                    dependency.pop("path", None)
    return result


def check(prepared, archives):
    receipt = json.loads((prepared / "prepared-source-identity.json").read_text())
    workspace = prepared / "Cargo.toml"
    resolver = tomllib.loads(workspace.read_text())["workspace"].get("resolver") if workspace.exists() else None
    expected = {(new, version) for _, version, new, _ in prepare.PACKAGES}
    if {(p["name"], p["version"]) for p in receipt["packages"]} != expected or len(receipt["packages"]) != len(expected):
        raise ValueError("Unexpected candidate package closure")
    results = []
    for package in receipt["packages"]:
        name, version = package["name"], package["version"]
        source = prepared / "packages" / name
        if prepare.tree(source) != package["candidateFiles"]:
            raise ValueError(f"Prepared source changed: {name}")
        manifest = tomllib.loads((source / "Cargo.toml").read_text())
        archive = archives / f"{name}-{version}.crate"
        prefix = f"{name}-{version}"
        contents = {}
        with tarfile.open(archive) as tar:
            for member in tar.getmembers():
                relative = Path(member.name).relative_to(prefix)
                if ".." in relative.parts or not (member.isdir() or member.isfile()):
                    raise ValueError("Unexpected archive entry")
                if member.isfile():
                    key = relative.as_posix()
                    if key in contents:
                        raise ValueError("Duplicate archive entry")
                    contents[key] = tar.extractfile(member).read()
        actual = tomllib.loads(contents["Cargo.toml"].decode())
        if actual != normalized(manifest, resolver):
            raise ValueError(f"Packaged manifest changes more than paths: {name}")
        if contents["Cargo.toml.orig"] != (source / "Cargo.toml").read_bytes():
            raise ValueError("Original packaged manifest differs from prepared input")
        for section in [actual] + list(actual.get("target", {}).values()):
            for category in ["dependencies", "dev-dependencies", "build-dependencies"]:
                for alias, dep in section.get(category, {}).items():
                    if isinstance(dep, dict) and any(k in dep for k in ["path", "git", "rev", "branch", "tag", "workspace"]):
                        raise ValueError("Packaged dependency retains a nonregistry source")
        if "patch" in actual or "workspace" in actual:
            raise ValueError("Packaged manifest retains a root override")
        for path, payload in contents.items():
            if path in ["Cargo.toml", "Cargo.toml.orig", "Cargo.lock", ".cargo_vcs_info.json"]:
                continue
            if hashlib.sha256(payload).hexdigest() != package["candidateFiles"].get(path):
                raise ValueError(f"Packaged source changed: {name}/{path}")
        required = {p for p in package["candidateFiles"] if p.endswith((".rs", ".js"))}
        if not required.issubset(contents):
            raise ValueError(f"Packaged Rust/JavaScript source missing: {name}")
        licenses = [p for p in contents if Path(p).name in ["LICENSE", "LICENSE.md"]]
        if not licenses:
            raise ValueError(f"Packaged license missing: {name}")
        results.append({"name": name, "version": version, "archiveSha256": prepare.digest(archive),
                        "files": len(contents), "licenses": licenses, "sourceAndNormalizedManifestVerified": True})
    return {"scope": "Actual Cargo archive source/license/normalization proof; no compilation or published-registry proof.",
            "packages": results, "preparedIdentitySha256": prepare.digest(prepared / "prepared-source-identity.json")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepared", type=Path, required=True)
    parser.add_argument("--archives", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    result = check(args.prepared, args.archives)
    with args.receipt.open("x") as stream:
        stream.write(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"packagesVerified": len(result["packages"]), "receipt": str(args.receipt)}))


if __name__ == "__main__":
    main()
