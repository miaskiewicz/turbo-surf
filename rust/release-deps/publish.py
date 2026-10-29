#!/usr/bin/env python3
"""Verify and explicitly publish the reviewed named dependency distributions.

Verification uses actual archives and an isolated directory source. Publication
uses the real registry, with Cargo verification enabled, and never accepts a
same-version package merely because it exists.
"""

import argparse
import email.utils
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import signal
import subprocess
import tarfile
import time
import tomllib
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
# The native V8 binding (turbo-dom-v8) is an external prerequisite published from
# the turbo-dom repository; the facade requires it from the registry.
ORDER = [
    ("turbo-surf-deno-v8", "0.4.1", "deno"),
    ("turbo-surf-serde-v8", "0.321.1", "deno"),
    ("turbo-surf-deno-core", "0.412.1", "deno"),
    ("turbo-surf-deno-bench-util", "0.253.1", "deno"),
    ("turbo-surf-deno-crypto", "0.273.1", "deno"),
    ("turbo-surf-deno-webidl", "0.259.1", "deno"),
    ("turbo-surf-deno-web", "0.290.1", "deno"),
    ("turbo-surf-btls-sys", "0.5.6", "transport"),
    ("turbo-surf-btls", "0.5.6", "transport"),
    ("turbo-surf-tokio-btls", "0.5.6", "transport"),
    ("turbo-surf-wreq", "0.16.1", "transport"),
    ("turbo-surf-wreq-util", "0.2.0", "transport"),
]
NATIVE_V8 = ("turbo-dom-v8", "150.4.")
RETIRED_RUNTIMES = {"v8", "v8x", "turbo-surf-v8", "deno_v8", "deno_core", "serde_v8", "deno_crypto",
                    "deno_web", "deno_webidl", "deno_bench_util"}
GENERATED = {"Cargo.lock", ".cargo_vcs_info.json"}
TRANSIENT = {429, 500, 502, 503, 504}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def source_files(path):
    result = {}
    for file in sorted(path.rglob("*")):
        if "target" in file.relative_to(path).parts:
            continue
        if file.is_symlink():
            raise ValueError("Candidate source contains a link")
        if file.is_file():
            result[file.relative_to(path).as_posix()] = digest(file)
    return result


def archive_payload(path, name, version):
    """Compare every source byte; only Cargo's two generated records are omitted."""
    files = {}
    seen = set()
    with tarfile.open(path) as archive:
        for entry in archive:
            parts = PurePosixPath(entry.name).parts
            if (not parts or parts[0] != f"{name}-{version}" or
                    PurePosixPath(entry.name).is_absolute() or ".." in parts or
                    not (entry.isfile() or entry.isdir())):
                raise ValueError("Invalid crate archive entry")
            if not entry.isfile():
                continue
            relative = "/".join(parts[1:])
            if not relative or relative in seen:
                raise ValueError("Duplicate crate archive entry")
            seen.add(relative)
            payload = archive.extractfile(entry).read()
            if relative in GENERATED:
                continue
            if relative in {"Cargo.toml", "Cargo.toml.orig"}:
                files[relative] = {"toml": tomllib.loads(payload.decode())}
            else:
                files[relative] = {"sha256": hashlib.sha256(payload).hexdigest(),
                                   "executable": bool(entry.mode & 0o111)}
    if not {"Cargo.toml", "Cargo.toml.orig"} <= files.keys():
        raise ValueError("Crate manifests are missing")
    package = files["Cargo.toml"]["toml"]["package"]
    if (package["name"], package["version"]) != (name, version):
        raise ValueError("Crate identity differs")
    return files


def require_equal_payload(expected, actual, name, version):
    if archive_payload(expected, name, version) != archive_payload(actual, name, version):
        raise ValueError(f"Published payload differs: {name}@{version}")


def audit_candidate(archive, source, name, version):
    payload = archive_payload(archive, name, version)
    original = tomllib.loads((source / "Cargo.toml").read_text())
    if payload["Cargo.toml.orig"]["toml"] != original:
        raise ValueError("Packaged original manifest differs from reviewed source")
    normalized = payload["Cargo.toml"]["toml"]
    if any(key in normalized for key in ("patch", "workspace")):
        raise ValueError("Published manifest requires a workspace override")
    for section in [normalized] + list(normalized.get("target", {}).values()):
        for category in ("dependencies", "dev-dependencies", "build-dependencies"):
            for dependency in section.get(category, {}).values():
                if isinstance(dependency, dict) and any(
                        key in dependency for key in ("path", "git", "rev", "workspace", "branch", "tag")):
                    raise ValueError("Published dependency is not a registry requirement")
    def dependencies(manifest):
        result = {}
        sections = [("root", manifest)] + list(manifest.get("target", {}).items())
        for target, section in sections:
            for category in ("dependencies", "dev-dependencies", "build-dependencies"):
                for alias, value in section.get(category, {}).items():
                    value = {"version": value} if isinstance(value, str) else dict(value)
                    for removed in ("path", "git", "rev", "branch", "tag"):
                        value.pop(removed, None)
                    result[(target, category, alias)] = value
        return result
    if (dependencies(original) != dependencies(normalized) or
            original.get("features", {}) != normalized.get("features", {}) or
            any(normalized.get("lib", {}).get(k) != v for k, v in original.get("lib", {}).items())):
        raise ValueError("Normalized manifest semantics differ")
    for relative, identity in payload.items():
        if relative in {"Cargo.toml", "Cargo.toml.orig"}:
            continue
        file = source / relative
        if (not file.is_file() or digest(file) != identity["sha256"] or
                bool(file.stat().st_mode & 0o111) != identity["executable"]):
            raise ValueError(f"Packaged source differs: {name}/{relative}")
    if not any(Path(p).name.upper().startswith(("LICENSE", "LICENCE", "COPYING")) for p in payload):
        raise ValueError("Package has no license payload")
    return payload


def retry_after(value, now=None):
    if not value:
        return None
    try:
        return max(0.0, float(value))
    except ValueError:
        try:
            return max(0.0, email.utils.parsedate_to_datetime(value).timestamp() -
                       (time.time() if now is None else now))
        except (TypeError, ValueError, OverflowError):
            return None


def pause(deadline, attempt, header=None, *, clock=time.monotonic, sleep=time.sleep):
    delay = retry_after(header)
    if delay is None:
        delay = min(30.0, 2.0 ** min(attempt, 5))
    delay = max(0.25, delay)
    remaining = deadline - clock()
    if remaining <= 0 or delay >= remaining:
        raise TimeoutError("Registry propagation/retry deadline exhausted")
    sleep(delay)


class Registry:
    def request(self, url, deadline):
        for attempt in range(12):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Registry request deadline exhausted")
            request = urllib.request.Request(url, headers={
                "User-Agent": "turbo-surf-release-dependencies (github.com/miaskiewicz/turbo-surf)",
                "Cache-Control": "no-cache",
            })
            try:
                with urllib.request.urlopen(request, timeout=min(30, remaining)) as response:
                    value = response.read(128 * 1024 * 1024 + 1)
                    if len(value) > 128 * 1024 * 1024:
                        raise ValueError("Registry response exceeds archive bound")
                    return value
            except urllib.error.HTTPError as error:
                if error.code == 404:
                    return None
                if error.code not in TRANSIENT:
                    raise RuntimeError(f"Registry HTTP {error.code}") from None
                pause(deadline, attempt, error.headers.get("Retry-After"))
            except urllib.error.URLError:
                pause(deadline, attempt)
        raise TimeoutError("Registry request attempt bound exhausted")

    def version(self, name, version, expected, destination, deadline):
        metadata = self.request(f"https://crates.io/api/v1/crates/{name}/{version}", deadline)
        if metadata is None:
            return False
        item = json.loads(metadata)["version"]
        if (item["crate"], item["num"]) != (name, version) or item.get("yanked") is not False:
            raise ValueError("Registry version identity/yank status differs")
        payload = self.request(f"https://static.crates.io/crates/{name}/{name}-{version}.crate", deadline)
        if payload is None:
            return False
        if hashlib.sha256(payload).hexdigest() != item["checksum"]:
            raise ValueError("Registry archive checksum differs")
        destination.write_bytes(payload)
        require_equal_payload(expected, destination, name, version)
        return True


class Runner:
    def __init__(self, output, target=None, publish_token=False):
        self.output = output
        self.records = []
        self.environment = os.environ.copy()
        self.environment.update(CARGO_TARGET_DIR=str(target or output / "target"))
        if not publish_token:
            self.environment.pop("CARGO_REGISTRY_TOKEN", None)
        for key in list(self.environment):
            if key.startswith(("RUSTY_V8_", "TURBO_SURF_ENGINE", "CARGO_PROFILE_", "CARGO_SOURCE_", "CARGO_REGISTRIES_")) or key in {
                    "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "V8_FROM_SOURCE", "V8_FORCE_DEBUG",
                    "DOCS_RS", "DENO_TRYBUILD", "CARGO_BUILD_TARGET", "CARGO_REGISTRY_DEFAULT",
                    "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"}:
                self.environment.pop(key)
        self.environment.update(CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0", RUSTUP_TOOLCHAIN="1.98.0", PYTHONDONTWRITEBYTECODE="1")

    def run(self, command, name, cwd=REPO, check=True, stdout_path=None):
        log = self.output / f"{len(self.records):03d}-{name}.log"
        start = time.monotonic()
        with log.open("wb") as stream:
            stdout = stdout_path.open("wb") if stdout_path else stream
            process = subprocess.Popen(command, cwd=cwd, env=self.environment,
                                       stdout=stdout, stderr=stream, start_new_session=True)
            try:
                process.wait(timeout=3600)
            except BaseException:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                self.records.append({"command": list(map(str, command)), "interruptedOrTimedOut": True,
                                     "exitCode": process.returncode, "log": log.name, "logSha256": digest(log)})
                write_json(self.output / "commands.json", self.records)
                raise
            finally:
                if stdout_path:
                    stdout.close()
        self.records.append({"command": list(map(str, command)), "exitCode": process.returncode,
                             "seconds": time.monotonic() - start, "log": log.name,
                             "logSha256": digest(log)})
        write_json(self.output / "commands.json", self.records)
        if check and process.returncode:
            raise RuntimeError(f"Command failed; see {log.name}")
        return process.returncode, log


def commit_matches(expected):
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("Expected source commit must be a complete SHA")
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    if actual != expected:
        raise ValueError("Checkout differs from approved source commit")
    dirty = subprocess.check_output([
        "git", "status", "--porcelain", "--untracked-files=all", "--",
        "rust/release-deps",
        ".github/workflows/release-dependencies.yml",
    ], cwd=REPO, text=True)
    if dirty.strip():
        raise ValueError("Release tooling checkout is dirty")


def candidates(roots):
    result = []
    for name, version, family in ORDER:
        path = roots[family] / "packages" / name
        manifest = tomllib.loads((path / "Cargo.toml").read_text())
        if (manifest["package"]["name"], manifest["package"]["version"]) != (name, version):
            raise ValueError("Candidate closure differs")
        receipt = json.loads((roots[family] / "prepared-source-identity.json").read_text())
        records = [row for row in receipt["packages"] if row["name"] == name]
        if len(records) != 1:
            raise ValueError("Candidate source identity differs")
        actual_files = source_files(path)
        # Only a newly generated root lock is outside the original candidate
        # identity. A reviewed upstream lock (e.g. wreq's) must remain exact.
        if "Cargo.lock" not in records[0]["candidateFiles"]:
            actual_files.pop("Cargo.lock", None)
        if actual_files != records[0]["candidateFiles"]:
            raise ValueError("Candidate source identity differs")
        result.append((name, version, path))
    return result


def verify(roots, output, expected):
    commit_matches(expected)
    tool_sha = digest(Path(__file__))
    packages = candidates(roots)
    runner = Runner(output)
    bootstrap = output / "bootstrap"
    archives = output / "archives"
    archives.mkdir()
    for family, path in roots.items():
        runner.run(["cargo", "fetch", "--manifest-path", str(path / "Cargo.toml")], f"fetch-{family}")
    runner.run(["python3", str(ROOT / "stage.py"), "--output", str(bootstrap)] +
               [argument for _, _, source in packages for argument in ("--candidate-source", str(source))], "bootstrap")
    for name, version, source in packages:
        runner.run(["cargo", "package", "--offline", "--allow-dirty", "--no-verify", "--locked",
                    "--config", str(bootstrap / "config.toml"), "--manifest-path", str(source / "Cargo.toml")], name + "-archive")
        shutil.copy2(output / "target/package" / f"{name}-{version}.crate", archives)
    stage = output / "actual-stage"
    runner.run(["python3", str(ROOT / "stage.py"), "--output", str(stage)] +
               [argument for name, version, _ in packages for argument in
                ("--candidate-archive", str(archives / f"{name}-{version}.crate"))], "actual-stage")
    runner.run(["python3", str(ROOT / "stage.py"), "--verify-stage", str(stage)], "source-before")
    rows = []
    for name, version, source in packages:
        archive = archives / f"{name}-{version}.crate"
        audit_candidate(archive, source, name, version)
        # This time Cargo unpacks and compiles its normalized distribution.
        runner.run(["cargo", "package", "--offline", "--allow-dirty", "--locked",
                    "--config", str(stage / "config.toml"), "--manifest-path", str(source / "Cargo.toml")], name + "-verify")
        require_equal_payload(archive, output / "target/package" / archive.name, name, version)
        rows.append({"name": name, "version": version, "source": str(source),
                     "sourceFiles": source_files(source), "archive": str(archive), "archiveSha256": digest(archive),
                     "sourceModes": {p: bool((source / p).stat().st_mode & 0o111)
                                     for p in source_files(source)}})
    for family in roots:
        consumer = output / f"{family}-consumer"
        shutil.copytree(ROOT / family / "consumer", consumer)
        flags = [[], ["--features", "prefix-symbols"]] if family == "transport" else [[]]
        for index, features in enumerate(flags):
            runner.run(["cargo", "test", "--offline", "--config", str(stage / "config.toml"),
                        "--manifest-path", str(consumer / "Cargo.toml")] + features,
                       f"{family}-consumer-{index}")
    runner.run(["python3", str(ROOT / "stage.py"), "--verify-stage", str(stage)], "source-after")
    before = json.loads((output / next(r["log"] for r in runner.records if r["command"][-1:] == [str(stage)] and "source-before" in r["log"])).read_text())
    after = json.loads((output / runner.records[-1]["log"]).read_text())
    if before != after:
        raise ValueError("Staged source changed during verification")
    candidates(roots)
    commit_matches(expected)
    if digest(Path(__file__)) != tool_sha:
        raise ValueError("Release helper changed during verification")
    plan = {"schema": 1, "verified": True, "sourceCommit": expected,
            "toolSha256": tool_sha, "packages": rows,
            "commandsSha256": digest(output / "commands.json"), "stageGuard": after,
            "scope": "Prepublication actual-package verification; not a real registry consumer."}
    write_json(output / "verified-plan.json", plan)


def publish_package(package, runner, registry, output, deadline):
    name, version = package["name"], package["version"]
    source, archive = Path(package["source"]), Path(package["archive"])
    if (source_files(source) != package["sourceFiles"] or digest(archive) != package["archiveSha256"] or
            any(bool((source / p).stat().st_mode & 0o111) != mode
                for p, mode in package["sourceModes"].items())):
        raise ValueError("Verified package changed before publication")
    downloaded = output / f"{name}-{version}.crate"
    exists = registry.version(name, version, archive, downloaded, deadline)
    disposition = "already-published-exact-payload" if exists else "published"
    attempt = 0
    while not exists:
        if time.monotonic() >= deadline:
            raise TimeoutError(f"Publish retry deadline exhausted: {name}@{version}")
        code, log = runner.run(["cargo", "publish", "--registry", "crates-io", "--locked", "--allow-dirty",
                                "--manifest-path", str(source / "Cargo.toml")], f"publish-{name}-{attempt}", check=False)
        # A failed response can follow a successful upload. Never blindly repeat it.
        exists = registry.version(name, version, archive, downloaded, deadline)
        if exists or code == 0:
            break
        text = log.read_text(errors="replace")
        if code and not re.search(r"(?:HTTP|status(?: code)?)[^\n]{0,20}(?:429|50[0234])", text, re.I):
            raise RuntimeError(f"Publish failed; see {log.name}")
        header = re.search(r"Retry-After:\s*([^\r\n]+)", text, re.I)
        pause(deadline, attempt, header.group(1) if header else None)
        attempt += 1
    while not exists:
        pause(deadline, 3)
        exists = registry.version(name, version, archive, downloaded, deadline)
    return {"name": name, "version": version, "disposition": disposition,
            "registryArchiveSha256": digest(downloaded), "exactPayloadVerified": True}


def require_deno_runtime(graph):
    """One registry turbo-dom-v8 150.4.x and no original/retired runtime crate."""
    names = {p["name"] for p in graph["packages"]}
    if RETIRED_RUNTIMES & names:
        raise ValueError("Real registry consumer selected an original runtime fork")
    native = [p for p in graph["packages"] if p["name"] == NATIVE_V8[0]]
    if (len(native) != 1 or not native[0]["version"].startswith(NATIVE_V8[1]) or
            native[0].get("source") != "registry+https://github.com/rust-lang/crates.io-index"):
        raise ValueError("Real registry consumer must link exactly one registry turbo-dom-v8 150.4")


def publish(plan_path, expected, expected_plan_sha, output):
    commit_matches(expected)
    if digest(plan_path) != expected_plan_sha:
        raise ValueError("Verification plan hash differs")
    plan = json.loads(plan_path.read_text())
    if (plan.get("verified") is not True or plan.get("sourceCommit") != expected or
            plan.get("toolSha256") != digest(Path(__file__))):
        raise ValueError("Package verification provenance differs")
    if [(p["name"], p["version"]) for p in plan["packages"]] != [(n, v) for n, v, _ in ORDER]:
        raise ValueError("Publish closure/order differs")
    if not os.environ.get("CARGO_REGISTRY_TOKEN"):
        raise ValueError("CARGO_REGISTRY_TOKEN is required for explicit publishing")
    runner = Runner(output, target=plan_path.parent / "target", publish_token=True)
    registry = Registry()
    receipt = {"sourceCommit": expected, "verifiedPlanSha256": expected_plan_sha,
               "packages": [], "passed": False}
    try:
        for package in plan["packages"]:
            receipt["packages"].append(publish_package(package, runner, registry, output, time.monotonic() + 900))
            write_json(output / "publication.json", receipt)
        # A new Cargo home plus no ancestor config prevents directory replacement
        # from masquerading as the real registry in Cargo metadata's source field.
        registry_home = output / "registry-cargo-home"
        registry_home.mkdir()
        runner.environment["CARGO_HOME"] = str(registry_home)
        for ancestor in [output, *output.parents]:
            if any((ancestor / ".cargo" / name).exists() for name in ("config", "config.toml")):
                raise ValueError("Ancestor Cargo configuration could replace the registry")
        # No replacement source: this is the required real-registry downstream gate.
        for family in ("deno", "transport"):
            consumer = output / f"registry-{family}-consumer"
            shutil.copytree(ROOT / family / "consumer", consumer)
            flags = [[], ["--features", "prefix-symbols"]] if family == "transport" else [[]]
            for index, features in enumerate(flags):
                runner.run(["cargo", "test", "--manifest-path", str(consumer / "Cargo.toml")] + features,
                           f"registry-{family}-{index}", cwd=consumer)
            metadata_path = output / f"registry-{family}-metadata.json"
            runner.run(["cargo", "metadata", "--locked", "--format-version=1",
                        "--manifest-path", str(consumer / "Cargo.toml")],
                       f"registry-{family}-metadata", cwd=consumer, stdout_path=metadata_path)
            graph = json.loads(metadata_path.read_text())
            found = {p["name"]: p for p in graph["packages"]}
            for name, version, kind in ORDER:
                if kind == family and (name not in found or found[name]["version"] != version or
                                       found[name].get("source") != "registry+https://github.com/rust-lang/crates.io-index"):
                    raise ValueError("Real registry consumer dependency identity differs")
            if family == "deno":
                require_deno_runtime(graph)
        commit_matches(expected)
        if digest(Path(__file__)) != plan["toolSha256"]:
            raise ValueError("Release helper changed during publication")
        receipt["realRegistryConsumersPassed"] = True
        receipt["passed"] = True
    finally:
        write_json(output / "publication.json", receipt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["verify", "publish"])
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--deno", type=Path)
    parser.add_argument("--transport", type=Path)
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--expected-plan-sha")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if args.mode == "verify":
        if not args.deno or not args.transport:
            parser.error("verify requires --deno and --transport")
        verify({"deno": args.deno.resolve(), "transport": args.transport.resolve()}, args.output.resolve(), args.expected_commit)
    else:
        if not args.plan or not args.expected_plan_sha:
            parser.error("publish requires a hash-pinned verification plan")
        publish(args.plan.resolve(), args.expected_commit, args.expected_plan_sha, args.output.resolve())


if __name__ == "__main__":
    main()
