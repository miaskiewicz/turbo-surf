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
- `deno/`: the coherent facade/serde/core/crypto/web/WebIDL/benchmark closure
  (seven packages). Its source archives, licenses, reviewed hardening and
  embedded-source inventories have explicit hashes. See its README for
  preparation and graph details.
- The native V8 binding is **not** prepared here. `turbo-dom-v8` 150.4.x (the
  reviewed rusty_v8 fork, library `v8`, `links = "rusty_v8"`) is published from the
  turbo-dom repository and is an external prerequisite: the facade requires it from
  the registry (`~150.4`). Publish it first. Neither is the experimental
  Chrome155 SDK.

Each distribution records its upstream identity (`[package.metadata.turbo-surf-upstream]`)
separately from its own version. The 0.x.1 Deno family is upstream + 1 patch;
preparation still verifies the downloaded upstream archive at its exact upstream
version. Preserve library names and dependency aliases even when package names
change.

## Pre-publication candidates

`push-candidates.sh` runs `deno/prepare.py --download` and `--check`, lays the
seven candidates out with `deno/candidate_tree.py` (registry-form manifests with
dependency paths removed, unique package names), commits them with a private
index to the orphan branch `consolidation/deno-candidates` (later runs append a
child commit only when the tree changes) and pushes it. It prints the commit and
the `[patch.crates-io]` table a consumer PR uses, one entry per package at that
one `rev`; the PR must also patch `turbo-dom-v8` to its turbo-dom Git revision.
Nothing is published; the branch is kept until the consumer PR merges.

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
python3 -B -m unittest discover -s rust/release-deps/deno -p 'test_*.py' -v
```

The controls cover package aliases and target edges, preserved native links,
benchmark/core identity, archive checksums, traversal rejection, input ownership
and actual detection of linked-source mutation. They do not replace native tests
or downstream registry verification.
