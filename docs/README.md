# turbo-surf docs

turbo-surf is a single native (Rust) engine — there is no per-module JS reference
anymore (the engine is Rust; its modules are crates). Start here:

## Overview & product
- [`../README.md`](../README.md) — what it is, install, the MCP server, the
  Playwright drop-in, and the benchmarks.
- [`../CHANGELOG.md`](../CHANGELOG.md) — what shipped.
- [`../PUBLISHING.md`](../PUBLISHING.md) — how releases cut (npm launcher + crates).
- [`../AGENTS.md`](../AGENTS.md) — working rules for contributors.
- [`QUALITY-GATES.md`](./QUALITY-GATES.md) — the CI coverage and complexity gates, their
  floors and the complexity refactor backlog.

## Engine (the Rust workspace)
- [`../rust/README.md`](../rust/README.md) — the 9-crate workspace, the tiers
  (net/crawl → view/extract → V8 render), and how they fit.
- [`../rust/HEADLESS-HYDRATION.md`](../rust/HEADLESS-HYDRATION.md) — the no-browser
  JS render tier (a `deno_core` V8 isolate over the native rtdom DOM + the
  `turbo-dom-browser-env` binding).
- [`../rust/crates/turbo-surf-napi/README.md`](../rust/crates/turbo-surf-napi/README.md)
  — the dev/harness in-process addon (not published).

## Agent & test surfaces
- **MCP server** — 80 tools over stdio; see the "MCP server" section of
  [`../README.md`](../README.md).
- **Playwright drop-in** — [`../rust/playwright-shim/LIMITATIONS.md`](../rust/playwright-shim/LIMITATIONS.md)
  is the per-method coverage map (what's supported / no-op / throws).

## Harness (benchmarks, live network)
- [`../harness/competitive/README.md`](../harness/competitive/README.md) —
  same-script parity + timing vs real browsers.
- [`../harness/crawlers/README.md`](../harness/crawlers/README.md) —
  crawler-vs-crawler throughput.

- [`../harness/detection/README.md`](../harness/detection/README.md) — reusable
  Chrome/native JS-interface and application-network traces, fresh cookies, controls
  and all native capability features.
- [`../harness/detection/FINDINGS.md`](../harness/detection/FINDINGS.md) — dated
  Google evidence, code audit and remaining gaps.
