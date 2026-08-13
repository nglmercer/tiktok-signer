//! Run one explicitly selected research case against the optional WebView oracle.
//!
//! Each invocation owns one WebView process. This prevents mutable browser state from one
//! experiment contaminating the next and permits profiles that require engine recreation.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_core::SignerBackend;
use ttl_sign_lab::webview_support::{engine_config, load_selected_case};
use ttl_sign_lab::{capture_experiment_outcome, write_capture};
use ttl_sign_webview::run;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_lab=info,ttl_sign_webview=warn".into()),
        )
        .init();

    let (plan_path, case_id, output_root) = match arguments() {
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
    let research_case = selected.research_case();
    let experiment = selected.clone();

    run(config, move |signer| {
        let shutdown = signer.clone();
        let runtime = tokio::runtime::Runtime::new().expect("could not create Tokio runtime");
        let result = runtime.block_on(async move {
            let outcome = signer.transport(research_case.request.clone()).await;
            let bundle = capture_experiment_outcome(&experiment, "webview", outcome)
                .context("could not sanitize oracle result")?;
            let directory =
                write_capture(&output_root, &bundle).context("could not persist oracle capture")?;
            println!(
                "{}",
                serde_json::json!({
                    "case_id": research_case.id,
                    "capture_directory": directory,
                    "outcome": bundle.observation.outcome,
                })
            );
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => shutdown.shutdown(),
            Err(error) => {
                eprintln!("{error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

fn arguments() -> Result<(PathBuf, String, PathBuf)> {
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-oracle <plan.json> <case-id> <output-directory>")?,
    );
    let case_id = args
        .next()
        .context("usage: ttl-sign-oracle <plan.json> <case-id> <output-directory>")?
        .to_string_lossy()
        .into_owned();
    let output = PathBuf::from(
        args.next()
            .context("usage: ttl-sign-oracle <plan.json> <case-id> <output-directory>")?,
    );
    if args.next().is_some() {
        anyhow::bail!("usage: ttl-sign-oracle <plan.json> <case-id> <output-directory>");
    }
    Ok((plan, case_id, output))
}
