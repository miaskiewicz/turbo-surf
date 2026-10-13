#!/usr/bin/env python3
"""Check release metadata before the normal local and CI gates (Python 3.11+)."""
import json
import re
import tomllib
from pathlib import Path


def check_versions(root: Path) -> str:
    package = json.loads((root / "package.json").read_text())
    version = package["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[\w.-]+)?", version):
        raise ValueError(f"Invalid package version: {version}")

    def check(actual, path):
        if actual != version:
            raise ValueError(f"{path}: expected {version}, found {actual}")

    lock = json.loads((root / "package-lock.json").read_text())
    check(lock["version"], "package-lock.json")
    check(lock["packages"][""]["version"], "package-lock.json root package")
    workspace = tomllib.loads((root / "rust/Cargo.toml").read_text())
    check(workspace["workspace"]["package"]["version"], "Rust workspace")
    check(
        json.loads((root / "rust/crates/turbo-surf-napi/package.json").read_text())["version"],
        "NAPI package",
    )
    check(
        tomllib.loads((root / "rust/crates/turbo-surf-py/pyproject.toml").read_text())["project"]["version"],
        "Python package",
    )
    members = set()
    for member in workspace["workspace"]["members"]:
        path = root / "rust" / member / "Cargo.toml"
        crate = tomllib.loads(path.read_text())
        members.add(crate["package"]["name"])
        if crate["package"]["version"] != {"workspace": True}:
            raise ValueError(f"{path}: crate version must inherit the workspace")
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, dependency in crate.get(section, {}).items():
                if isinstance(dependency, dict) and dependency.get("path", "").startswith("../turbo-surf"):
                    check(dependency.get("version"), f"{path}: {name}")
    rust_lock = tomllib.loads((root / "rust/Cargo.lock").read_text())
    locked_members = set()
    for crate in rust_lock["package"]:
        if crate["name"] in members:
            check(crate["version"], f"Rust lockfile: {crate['name']}")
            locked_members.add(crate["name"])
    if locked_members != members:
        raise ValueError(f"Rust lockfile missing workspace packages: {members - locked_members}")
    if f"Status: **v{version} " not in (root / "README.md").read_text():
        raise ValueError(f"README status must report v{version}")
    if f"## [{version}]" not in (root / "CHANGELOG.md").read_text():
        raise ValueError(f"CHANGELOG must contain [{version}]")
    return version


if __name__ == "__main__":
    print(f"Release versions agree: {check_versions(Path(__file__).resolve().parents[1])}")
