//! The shipped sandbox must be the one in `scripts/headless`, not a copy that drifted from it.
//!
//! `bootstrap.js` is generated from `shim.mjs` and `lib/canvas.mjs`. Committing a generated file is
//! a deliberate trade — the crate builds without Node — but it means an edit to the shim can leave
//! the embedded signer running last week's sandbox, signing correctly and differently. This
//! regenerates and compares.
//!
//! Skips when Node or the sources are absent, so a fresh clone still runs `cargo test` offline.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repository root")
}

#[test]
fn the_committed_bootstrap_matches_the_shim() {
    let root = root();
    let generator = root.join("scripts/headless/tools/build-bootstrap.mjs");
    if !generator.is_file() {
        eprintln!("skipped: no generator at {}", generator.display());
        return;
    }
    let temporary = std::env::temp_dir().join("ttl-bootstrap-freshness.js");
    let generated = Command::new("node").arg(&generator).arg(&temporary).output();
    let Ok(output) = generated else {
        eprintln!("skipped: node is not available");
        return;
    };
    assert!(
        output.status.success(),
        "the generator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let fresh = std::fs::read_to_string(&temporary).expect("generated bootstrap");
    let shipped = include_str!("../bootstrap.js");
    let _ = std::fs::remove_file(&temporary);

    assert_eq!(
        fresh,
        shipped,
        "crates/ttl-sign-embedded/bootstrap.js is stale. Regenerate it:\n  \
         node scripts/headless/tools/build-bootstrap.mjs crates/ttl-sign-embedded/bootstrap.js"
    );
}
