# Actual packaged Surf consumer

This standalone fixture uses the default features of all five published Surf Rust
libraries at the exact workspace version, plus `turbo-dom-raster` (the screenshot /
canvas tier, published from turbo-dom). Its DOM type comes from registry turbo-dom;
`scripts/check-versions.py` requires both turbo-dom pins to equal the workspace
lockfile.
There are no path dependencies, root patches, disabled defaults or backend feature
overrides. One offline test covers core/page type identity, view/DOM identity,
turbo-dom-v8 150 execution, real DOM mutation and the MCP-to-raster canvas hook. It
does not create an MCP Session, load `.env`, configure solvers or fetch a page.

Run every command from a fresh private directory outside the source workspace.
Retain exact commands, toolchain, sources, archive hashes, loaded artifact paths,
test results and failures. Keep prepublication staging and registry results distinct.

## Before registry publication

First require successful publication and real-registry verification of all
thirteen named Deno/transport dependencies and the DOM/layout prerequisites.
Integrate their registry requirements and the updated lockfile in the selected
release checkout. No workspace patches or source replacements may supply them.
The Surf crates themselves do not have to be published yet.

Package all five Surf crates in **one** normal Cargo invocation. Cargo stages the
unpublished sibling path dependencies automatically and verifies their normalized
distributions in dependency order:

`turbo-surf-core → turbo-surf-view → turbo-surf-page → turbo-surf-render → turbo-surf-mcp`

Use a fresh Cargo home without ancestor Cargo config that could replace the
registry. From the selected release checkout, fetch the real dependencies and
keep compilation verification enabled:

```sh
CARGO_HOME="$RELEASE_CARGO_HOME" cargo fetch --locked --manifest-path rust/Cargo.toml
CARGO_HOME="$RELEASE_CARGO_HOME" CARGO_TARGET_DIR="$PACKAGE_TARGET" \
  cargo package --locked --manifest-path rust/Cargo.toml \
  -p turbo-surf-core -p turbo-surf-view -p turbo-surf-page \
  -p turbo-surf-render -p turbo-surf-mcp
ARCHIVES="$PACKAGE_TARGET/package"
```

Do not add a directory-source configuration to that multi-package command.
Cargo1.98's automatic sibling staging works with its normal registry, but a tiny
actual check with an empty directory replacement fails to resolve the unpublished
sibling even with `--registry crates-io`. The same tiny two-crate command without
replacement passes normal compilation verification. These are procedure controls,
not evidence that the five real Surf packages have already passed.

After successful verification, create a private directory source from the
**actual** dependency archives and these five actual Surf archives using
`../stage.py`:

```sh
python3 -B rust/release-deps/stage.py --output "$RELEASE_STAGE" \
  --candidate-archive "$ARCHIVES/turbo-surf-core-0.6.0.crate" \
  --candidate-archive "$ARCHIVES/turbo-surf-view-0.6.0.crate" \
  --candidate-archive "$ARCHIVES/turbo-surf-page-0.6.0.crate" \
  --candidate-archive "$ARCHIVES/turbo-surf-render-0.6.0.crate" \
  --candidate-archive "$ARCHIVES/turbo-surf-mcp-0.6.0.crate"
```

Append one `--candidate-archive` for **each** of the thirteen actual dependency
distributions, retaining their verified archive identities even after publication. Standard
dependencies in this stage are validated cache wrappers, not workspace patches.
The published DOM/layout crate archives must also be available, with their real
registry checksums. Preserve actual archive hashes and distinguish any generated
lock/VCS changes from source payloads and normalized manifest semantics. There is
no custom sequential Surf bootstrap or `--no-verify` substitution for the normal
multi-package gate above.

Copy this complete fixture to `$CONSUMER`, then run:

```sh
python3 -B rust/release-deps/stage.py --verify-stage "$RELEASE_STAGE"
cargo test --offline --config "$RELEASE_STAGE/config.toml" \
  --manifest-path "$CONSUMER/Cargo.toml"
cargo test --locked --offline --config "$RELEASE_STAGE/config.toml" \
  --manifest-path "$CONSUMER/Cargo.toml"
cargo metadata --locked --offline --format-version=1 \
  --config "$RELEASE_STAGE/config.toml" --manifest-path "$CONSUMER/Cargo.toml" \
  > "$CONSUMER/metadata.json"
python3 -B rust/release-deps/stage.py --verify-stage "$RELEASE_STAGE"
```

Check the resolved graph contains exactly one package at the release version for each of the five
Surf libraries, the expected named Deno/transport versions, and no original
unpatched runtime or transport packages. The two stage checksum maps must match.
Also retain successful normal `cargo package` verification of each actual Surf
distribution; this consumer does not replace those default standalone checks.

## After registry publication

Copy a fresh fixture, use a new empty `CARGO_HOME`, and run `cargo test`, then
`cargo test --locked` and `cargo metadata --locked --format-version=1` with no
`--config`, root patches or directory replacement. Reject ancestor Cargo config
that could replace the registry. Confirm the five release archives/checksums and named
dependency versions are the real published ones. Cargo metadata's registry source
string alone does not prove that a directory replacement was absent.

## Python wheel and sdist

Use the default PyO3 feature profile; do not add GPU/CoreText or an SDK overlay.
Select an actual Python3.12 interpreter, matching CI. Do not use the host's
Python3.14 with the current PyO3 version or change user-site packages. Build and
hash the actual artifacts from a private environment, for example:

```sh
"$PYTHON312" -m venv "$BUILD_ENV"
"$BUILD_ENV/bin/python" -m pip install 'maturin>=1.9.3,<2.0'
PYO3_PYTHON="$BUILD_ENV/bin/python" "$BUILD_ENV/bin/maturin" build --release \
  --interpreter "$BUILD_ENV/bin/python" \
  --manifest-path rust/crates/turbo-surf-py/Cargo.toml --out "$WHEELS"
"$BUILD_ENV/bin/maturin" sdist --manifest-path rust/crates/turbo-surf-py/Cargo.toml --out "$SDISTS"
```

Unpack and inspect the sdist before compiling it. Its pyproject and Cargo version
must be0.6.0; Python source, all workspace/path dependencies needed for a source
build, and licenses/notices must be present. It must not require missing external
checkout paths or workspace patches. Build a wheel **from that tarball**, outside
the checkout, with a fresh Cargo home and no source replacements:

```sh
"$PYTHON312" -m venv "$SDIST_ENV"
"$SDIST_ENV/bin/python" -m pip install --upgrade pip
CARGO_HOME="$SDIST_CARGO_HOME" PYO3_PYTHON="$SDIST_ENV/bin/python" \
  "$SDIST_ENV/bin/python" -m pip wheel --no-deps "$SDIST_TARBALL" --wheel-dir "$SDIST_WHEELS"
```

On Windows use `Scripts/python.exe`. Install each matching-platform wheel into
a different clean venv using its exact local file path and `--no-deps`; repeat
for the wheel rebuilt from the sdist. Run from outside the source tree:

```sh
"$PYTHON312" -m venv "$SMOKE_ENV"
"$SMOKE_ENV/bin/python" -m pip install --no-deps "$EXACT_WHEEL"
"$SMOKE_ENV/bin/python" -I - <<'PY'
import importlib.metadata
import json
from pathlib import Path
import turbo_surf as ts
import turbo_surf._turbo_surf as native

assert importlib.metadata.version('turbo-surf') == ts.version() == '0.6.0'
assert Path(native.__file__).is_file()
html = '<title>Packaged wheel</title><body><div id="root">Before</div><a href="/next">Next</a></body>'
assert ts.title(html) == 'Packaged wheel'
assert 'Before' in ts.text(html)
assert ts.links(html, base_url='https://example.test/') == ['https://example.test/next']
assert len(json.loads(ts.query(html, '#root'))) == 1
assert ts.evaluate(html, "document.getElementById('root').textContent = 'V8'; document.getElementById('root').textContent") == 'V8'
hydrated = ts.render(html, "document.getElementById('root').textContent = 'hydrated';", base_url='https://example.test/')
assert 'hydrated' in hydrated
assert Path(ts.__file__).with_name('THIRD_PARTY_NOTICES.md').is_file()
print('wheel import, metadata, views, DOM/V8 and hydration passed')
PY
```

These calls contain no external resources and require no network after
installation. They are artifact-consumer checks, not a live crawler/Google test.
Also copy `rust/crates/turbo-surf-py/tests/test_smoke.py` into a private test
directory outside the checkout. Install pytest only in the same smoke venv and
run all ten existing offline cases for **each** installed wheel:

```sh
"$SMOKE_ENV/bin/python" -m pip install 'pytest>=7'
"$SMOKE_ENV/bin/python" -I -m pytest "$SMOKE_TESTS/test_smoke.py"
```

The version case binds the extension's actual version to installed distribution
metadata; the explicit0.6.0 assertion above additionally checks release identity.
Keep the direct-wheel and source-tarball-rebuilt-wheel outcomes separate.
