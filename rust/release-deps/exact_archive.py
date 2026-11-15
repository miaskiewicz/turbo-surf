#!/usr/bin/env python3
"""Explicit hash-pinned upload of reviewed Cargo archives; never repack them."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import struct
import tarfile
import time
import tomllib
import urllib.request
import urllib.error

ENDPOINT = "https://crates.io/api/v1/crates/new"
ORDER = [("turbo-surf-deno-core", "0.412.3"),
         ("turbo-surf-deno-bench-util", "0.253.3"),
         ("turbo-surf-deno-crypto", "0.273.3"),
         ("turbo-surf-deno-webidl", "0.259.3"),
         ("turbo-surf-deno-web", "0.290.3")]
LIMIT = 64 * 1024 * 1024


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def archive_files(path):
    if path.is_symlink() or path.stat().st_size > LIMIT:
        raise ValueError("Invalid archive input")
    files, prefix, size = {}, None, 0
    with tarfile.open(path) as source:
        for member in source.getmembers():
            parts = PurePosixPath(member.name).parts
            if not parts or member.name.startswith("/") or ".." in parts:
                raise ValueError("Archive path escapes its root")
            prefix = prefix or parts[0]
            if parts[0] != prefix or not (member.isfile() or member.isdir()):
                raise ValueError("Unexpected archive member")
            if member.isfile():
                key = "/".join(parts[1:])
                size += member.size
                if not key or key in files or size > LIMIT or len(files) >= 1024:
                    raise ValueError("Invalid archive inventory")
                files[key] = source.extractfile(member).read()
    if "Cargo.lock" in files:
        raise ValueError("Exact mode requires the reviewed no-lock payload")
    return files


def relative_file(value, manifest_path):
    if value is None:
        return None
    path = Path(value)
    if path.is_absolute():
        path = path.relative_to(manifest_path.parent)
    key = path.as_posix()
    if ".." in path.parts:
        raise ValueError("Metadata file escapes the package")
    return key


def dependency(value):
    if value.get("path") or value.get("registry"):
        raise ValueError("Exact mode only accepts normal crates.io dependency declarations")
    source = value.get("source")
    if source and not source.startswith("registry+"):
        raise ValueError("Non-registry dependency declaration")
    return {"name": value["name"], "version_req": value["req"],
            "features": value["features"], "optional": value["optional"],
            "default_features": value["uses_default_features"],
            "target": value.get("target"), "kind": value.get("kind") or "normal",
            "registry": None, "explicit_name_in_toml": value.get("rename")}


def package_plan(archive, cargo_metadata):
    archive = Path(archive).resolve()
    files = archive_files(archive)
    manifest = tomllib.loads(files["Cargo.toml"].decode())
    identity = manifest["package"]
    pair = (identity["name"], identity["version"])
    if pair not in ORDER or any(key in manifest for key in ("workspace", "patch")):
        raise ValueError("Package identity or manifest is outside the reviewed five")
    matches = [p for p in cargo_metadata["packages"] if (p["name"], p["version"]) == pair]
    if len(matches) != 1:
        raise ValueError("Cargo metadata identity is not unique")
    package = matches[0]
    manifest_path = Path(package["manifest_path"]).resolve()
    for name, payload in files.items():
        path = manifest_path.parent / name
        if path.is_symlink() or path.read_bytes() != payload:
            raise ValueError("Cargo metadata source differs from the actual archive")
    readme = relative_file(package.get("readme"), manifest_path)
    license_file = relative_file(package.get("license_file"), manifest_path)
    result = {"name": pair[0], "vers": pair[1],
              "deps": [dependency(d) for d in package["dependencies"]],
              "features": package["features"], "authors": package["authors"],
              "readme": files[readme].decode() if readme else None,
              "readme_file": readme, "license_file": license_file,
              "badges": manifest.get("badges", {})}
    for key in ("description", "documentation", "homepage", "license", "repository", "links", "rust_version"):
        result[key] = package.get(key)
    for key in ("keywords", "categories"):
        result[key] = package.get(key, [])
    metadata = encoded(result)
    raw = archive.read_bytes()
    body = wire_body(metadata, raw)
    return {"name": pair[0], "version": pair[1], "archive": str(archive),
            "archiveSha256": digest(raw), "archiveBytes": len(raw),
            "manifestPath": str(manifest_path), "metadata": result,
            "metadataSha256": digest(metadata), "bodySha256": digest(body),
            "sourceFiles": {name: digest(payload) for name, payload in files.items()}}


def wire_body(metadata, archive):
    if len(metadata) > 1024 * 1024 or len(archive) > LIMIT:
        raise ValueError("Upload payload exceeds its bound")
    return struct.pack("<I", len(metadata)) + metadata + struct.pack("<I", len(archive)) + archive


def build_plan(archives, metadata_path, expected_commit, evidence):
    if not re.fullmatch(r"[0-9a-f]{40}", expected_commit):
        raise ValueError("Source commit must be a full SHA")
    metadata_path = Path(metadata_path).resolve()
    raw_metadata = metadata_path.read_bytes()
    packages = [package_plan(p, json.loads(raw_metadata)) for p in archives]
    by_name = {p["name"]: p for p in packages}
    if len(packages) != 5 or set(by_name) != {p[0] for p in ORDER}:
        raise ValueError("Exact mode requires precisely the five reviewed versions")
    return {"mode": "exact-reviewed-no-lock-archives", "endpoint": ENDPOINT,
            "sourceCommit": expected_commit, "scriptSha256": digest(Path(__file__).read_bytes()),
            "standardPublisherSha256": digest(Path(__file__).with_name("publish.py").read_bytes()),
            "cargoMetadata": str(metadata_path), "cargoMetadataSha256": digest(raw_metadata),
            "evidence": {str(Path(p).resolve()): digest(Path(p).read_bytes()) for p in evidence},
            "packages": [by_name[name] for name, _ in ORDER]}


def validate_plan(path, approved_sha, expected_commit):
    path = Path(path)
    raw = path.read_bytes()
    if digest(raw) != approved_sha:
        raise ValueError("Plan differs from the explicitly approved SHA")
    plan = json.loads(raw)
    if plan["sourceCommit"] != expected_commit or plan["endpoint"] != ENDPOINT:
        raise ValueError("Source or registry differs from approval")
    rebuilt = build_plan([p["archive"] for p in plan["packages"]], plan["cargoMetadata"],
                         expected_commit, plan["evidence"])
    if rebuilt != plan:
        raise ValueError("Archive, metadata, source, evidence or uploader changed")
    return plan


def checked_commit(value):
    # Reuse the existing publisher's source/dirty-tree guard; no standard mode changes.
    from publish import commit_matches
    commit_matches(value)


def credential():
    value = os.environ.get("CARGO_REGISTRY_TOKEN")
    if not value or "\r" in value or "\n" in value:
        raise ValueError("An explicit publishing credential is required")
    return value


def registry_check(package, output, deadline):
    from publish import Registry
    destination = output / (package["name"] + "-" + package["version"] + ".crate")
    exists = Registry().version(package["name"], package["version"],
                                Path(package["archive"]), destination, deadline)
    if exists and digest(destination.read_bytes()) != package["archiveSha256"]:
        raise ValueError("Registry version exists with a different RAW archive; stop")
    return exists


class RejectRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        raise urllib.error.HTTPError(request.full_url, code, "Registry redirect refused", headers, None)


def no_redirect_transport(request, timeout):
    if request.full_url != ENDPOINT or request.get_method() != "PUT":
        raise ValueError("Exact upload endpoint or method differs")
    return urllib.request.build_opener(RejectRedirects()).open(request, timeout=timeout)


def send_exact(package, token, transport):
    body = wire_body(encoded(package["metadata"]), Path(package["archive"]).read_bytes())
    if digest(body) != package["bodySha256"]:
        raise ValueError("Upload bytes changed after approval")
    request = urllib.request.Request(ENDPOINT, data=body, method="PUT", headers={
        "Content-Type": "application/octet-stream", "Authorization": token})
    with transport(request, timeout=60) as response:
        payload = response.read(65537)
        if not 200 <= response.status < 300 or len(payload) > 65536:
            raise ValueError("Registry did not confirm the exact upload")
        if json.loads(payload).get("errors"):
            raise ValueError("Registry reported an error; inspect/read back before retry")


def upload_plan(path, approved_sha, expected_commit, output,
                token_provider=credential, transport=no_redirect_transport,
                commit_check=checked_commit, readback=registry_check):
    plan = validate_plan(path, approved_sha, expected_commit)
    commit_check(expected_commit)
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    token, results = None, []
    for package in plan["packages"]:
        deadline = time.monotonic() + 900
        exists = readback(package, output, deadline)
        if not exists:
            token = token or token_provider()  # After ALL approval/source checks.
            # No PUT retry: ambiguous failure requires explicit registry readback.
            send_exact(package, token, transport)
            from publish import pause
            while not readback(package, output, deadline):
                pause(deadline, 3)
        results.append({"name": package["name"], "version": package["version"],
                        "archiveSha256": package["archiveSha256"],
                        "disposition": "already-exact" if exists else "uploaded-and-read-back-exact"})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    plan = sub.add_parser("plan")
    plan.add_argument("--archive", type=Path, action="append", required=True)
    plan.add_argument("--cargo-metadata", type=Path, required=True)
    plan.add_argument("--expected-commit", required=True)
    plan.add_argument("--evidence", type=Path, action="append", required=True)
    plan.add_argument("--output", type=Path, required=True)
    upload = sub.add_parser("upload")
    upload.add_argument("--plan", type=Path, required=True)
    upload.add_argument("--approved-plan-sha", required=True)
    upload.add_argument("--expected-commit", required=True)
    upload.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.mode == "plan":
        result = build_plan(args.archive, args.cargo_metadata, args.expected_commit, args.evidence)
        with args.output.open("x") as stream:
            stream.write(json.dumps(result, indent=2) + "\n")
        print(digest(args.output.read_bytes()))
    else:
        print(json.dumps(upload_plan(args.plan, args.approved_plan_sha, args.expected_commit, args.output)))


if __name__ == "__main__":
    main()
