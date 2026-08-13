//! Validate a controlled experiment plan without starting a browser.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;
use ttl_sign_lab::ExperimentPlan;

#[derive(Serialize)]
struct PlanSummary<'a> {
    plan_version: u32,
    baseline: &'a str,
    experiments: Vec<&'a str>,
    selected: Option<&'a str>,
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-plan <plan.json> [case-id]")?,
    );
    let selected = args
        .next()
        .map(|value| value.to_string_lossy().into_owned());
    if args.next().is_some() {
        anyhow::bail!("usage: ttl-sign-plan <plan.json> [case-id]");
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read experiment plan {}", path.display()))?;
    let plan: ExperimentPlan = serde_json::from_str(&raw)
        .with_context(|| format!("could not parse experiment plan {}", path.display()))?;
    plan.validate()
        .context("invalid controlled experiment plan")?;
    if let Some(id) = selected.as_deref() {
        plan.select(id).context("could not select experiment")?;
    }

    let summary = PlanSummary {
        plan_version: plan.plan_version,
        baseline: &plan.baseline.id,
        experiments: plan
            .experiments
            .iter()
            .map(|case| case.id.as_str())
            .collect(),
        selected: selected.as_deref(),
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
