#!/usr/bin/env python3
"""Keep package notices and Rust package MIT licenses synchronized."""
import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    copies = [
        (
            root / "THIRD_PARTY_NOTICES.md",
            [
                root / "rust/crates/turbo-surf-render/THIRD_PARTY_NOTICES.md",
                root / "rust/crates/turbo-surf-py/python/turbo_surf/THIRD_PARTY_NOTICES.md",
            ],
        ),
        (
            root / "LICENSE",
            [root / "rust/LICENSE"]
            + [
                manifest.parent / "LICENSE"
                for manifest in sorted((root / "rust/crates").glob("turbo-surf-*/Cargo.toml"))
            ],
        ),
    ]
    stale = []
    for source, destinations in copies:
        content = source.read_bytes()
        for destination in destinations:
            if options.check:
                if not destination.exists() or destination.read_bytes() != content:
                    stale.append(str(destination.relative_to(root)))
            else:
                destination.write_bytes(content)
    if stale:
        parser.exit(1, "Stale package notices or license: " + ", ".join(stale) + "\n")
    print("Package notices and Rust package licenses match their root sources.")


if __name__ == "__main__":
    main()
