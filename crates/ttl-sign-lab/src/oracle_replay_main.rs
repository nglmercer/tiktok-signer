//! Compare the WebView oracle with one sanitized replay candidate on the same research case.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::webview_support::{engine_config, load_selected_case};
use ttl_sign_lab::DifferentialRunner;
use ttl_sign_replay::ReplayBackend;
use ttl_sign_webview::run;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_lab=info,ttl_sign_webview=warn".into()),
        )
        .init();

    let (plan_path, case_id, replay_path) = match arguments() {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let selected = match load_selected_case(&plan_path, &case_id) {
        Ok(case) => case,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let config = match engine_config(&selected) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let replay = match ReplayBackend::load_case(&replay_path) {
        Ok(replay) => replay,
        Err(error) => {
            eprintln!(
                "could not load replay candidate {}: {error}",
                replay_path.display()
            );
            std::process::exit(2);
        }
    };
    let research_case = selected.research_case();

    run(config, move |signer| {
        let shutdown = signer.clone();
        let runtime = tokio::runtime::Runtime::new().expect("could not create Tokio runtime");
        let result = runtime.block_on(async move {
            let result =
                DifferentialRunner::compare(&research_case, "webview", &signer, "replay", &replay)
                    .await;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Result::<bool>::Ok(result.is_match())
        });
        match result {
            Ok(true) => shutdown.shutdown(),
            Ok(false) => shutdown.shutdown_with_code(2),
            Err(error) => {
                eprintln!("could not compare oracle and replay candidate: {error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

fn arguments() -> Result<(PathBuf, String, PathBuf)> {
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-oracle-replay <plan.json> <case-id> <candidate-case.json>")?,
    );
    let case_id = args
        .next()
        .context("usage: ttl-sign-oracle-replay <plan.json> <case-id> <candidate-case.json>")?
        .to_string_lossy()
        .into_owned();
    let replay = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-oracle-replay <plan.json> <case-id> <candidate-case.json>")?,
    );
    if args.next().is_some() {
        anyhow::bail!("usage: ttl-sign-oracle-replay <plan.json> <case-id> <candidate-case.json>");
    }
    Ok((plan, case_id, replay))
}
