//! Guard against the v0.5.1 crates.io publish failure: a crate `default` feature must never
//! enable a **vendored-wreq-only** capability. `cargo publish` verifies a crate against the
//! crates.io `wreq` (path/patch overrides don't apply to a published crate's deps), so a default
//! that pulls in code using a field only present in `rust/vendor/wreq` (e.g.
//! `TlsOptions::trust_anchors`, gated by the `trust-anchors` feature) fails to compile at publish
//! time with `no field 'trust_anchors' on type '&mut TlsOptions'` — even though local/CI builds
//! (which use the vendored wreq) pass. Keep such features opt-in (the release binary enables them
//! explicitly); this test fails loudly if one is ever added to a crate `default`.

use std::fs;
use std::path::Path;

// Features that depend on APIs present ONLY in the vendored wreq (rust/vendor/wreq), not the
// crates.io release. None of these may appear in any crate's `default` feature list.
const VENDORED_ONLY_FEATURES: &[&str] = &["trust-anchors"];

#[test]
fn no_crate_default_enables_a_vendored_only_feature() {
    // .../rust/crates/turbo-surf-mcp -> .../rust/crates
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir");
    let mut checked = 0;
    for entry in fs::read_dir(crates_dir).expect("read crates dir") {
        let dir = entry.expect("dir entry").path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let text = fs::read_to_string(&manifest).expect("read Cargo.toml");
        let Some(default_line) = text
            .lines()
            .find(|l| l.trim_start().starts_with("default = ["))
        else {
            continue; // crate has no explicit default features
        };
        checked += 1;
        for feat in VENDORED_ONLY_FEATURES {
            assert!(
                !default_line.contains(&format!("\"{feat}\"")),
                "{}: `default` enables vendored-wreq-only feature `{feat}` — this breaks \
                 `cargo publish` (crates.io wreq lacks its API). Make it opt-in and enable it \
                 in the release-binary build instead. Offending line: {default_line}",
                manifest.display()
            );
        }
    }
    assert!(checked > 0, "no crate `default` lines found — path wrong?");
}
