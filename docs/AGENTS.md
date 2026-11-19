# turbo-surf: agent + human runbook

Short, one-command wrappers for the workflows an agent (or a human) hits most on
this repo. Every script lives in `scripts/agent/`, is pure bash with
`set -euo pipefail` and `--help`, and is safe to call from any working
directory. The scripts do not hide behaviour — each one shells out to the real
tool and documents which flags it adds.

## Scripts

| Script | What it does |
| --- | --- |
| `scripts/agent/oracle.sh ARGS…` | Launches the frozen Chrome 156 `chrome-headless-shell` oracle with the pinned flags (`--use-mock-keychain`, `--password-store=basic`, localhost-only resolver). Every other recorder / probe calls this. |
| `scripts/agent/record-chrome-control.sh PROBE OUT` | Biome-formats `PROBE` first, then records its control via `harness/detection/web-apis-chrome-control.mjs` into `OUT`. |
| `scripts/agent/build-addon.sh` | `npm ci` + `npm run build:addon` with `RUSTY_V8_ARCHIVE` and a per-run `CARGO_TARGET_DIR` under the scratchpad. |
| `scripts/agent/test-crate.sh CRATE [FILTER]` | `cd rust && cargo test -p CRATE [-- FILTER]` with the right features. |
| `scripts/agent/identity-battery.sh` | Runs `harness/detection/reversion-chrome.mjs --check` against the committed `chrome-identity-<major>.json` fixture. |
| `scripts/agent/parity-check.sh` | Runs the offline api-shape parity test over `harness/detection/API-SHAPE.json`. |
| `scripts/agent/pin-turbo-dom.sh REV` | Pins every `turbo-dom*` git dep (parser/layout/raster/browser-env) in `rust/Cargo.toml` to `REV`. `turbo-dom-v8` has its own candidate rev and is not touched. |
| `scripts/agent/drop-turbo-dom-pin.sh [REQ]` | Replaces the git pins with `version = "<REQ>"` (default `0.8`) for the published release. |
| `scripts/agent/coverage.sh [CRATE]` | Runs one sharded tarpaulin pass per `rust/coverage-floors.json`. |
| `scripts/agent/release-prep.sh X.Y.Z` | Bumps every version string so `python3 scripts/check-versions.py` passes. Does not tag; see [PUBLISHING.md](../PUBLISHING.md). |

## Oracle: path and launch flags

Always call Chrome through `scripts/agent/oracle.sh`; never invoke
`chrome-headless-shell` directly. That keeps the flag set in one place.

Default cache layout (persistent, outside the repo, outside `/tmp`):

```
~/.cache/turbo-consolidation/
├── chrome156/
│   ├── chrome-headless-shell-mac-arm64/chrome-headless-shell   # TURBO_CHROME_HEADLESS
│   └── chrome-mac-arm64/Google Chrome for Testing.app/…        # TURBO_SURF_CHROME_PATH
└── v8cache/
    └── librusty_v8_simdutf_release_aarch64-apple-darwin.a.gz   # RUSTY_V8_ARCHIVE
```

Overrides: `TURBO_CACHE_ROOT`, `TURBO_ORACLE_ROOT`, `TURBO_CHROME_HEADLESS`,
`TURBO_SURF_CHROME_PATH`, `TURBO_V8_CACHE`, `RUSTY_V8_ARCHIVE`,
`CARGO_TARGET_DIR`.

Flags `oracle.sh` always sets:

- `--headless=new`
- `--use-mock-keychain`, `--password-store=basic` (no OS keyring access)
- `--host-resolver-rules="MAP * ~NOTFOUND , EXCLUDE 127.0.0.1, …"` (localhost
  only, no live network)
- `--no-default-browser-check`, `--no-first-run`, `--disable-sync`

## `CARGO_TARGET_DIR` under the scratchpad

Every script that runs cargo sets a scratchpad-rooted `CARGO_TARGET_DIR`
(default `$CLAUDE_SCRATCHPAD_DIR/turbo-surf-cargo-target`, else `$TMPDIR/…`).
That keeps simultaneous sibling sessions from racing over `rust/target`, and
survives a reboot only if the scratchpad does.

The checkout's `rust/target` may be a directory or a session-owned symlink.
Inspect it before choosing a target; do not replace another session's target or
assume that a path recorded in an old receipt is still valid. The wrappers set
`CARGO_TARGET_DIR` explicitly. Coordinate exclusive ownership if intentionally
sharing a cache, and keep acceptance/performance runs free of competing builds.

## Recording a Chrome 156 control

Format first, record second — in that order, always. The pre-commit hook
rewrites any staged `.js`/`.json` with biome, and that rewrite shifts source
hashes stored in the fixture. So:

```bash
scripts/agent/record-chrome-control.sh \
  harness/detection/your-feature-probe.js \
  harness/detection/fixtures/your-feature-chrome156.json
```

New probes ship as part of the vendored set in `.githooks/pre-commit` so the
hook stops re-formatting them once they land; add the probe + fixture to the
`VENDORED` regex in the same PR.

## Known-gap registry

Tolerated turbo-surf / Chrome differences live in
`harness/detection/chrome-identity-known-gaps.json`. Each entry names the field,
the reason, and the owner. To remove a rule: delete its entry, re-run
`scripts/agent/identity-battery.sh`, and let the parity diff pass without it.
See `harness/detection/CHROME-REVERSION.md` for the schema.

## Coverage floors + running one shard

`rust/coverage-floors.json` lists the per-crate floors (bindings excluded —
tarpaulin does not instrument them). CI shards the workspace; locally, run one
crate at a time so tarpaulin does not exhaust disk:

```bash
scripts/agent/coverage.sh turbo-surf-view
```

The merged workspace floor lives in `rust/tarpaulin.toml` (`fail-under`).
Never lower a floor to make a change land; raise coverage instead.

## Browser-environment compatibility modes

Default Surf execution uses the Chrome-oriented browser environment. A consumer
such as turbo-test can install its own compatibility policies through the pinned
turbo-dom API; those are not global Surf defaults or undocumented
`PageSessionOptions` flags. Inspect the current public options in
`rust/crates/turbo-surf-render/src/runtime.rs` and the consumer's integration
before documenting or selecting a mode. Build-time capabilities are listed in
[BUILD-FEATURES.md](./BUILD-FEATURES.md).

## Pre-commit hook gotchas

- The hook lives at `.githooks/pre-commit` and is wired by `npm run prepare`
  (sets `core.hooksPath`). If a format pass seems to loop, check the staged
  file is not in the hook's `VENDORED` regex: vendored probes and fixtures
  keep exact bytes for source-bound oracle comparisons.
- Biome refuses lockfiles; `package-lock.json` is filtered out of the staged
  set by the hook already.
- An isolated worktree needs the locked JS toolchain for the hook too. Confirm
  `npx --no-install` resolves the dependency version in `package-lock.json`;
  it can otherwise select an unrelated cached tool. Install the locked dependencies
  or temporarily link a verified matching dependency directory, and remove an
  untracked temporary link before freezing a clean source receipt.
- Existing target directories and session-owned symlinks are covered above.
- Never bypass with `--no-verify`.

## Release

The full release checklist is in [PUBLISHING.md](../PUBLISHING.md). The
per-agent view:

1. `scripts/agent/release-prep.sh X.Y.Z`
2. `npm run check` (and read the resulting diff)
3. `scripts/agent/identity-battery.sh` + `scripts/agent/parity-check.sh`
4. Commit `chore(release): vX.Y.Z`, push — tagging is the publish and only
   happens when the user asks.
