//! Detect structural Oracle-vs-Oracle regressions between two route subgraphs.
//!
//! Exit code 2 signals a structural difference. Entropy — signed-value lengths, call counts,
//! digests — never produces one.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::{compare_subgraphs, read_subgraph_document};

fn main() -> Result<()> {
    let (baseline_path, candidate_path) = arguments()?;
    let baseline = read_subgraph_document(baseline_path)?;
    let candidate = read_subgraph_document(candidate_path)?;
    let result = compare_subgraphs(&baseline, &candidate);
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !result.same_bundle {
        eprintln!(
            "note: the two documents describe different webmssdk bundles; structural differences \
             are expected rather than a regression"
        );
    }
    if result.is_structurally_equivalent() {
        Ok(())
    } else {
        std::process::exit(2);
    }
}

fn arguments() -> Result<(PathBuf, PathBuf)> {
    let usage = "usage: ttl-sign-subgraph-diff <baseline-subgraph.json> <candidate-subgraph.json>";
    let mut args = std::env::args_os().skip(1);
    let baseline = PathBuf::from(args.next().context(usage)?);
    let candidate = PathBuf::from(args.next().context(usage)?);
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok((baseline, candidate))
}
