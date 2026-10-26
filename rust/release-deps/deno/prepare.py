#!/usr/bin/env python3
"""Prepare the pinned, renamed Deno dependency closure without Cargo patches."""

import argparse
import hashlib
import json
import re
import shutil
import tarfile
import tempfile
import tomllib
import urllib.request
from collections import namedtuple
from pathlib import Path


ROOT = Path(__file__).resolve().parent
SURF = ROOT.parents[2]
# `version` is the upstream archive identity; `publish` is the distribution's own
# version. The native V8 binding is not prepared here: turbo-dom publishes it as
# `turbo-dom-v8` and the facade requires it from the registry.
Package = namedtuple("Package", "upstream version name publish vendor")
PACKAGES = [
    Package("deno_v8", "0.4.0", "turbo-surf-deno-v8", "0.4.1", None),
    Package("serde_v8", "0.321.0", "turbo-surf-serde-v8", "0.321.1", None),
    Package("deno_core", "0.412.0", "turbo-surf-deno-core", "0.412.1", None),
    Package("deno_bench_util", "0.253.0", "turbo-surf-deno-bench-util", "0.253.1", None),
    Package("deno_crypto", "0.273.0", "turbo-surf-deno-crypto", "0.273.1", "deno_crypto"),
    Package("deno_web", "0.290.0", "turbo-surf-deno-web", "0.290.1", None),
    Package("deno_webidl", "0.259.0", "turbo-surf-deno-webidl", "0.259.1", None),
]
NAMES = {p.upstream: (p.publish, p.name) for p in PACKAGES}
# Upstream `v8` (rusty_v8) edges become this registry requirement, never a path.
NATIVE = ("v8", "turbo-dom-v8", "~150.4")
V8_DEFAULT_LEAVES = {"deno_bench_util", "deno_crypto", "deno_webidl", "deno_web"}
EMBEDDED_LEAVES = {"deno_crypto", "deno_web", "deno_webidl"}
# The facade's optional second backend is unused by every consumer; 0.4.1 drops
# it so no lock file lists a second native engine. Features that only forwarded
# to it (`quickjs` in serde_v8/deno_core) would otherwise fail resolution.
FACADE_PASSTHROUGHS = ["simdutf", "use_custom_libcxx", "v8_enable_pointer_compression",
                       "v8_enable_sandbox", "v8_enable_v8_checks"]
QUICKJS_FORWARDS = {"serde_v8": ["v8/quickjs"], "deno_core": ["v8/quickjs", "serde_v8/quickjs"]}
SOURCE_KINDS = {"js": "Js", "esm": "Esm", "lazy_loaded_js": "LazyJs", "lazy_loaded_esm": "LazyEsm"}
EMBEDDED_MODULE = "embedded_sources.rs"
LIB_SUFFIX = ("\n// turbo-surf distribution: compile-time copies of this extension's JavaScript.\n"
              "pub mod embedded_sources;\n")


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


def remove_field(body, key):
    return re.sub(rf"(?m)^{re.escape(key)} = .*\n", "", body)


def backend_features(parsed, old):
    if old not in V8_DEFAULT_LEAVES:
        return ""
    if "features" in parsed:
        raise ValueError("Upstream leaf features changed; review backend forwarding")
    dependency = parsed["dependencies"]["deno_core"]
    if dependency.get("default-features") is not False:
        raise ValueError("Upstream leaf backend admission changed")
    # Upstream extensions rely on an enclosing runtime to select the backend.
    # Named standalone distributions need a default, while callers must still
    # be able to disable it and choose a backend on their own core dependency.
    return '\n[features]\ndefault = ["v8"]\nv8 = ["deno_core/v8"]\n'


def facade_features(features):
    """Drop the second backend from the facade's reviewed feature surface."""
    expected = {"default": ["v8"], "v8": ["dep:rusty_v8"],
                "quickjs": ["dep:v8x_backend", "v8x_backend/quickjs"],
                **{name: [f"rusty_v8?/{name}", f"v8x_backend?/{name}"] for name in FACADE_PASSTHROUGHS}}
    if features != expected:
        raise ValueError("Upstream facade features changed; review backend forwarding")
    return {"default": ["v8"], "v8": ["dep:rusty_v8"],
            **{name: [f"rusty_v8?/{name}"] for name in FACADE_PASSTHROUGHS}}


def rewrite_features(features, old):
    if old == "deno_v8":
        return facade_features(features)
    if old in QUICKJS_FORWARDS:
        if features.get("quickjs") != QUICKJS_FORWARDS[old]:
            raise ValueError("Upstream quickjs forwarding changed; review backend forwarding")
        return {key: value for key, value in features.items() if key != "quickjs"}
    return features


def feature_table(features):
    key = lambda name: name if re.fullmatch(r"[A-Za-z0-9_-]+", name) else json.dumps(name)
    return "".join(f"{key(name)} = {json.dumps(value)}\n" for name, value in features.items()) + "\n"


def manifest(source, package):
    old, version, new, publish = package.upstream, package.version, package.name, package.publish
    parsed = tomllib.loads(source)
    if (parsed["package"]["name"], parsed["package"]["version"]) != (old, version):
        raise ValueError("Unexpected upstream manifest identity")
    sections = re.split(r"(?m)^(\[[^\n]+\]\n)", source)
    for index in range(1, len(sections), 2):
        header, body = sections[index:index + 2]
        if header == "[package]\n":
            body = replace_field(body, "name", new)
            body = replace_field(body, "version", publish)
            body = replace_field(body, "repository", "https://github.com/miaskiewicz/turbo-surf")
            description = parsed["package"].get("description", old)
            body = replace_field(body, "description", f"turbo-surf reviewed distribution of {old}: {description}")
        elif header == "[features]\n":
            body = feature_table(rewrite_features(tomllib.loads(body), old))
        match = re.search(r"(?:^|\.)(?:dev-|build-)?dependencies\.([^\]]+)\]", header[1:])
        if match:
            alias = match[1].strip('"')
            fields = tomllib.loads(body)
            upstream = fields.get("package", alias)
            if upstream == "v8x":
                if old != "deno_v8" or alias != "v8x_backend" or fields.get("optional") is not True:
                    raise ValueError("Unexpected second native backend edge")
                sections[index] = body = ""
            elif upstream == NATIVE[0]:
                if old != "deno_v8" or alias != "rusty_v8" or fields.get("default-features") is not False:
                    raise ValueError("Unexpected native V8 edge")
                body = replace_field(body, "package", NATIVE[1])
                body = replace_field(body, "version", NATIVE[2])
                body = remove_field(body, "path")
            elif upstream in NAMES:
                required, renamed = NAMES[upstream]
                body = replace_field(body, "package", renamed)
                body = replace_field(body, "version", "=" + required)
                body = replace_field(body, "path", "../" + renamed)
        sections[index + 1] = body
    result = "".join(sections) + backend_features(parsed, old)
    return result + "\n[package.metadata.turbo-surf-upstream]\n" + f"name = {json.dumps(old)}\nversion = {json.dumps(version)}\nrepository = {json.dumps(parsed['package']['repository'])}\n"


def declared_sources(declaration):
    """The extension!() source lists, in declaration order (as deno_core embeds them)."""
    result = []
    for kind, listing in re.findall(r"(lazy_loaded_js|lazy_loaded_esm|esm|js)\s*=\s*\[([^]]*)\]", declaration, re.S):
        result.extend((kind, name) for name in re.findall(r'"([^"\n]+\.js)"', listing))
    return result


def embedded_module(old, pin, target):
    """Generate the public compile-time source table from the reviewed declaration."""
    lib = target / "lib.rs"
    if digest(lib) != pin["declarationSha256"]:
        raise ValueError(f"Official extension declaration changed: {old}")
    if digest(target / pin["licenseFile"]) != pin["licenseSha256"]:
        raise ValueError(f"Upstream license changed: {old}")
    sources = pin["sources"]
    if declared_sources(lib.read_text()) != [(row["kind"], row["file"]) for row in sources]:
        raise ValueError(f"Official extension source inventory changed: {old}")
    if (target / EMBEDDED_MODULE).exists():
        raise ValueError(f"Upstream already has {EMBEDDED_MODULE}: {old}")
    rows = []
    for row in sources:
        data = (target / row["file"]).read_bytes()
        if not data.isascii() or len(data) != row["bytes"] or hashlib.sha256(data).hexdigest() != row["sha256"]:
            raise ValueError(f"Official extension source bytes changed: {old}/{row['file']}")
        rows.append(f'    EmbeddedSource {{ extension: EXTENSION, kind: SourceKind::{SOURCE_KINDS[row["kind"]]}, '
                    f'specifier: "ext:{old}/{row["file"]}", code: deno_core::ascii_str_include!("{row["file"]}"), '
                    f'sha256: "{row["sha256"]}", bytes: {row["bytes"]} }},')
    module = f'''// Generated by turbo-surf rust/release-deps/deno/prepare.py; do not edit.
// Exact published Deno JS, Copyright 2018-2026 the Deno authors, MIT.
//! Compile-time copies of the JavaScript declared by the `{old}` extension.
//!
//! Upstream extension declarations may resolve their sources from the build
//! machine's checkout at runtime. Embedders that must not depend on those paths
//! can substitute these exact published bytes, in declaration order, without
//! vendoring the files themselves.

/// Which `deno_core::Extension` source list a file belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKind {{
    /// `Extension::js_files`
    Js,
    /// `Extension::esm_files`
    Esm,
    /// `Extension::lazy_loaded_js_files`
    LazyJs,
    /// `Extension::lazy_loaded_esm_files`
    LazyEsm,
}}

/// One extension source, compiled into the binary and checked to be ASCII.
#[derive(Clone, Copy)]
pub struct EmbeddedSource {{
    /// The `deno_core::Extension::name` that declares this source.
    pub extension: &'static str,
    /// The source list this file belongs to.
    pub kind: SourceKind,
    /// The `ext:` module specifier the extension registers.
    pub specifier: &'static str,
    /// The exact published source text.
    pub code: deno_core::FastStaticString,
    /// SHA-256 of `code`, pinned at preparation time.
    pub sha256: &'static str,
    /// Length of `code` in bytes.
    pub bytes: usize,
}}

impl EmbeddedSource {{
    /// The equivalent `deno_core` source entry, backed by the embedded bytes.
    pub const fn file_source(&self) -> deno_core::ExtensionFileSource {{
        deno_core::ExtensionFileSource::new(self.specifier, self.code)
    }}
}}

/// The extension these sources belong to.
pub const EXTENSION: &str = "{old}";

/// The upstream license covering these sources.
pub const LICENSE: &str = include_str!("{pin["licenseFile"]}");

/// SHA-256 of [`LICENSE`].
pub const LICENSE_SHA256: &str = "{pin["licenseSha256"]}";

/// Every source the extension declares, in declaration order.
#[rustfmt::skip]
pub const SOURCES: &[EmbeddedSource] = &[
{chr(10).join(rows)}
];
'''
    (target / EMBEDDED_MODULE).write_text(module)
    with lib.open("a") as stream:
        stream.write(LIB_SUFFIX)


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
    if set(pins["packages"]) != {p.upstream for p in PACKAGES}:
        raise ValueError("Source pin package closure changed")
    receipt = {"packages": [], "scope": "Prepared sources only; not packaged, compiled, published or clean-registry-consumer verified."}
    for package in PACKAGES:
        old, version, new, publish, vendor = package
        pin = pins["packages"][old]
        if (pin["version"], pin["candidateName"], pin["publishVersion"]) != (version, new, publish):
            raise ValueError("Source pin package identity changed")
        if ("embeddedSources" in pin) != (old in EMBEDDED_LEAVES):
            raise ValueError("Embedded source pin closure changed")
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
        (target / "Cargo.toml").write_text(manifest((target / "Cargo.toml").read_text(), package))
        if not (target / "LICENSE").exists() and not (target / "LICENSE.md").exists():
            shutil.copyfile(SURF / "rust/vendor/deno_crypto/LICENSE.md", target / "LICENSE.md")
        if old in EMBEDDED_LEAVES:
            embedded_module(old, pin["embeddedSources"], target)
        final = tree(target)
        receipt["packages"].append({"upstreamName": old, "upstreamVersion": version, "name": new, "version": publish,
                                    "archiveSha256": pin["archiveSha256"], "inputFiles": original, "candidateFiles": final,
                                    "candidateBytes": sum(p.stat().st_size for p in target.rglob("*") if p.is_file())})
    members = [f"packages/{p.name}" for p in PACKAGES]
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
    print("Prepared source candidates match pins." if args.check else f"Prepared {len(PACKAGES)} source candidates; no Cargo or publishing invoked.")


if __name__ == "__main__":
    main()
