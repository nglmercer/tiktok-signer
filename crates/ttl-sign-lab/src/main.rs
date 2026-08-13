//! Compare two replay corpora and emit structured JSON differential results.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::{DifferentialRunner, EnvironmentInput, ResearchCase};
use ttl_sign_replay::ReplayBackend;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let oracle_root = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-lab <oracle-fixtures> <candidate-fixtures>")?,
    );
    let candidate_root = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-lab <oracle-fixtures> <candidate-fixtures>")?,
    );
    if args.next().is_some() {
        anyhow::bail!("usage: ttl-sign-lab <oracle-fixtures> <candidate-fixtures>");
    }

    let oracle = load_replay(&oracle_root)
        .with_context(|| format!("could not load oracle input {}", oracle_root.display()))?;
    let candidate = load_replay(&candidate_root).with_context(|| {
        format!(
            "could not load candidate input {}",
            candidate_root.display()
        )
    })?;

    let mut results = Vec::new();
    for request in oracle.requests() {
        let case = ResearchCase {
            id: format!("room-{}", request.room_id),
            description: "replay corpus differential".into(),
            request,
            environment: EnvironmentInput {
                preset: "from-fixture".into(),
                timestamp_mode: "fixture".into(),
                session: "sanitized".into(),
            },
        };
        results.push(
            DifferentialRunner::compare(&case, "oracle", &oracle, "candidate", &candidate).await,
        );
    }

    println!("{}", serde_json::to_string_pretty(&results)?);
    if results.iter().any(|result| !result.is_match()) {
        std::process::exit(2);
    }
    Ok(())
}

fn load_replay(path: &PathBuf) -> Result<ReplayBackend, ttl_sign_replay::ReplayError> {
    if path.is_file() {
        ReplayBackend::load_case(path)
    } else {
        ReplayBackend::load(path)
    }
}
