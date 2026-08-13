//! Compare two sanitized URL-signing observations from a controlled experiment pair.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::{compare_signed_url_observations, read_signed_url_observation};

fn main() -> Result<()> {
    let (baseline_path, experiment_path) = arguments()?;
    let baseline = read_signed_url_observation(&baseline_path)?;
    let experiment = read_signed_url_observation(&experiment_path)?;
    let result = compare_signed_url_observations(&baseline, &experiment)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if result.is_match() {
        Ok(())
    } else {
        std::process::exit(2);
    }
}

fn arguments() -> Result<(PathBuf, PathBuf)> {
    let usage =
        "usage: ttl-sign-url-diff <baseline-observation.json> <experiment-observation.json>";
    let mut args = std::env::args_os().skip(1);
    let baseline = PathBuf::from(args.next().context(usage)?);
    let experiment = PathBuf::from(args.next().context(usage)?);
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok((baseline, experiment))
}
