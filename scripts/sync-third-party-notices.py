#!/usr/bin/env python3
"""Keep notices in the published Rust and Python package roots."""
import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    notices = (root / "THIRD_PARTY_NOTICES.md").read_bytes()
    destinations = [
        root / "rust/crates/turbo-surf-render/THIRD_PARTY_NOTICES.md",
        root / "rust/crates/turbo-surf-py/python/turbo_surf/THIRD_PARTY_NOTICES.md",
    ]
    stale = []
    for destination in destinations:
        if options.check:
            if not destination.exists() or destination.read_bytes() != notices:
                stale.append(str(destination.relative_to(root)))
        else:
            destination.write_bytes(notices)
    if stale:
        parser.exit(1, "Stale third-party notices: " + ", ".join(stale) + "\n")
    print("Third-party notices match all package copies.")


if __name__ == "__main__":
    main()
