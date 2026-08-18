//! Regression gate: the committed fixture tree must contain no live secrets.
//!
//! This runs on every `cargo test`, so a captured cookie, signed URL, or raw signature that
//! slips into `fixtures/` fails the build instead of entering git history.

use std::path::Path;

use ttl_fixture_hygiene::scan_dir;

#[test]
fn committed_fixtures_contain_no_live_secrets() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let findings = scan_dir(&fixtures).expect("fixtures directory must be scannable");
    assert!(
        findings.is_empty(),
        "committed fixtures contain live secrets:\n{}",
        findings
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
