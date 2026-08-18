//! Artifact-hygiene gate CLI.
//!
//! ```sh
//! cargo run -p ttl-fixture-hygiene -- fixtures
//! ```
//!
//! Scans one or more paths for committed live secrets (session cookies, reusable signed URLs,
//! raw signature values). Exits `0` when clean, `1` when a leak is found, `2` on I/O error.
//! Findings are printed sanitized: rule, location, non-secret field name, and value length —
//! never the secret itself.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ttl_fixture_hygiene::{scan_dir, scan_path, Finding};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let roots: Vec<PathBuf> = if args.is_empty() {
        vec![PathBuf::from("fixtures")]
    } else {
        args.into_iter().map(PathBuf::from).collect()
    };

    let mut findings: Vec<Finding> = Vec::new();
    for root in &roots {
        match scan_root(root) {
            Ok(mut found) => findings.append(&mut found),
            Err(error) => {
                eprintln!("error: could not scan {}: {error}", root.display());
                return ExitCode::from(2);
            }
        }
    }

    if findings.is_empty() {
        let joined: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        println!("fixture hygiene: clean ({})", joined.join(", "));
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "fixture hygiene: {} violation(s) — committed fixtures must be sanitized",
        findings.len()
    );
    for finding in &findings {
        eprintln!("  {finding}");
        eprintln!("      → {}", finding.rule.description());
    }
    eprintln!(
        "\nCommitted fixtures may only contain hashes, lengths, field names/ordering, \
         fixed-vocabulary labels, and synthetic ids. See fixtures/NOTES.md."
    );
    ExitCode::from(1)
}

fn scan_root(root: &Path) -> std::io::Result<Vec<Finding>> {
    let metadata = std::fs::metadata(root)?;
    if metadata.is_dir() {
        scan_dir(root)
    } else {
        scan_path(root)
    }
}
