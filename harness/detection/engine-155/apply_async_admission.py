#!/usr/bin/env python3
"""Apply the pinned experimental async-admission patch to a managed SDK only."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("sdk", type=Path, help="managed SDK directory containing deno_core")
parser.add_argument("--record-provenance", action="store_true")
parser.add_argument("--reverse", action="store_true", help="restore the exact pinned preimage for controls")
args = parser.parse_args()
if args.reverse and args.record_provenance:
    parser.error("Cannot record patched provenance while reversing")
root = Path(__file__).resolve().parent
sdk = args.sdk.resolve()
source = sdk / "deno_core"
if not (sdk / "cargo-config.toml").is_file() or not (sdk / "provenance.json").is_file():
    raise SystemExit("Expected an explicitly prepared managed SDK, not a Cargo cache")
provenance = json.loads((sdk / "provenance.json").read_text())
if provenance.get("chromeMajor") != 155 or provenance.get("denoCoreVersion") != "0.412.0":
    raise SystemExit("Expected managed Chrome155 / Deno0.412.0 provenance")
manifest = json.loads((root / "async-admission-sources.json").read_text())
def record_provenance():
    if args.reverse:
        provenance.pop("asyncAdmissionTransaction", None)
        (sdk / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        return
    if not args.record_provenance:
        return
    provenance["asyncAdmissionTransaction"] = {
        "patchSha256": hashlib.sha256((root / "async-admission.patch").read_bytes()).hexdigest(),
        "sources": manifest,
        "verification": "source-preimage-postimage-and-reverse-check",
    }
    (sdk / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
if args.reverse:
    manifest = {name: {"before": entry["after"], "after": entry["before"]} for name, entry in manifest.items()}
def hashes():
    return {name: hashlib.sha256((source / name).read_bytes()).hexdigest() for name in manifest}
actual = hashes()
patch = root / "async-admission.patch"
if all(actual[name] == entry["after"] for name, entry in manifest.items()):
    undo = [] if args.reverse else ["--reverse"]
    subprocess.run(["git", "apply", *undo, "--check", str(patch)], cwd=source, check=True)
    record_provenance()
    print("Async admission transaction patch already verified")
    raise SystemExit(0)
if not all(actual[name] == entry["before"] for name, entry in manifest.items()):
    raise SystemExit("Managed Deno source hash mismatch; refusing partial or unknown source")
direction = ["--reverse"] if args.reverse else []
subprocess.run(["git", "apply", *direction, "--check", str(patch)], cwd=source, check=True)
subprocess.run(["git", "apply", *direction, str(patch)], cwd=source, check=True)
if not all(hashes()[name] == entry["after"] for name, entry in manifest.items()):
    raise SystemExit("Patched Deno postimage mismatch")
undo = [] if args.reverse else ["--reverse"]
subprocess.run(["git", "apply", *undo, "--check", str(patch)], cwd=source, check=True)
record_provenance()
print("Verified experimental async admission transaction patch")
