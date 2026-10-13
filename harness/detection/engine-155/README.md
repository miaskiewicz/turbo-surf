# Native Chrome 155 engine profile

This profile compiles V8 **15.5.35.20** from the exact engine revision used by the installed **Chrome 155.0.8059.40** oracle. The version comes from the compiled engine; the harness checks its native `V8::get_version()` before accepting an archive or running Surf. It shares the SDK preparation and binding patches with the preserved Chrome 154 profile.

```sh
python3 harness/detection/engine-155/prepare.py .native-builds/sdk155
python3 harness/detection/engine-155/build.py .native-builds/sdk155 --jobs 4
python3 harness/detection/engine-155/build.py .native-builds/sdk155 --surf test --manifest-path rust/Cargo.toml -p turbo-surf-render
```

The detection executable needs all four native features. Run its renderer/MCP checks and build with the same SDK override:

```sh
python3 harness/detection/engine-155/build.py .native-builds/sdk155 --surf test --manifest-path rust/Cargo.toml -p turbo-surf-render -p turbo-surf-mcp --no-fail-fast --features turbo-surf-mcp/trust-anchors,turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext
python3 harness/detection/engine-155/build.py .native-builds/sdk155 --surf clippy --manifest-path rust/Cargo.toml -p turbo-surf-render -p turbo-surf-mcp --all-targets --features turbo-surf-mcp/trust-anchors,turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext -- -D warnings
python3 harness/detection/engine-155/build.py .native-builds/sdk155 --surf build --manifest-path rust/Cargo.toml -p turbo-surf-mcp --example detection_trace --features turbo-surf-mcp/trust-anchors,turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext
.native-builds/sdk155/target/debug/examples/detection_trace --build-info
```

The default MCP feature supplies `impersonate`; the explicit flags enable trust anchors, Metal and CoreText. Check the executable's build information before a capture. The source-built engine, native WindowProxy/security smoke and all-feature executable have been validated on macOS arm64; other host builds remain unverified.

The native addon uses the same verified engine override. On macOS arm64, select its release build explicitly for the shim checks:

```sh
python3 harness/detection/engine-155/build.py .native-builds/sdk155 --surf build --manifest-path rust/Cargo.toml --release -p turbo-surf-napi --features turbo-surf-napi/gpu-metal,turbo-surf-napi/coretext,turbo-surf-core/trust-anchors
cp .native-builds/sdk155/target/release/libturbo_surf_napi.dylib .native-builds/sdk155/frozen/turbo-surf-native155-release.node
codesign --force --sign - .native-builds/sdk155/frozen/turbo-surf-native155-release.node
TURBO_SURF_NAPI_PATH="$PWD/.native-builds/sdk155/frozen/turbo-surf-native155-release.node" npm run test:shim
TURBO_SURF_NAPI_PATH="$PWD/.native-builds/sdk155/frozen/turbo-surf-native155-release.node" npm run test:e2e
```

An explicit addon path prevents the loader from choosing another engine's release or debug library. Building the addon does not publish it.

Use a separate persistent, ignored destination. The helper never overwrites the SDK 154 source, native archive, bindings or frozen executables. A new engine requires recompiling C++ and generating matching bindings; a cached 154 archive cannot substitute for the 155 engine. Every accepted build runs the smoke program and the native WindowProxy/security regressions. A Surf command receives `TURBO_SURF_ENGINE=155`, the selected Cargo source patches and the verified native artifact paths.

The checked profile is [`../engine-154/profiles/155.json`](../engine-154/profiles/155.json). [Chrome's exact release DEPS](https://chromium.googlesource.com/chromium/src/+/refs/tags/155.0.8059.40/DEPS) pins V8 revision `56832c166a78bd1eb61068ac69876d6880eaabdf`; [the official V8 version header](https://chromium.googlesource.com/v8/v8/+/refs/tags/15.5.35.20/include/v8-version.h) declares `15.5.35.20`. The imported source dependency fragments follow that engine's DEPS: build `8ca8a372cab9ff143072204053d30fbc2c17bd7a`, ICU `6ebb40c594776cc2c21ea14df85a2a89a328b364`, and Rust dependencies `4dec6f65bcb34c4c289736b0e6fd571b35c6c665`.

V8 155 adds a metagen build step that requires the LLVM libclang library and Python bindings. Preparation installs the official platform package pinned in that engine's DEPS and verifies its SHA-256. These package URLs and hashes are in the profile manifest; `prepare.py --adopt` repairs this dependency in an already prepared destination. Platforms without an official package entry fail explicitly. The build helper also exposes the SDK's own downloaded GN/Ninja tools and a local `python` alias for its active Python interpreter on the subprocess PATH, so metagen reconstructs compile flags using the same GN executable that generated the native build.

The wrapper SDK remains the official rusty-v8 `152.2.0`, `deno_v8 0.4.0`, and `deno_core 0.412.0` with the committed native binding and context/module/inspector patches. Published crates still use their declared SDK; this experimental source profile does not publish a new SDK or establish full browser API, resource-policy or Google outcome parity. Transport is configured and measured separately: upstream `wreq-util` currently ends at Chrome 154, so this helper makes no claim that it provides an upstream Chrome 155 TLS template.

Preparation records the profile and reverse-verifies every applied patch; building records the actual version, native archive/binding hashes, source revision and test result in the destination's `validated.json`. Raw diagnostics stay ignored. Preserve the 154 destination for regressions and comparisons.

The 2026-10-13 checkpoint passed 366 renderer/MCP tests with all four features, including 12 WebCrypto, seven host-input and three asynchronous expression contracts. The inherited deno_core extension doctest remains ignored; no application tests were ignored. Strict Clippy passed for both crates and all targets. The frozen detection executable reports native `15.5.35.20-rusty`, stable WindowProxy support and Chrome `155.0.8059.40`; its ignored `frozen/final-build.json` retains executable/source hashes, SDK provenance and gate logs. This checkpoint validates the native engine integration; fresh paired captures establish the page outcome separately.
