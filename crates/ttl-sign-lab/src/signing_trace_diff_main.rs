//! Compare repeated signing traces while ignoring individual entropy values.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::{compare_signing_traces, read_signing_trace};

fn main() -> Result<()> {
    let (baseline_path, experiment_path) = arguments()?;
    let baseline = read_signing_trace(baseline_path)?;
    let experiment = read_signing_trace(experiment_path)?;
    let result = compare_signing_traces(&baseline, &experiment)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if result.is_behaviorally_equivalent() {
        Ok(())
    } else {
        std::process::exit(2);
    }
}

fn arguments() -> Result<(PathBuf, PathBuf)> {
    let usage = "usage: ttl-sign-trace-diff <baseline-trace.json> <experiment-trace.json>";
    let mut args = std::env::args_os().skip(1);
    let baseline = PathBuf::from(args.next().context(usage)?);
    let experiment = PathBuf::from(args.next().context(usage)?);
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok((baseline, experiment))
}
