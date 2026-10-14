# Native weak-handle teardown

The default engine consumes a reviewed patch of Rusty V8 **150.4.0**. The published
archive and upstream revision, retained MIT license and source hashes are pinned
in [RUSTY_V8_SOURCE_PINS.json](RUSTY_V8_SOURCE_PINS.json). The complete patch is
[rusty-v8-150.patch](rusty-v8-150.patch).

Upstream invalidates its IsolateHandle before dropping native context/isolate
slots. A Weak destructor then skips V8's global-handle reset, while its callback
data can already be freed. Keep the handle live through slot and guaranteed
finalizer cleanup; invalidate it immediately before V8 disposal. The offline
`weak_teardown_contracts` regression runs 16 independent isolates and checks both
slot destructors. The original dependency fails that check.

This changes Rust ownership cleanup, preserving the dependency version and
official prebuilt native archive. It does not update the embedded V8 engine to
Chrome 155. The experimental 155 SDK remains a separate build and validation task.

The committed vendor includes every Rust source, generated platform binding,
root manifest/metadata, example, test, benchmark and tooling file from the archive,
plus its upstream license. Native C++ source trees are omitted from the default
prebuilt distribution. A source build fails explicitly if those trees are absent.
Reproduce either distribution without changing the Cargo package cache:

```sh
python3 rust/vendor/update-rusty-v8.py --check
# Restore the complete published C++ source payload before a source build:
python3 rust/vendor/update-rusty-v8.py --write --full-source
python3 rust/vendor/update-rusty-v8.py --check --full-source
```

The helper accepts `--archive PATH` and verifies the published archive hash before
extracting or applying anything. Full-source payloads are local build inputs;
commit only the reviewed default subset.

The workspace Cargo patch is development integration. Registry consumers do not
inherit workspace patches: publishable Surf libraries still require a properly
distributed dependency or an upstream release containing the teardown fix, plus
clean consumer packaging checks.
