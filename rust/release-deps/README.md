# Registry-compatible dependency preparation

Surf's binary builds originally used workspace patches. A published library's
consumer does not inherit those patches. These tools prepare explicit named
packages that preserve the reviewed transport, crypto and V8 fixes.

Preparation does not publish anything. A successful `cargo package --no-verify`
only produces an archive; it does not prove compilation, runtime behavior or a
published registry consumer. Keep those verdicts separate.

## Candidates

- `transport/prepare.py`: five packages in dependency order,
  `turbo-surf-btls-sys`, `turbo-surf-btls`, `turbo-surf-tokio-btls`,
  `turbo-surf-wreq`, `turbo-surf-wreq-util`. Exact public Git revisions and the
  BoringSSL submodule revision are pinned in the generator. The selected native
  sources are bundled, so a registry consumer does not need a Git submodule.
- `deno/`: the coherent V8/facade/serde/core/crypto/web/WebIDL/benchmark closure.
  Its source archives, licenses, reviewed hardening and distribution test patch
  have explicit hashes. See its README for preparation and graph details.
- Turbo-test prepares the same `turbo-surf-v8` package family at upstream version
  149.3.0; Surf's stock Deno facade requires 150.4.0. They are separate dependency
  graphs. Neither is the experimental Chrome155 SDK.

The initial fork versions retain upstream numbers because the native build
scripts derive official SDK URLs from them. Preserve library names and dependency
aliases even when package names change. Later package-version changes require
separate upstream-SDK-version handling and validation.

## Staged validation

`stage.py` creates a private Cargo directory source from candidate sources or
actual `.crate` archives. Bootstrap source staging removes paths, Git pins and
workspace patches from its copied manifest. Archive staging preserves Cargo's
normalized manifest and binds checksums to that archive. Actual archive staging
must precede the packaged-consumer build; bootstrap alone is insufficient.

Standard registry packages are represented by private wrappers linking their
existing Cargo-managed source files. Wrapper checksums come from the real cached
registry archive. Link permissions remain unchanged; the staging helper does not
write the linked source files. Run the following before and after a build and
retain both receipts to establish that expected source bytes remain unchanged:

```sh
python3 -B rust/release-deps/stage.py --verify-stage /path/to/private-stage
cargo test --offline --config /path/to/private-stage/config.toml \
  --manifest-path /path/to/isolated-consumer/Cargo.toml
python3 -B rust/release-deps/stage.py --verify-stage /path/to/private-stage
```

This proves only the staged distributions. After publishing, build a fresh
consumer using the real default registry, without replacement sources or workspace
patches. Verify normalized dependency names, versions, checksums and runtime
contracts before treating registry consumption as complete.

`fetch-registry.py NAME VERSION --stage STAGE --downloads PRIVATE_DIR` can add a
missing standard dependency after checking the public sparse-index checksum.
It writes only the specified private output. Normal `cargo fetch` can instead
populate Cargo's managed cache; `stage.populate_cache` adds new wrapper entries
without changing that cache or replacing existing staged candidates.

## Offline controls

```sh
python3 -B -m unittest discover -s rust/release-deps -p 'test_*.py' -v
```

The controls cover package aliases and target edges, preserved native links,
benchmark/core identity, archive checksums, traversal rejection, input ownership
and actual detection of linked-source mutation. They do not replace native tests
or downstream registry verification.
