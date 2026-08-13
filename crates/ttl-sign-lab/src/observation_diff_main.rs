//! Compare a baseline observation with exactly one controlled experiment observation.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::{compare_experiment_artifacts, read_observation_artifact};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let baseline_path = PathBuf::from(args.next().context(
        "usage: ttl-sign-observation-diff <baseline-observation.json> <experiment-observation.json>",
    )?);
    let experiment_path = PathBuf::from(args.next().context(
        "usage: ttl-sign-observation-diff <baseline-observation.json> <experiment-observation.json>",
    )?);
    if args.next().is_some() {
        anyhow::bail!(
            "usage: ttl-sign-observation-diff <baseline-observation.json> <experiment-observation.json>"
        );
    }

    let baseline = read_observation_artifact(&baseline_path)
        .with_context(|| format!("could not load baseline {}", baseline_path.display()))?;
    let experiment = read_observation_artifact(&experiment_path)
        .with_context(|| format!("could not load experiment {}", experiment_path.display()))?;
    let result = compare_experiment_artifacts(&baseline, &experiment)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
