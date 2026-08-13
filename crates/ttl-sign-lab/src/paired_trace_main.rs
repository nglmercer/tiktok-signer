//! Interleave baseline and one query mutation inside the same ephemeral WebView identity.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use ttl_sign_core::SignError;
use ttl_sign_lab::webview_support::{engine_config, load_plan};
use ttl_sign_lab::{
    build_signing_trace, collect_sdk_evidence, compare_signing_traces, observe_signed_url,
    write_signing_trace, EnvironmentProfile, ExperimentDimension,
};
use ttl_sign_webview::run;

const DEFAULT_REPETITIONS: usize = 5;
const DEFAULT_INTERVAL_MS: u64 = 1_100;
const MAX_REPETITIONS: usize = 20;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_lab=info,ttl_sign_webview=warn".into()),
        )
        .init();
    let arguments = match arguments() {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let plan = match load_plan(&arguments.plan) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let baseline = plan.baseline;
    let experiment = match plan
        .experiments
        .into_iter()
        .find(|case| case.id == arguments.experiment_id)
    {
        Some(experiment) => experiment,
        None => {
            eprintln!("unknown experiment case: {}", arguments.experiment_id);
            std::process::exit(2);
        }
    };
    let changed = experiment.changed_dimensions_from(&baseline);
    if changed != [ExperimentDimension::QueryMutation] {
        eprintln!(
            "paired traces currently require exactly one query_mutation; changed {changed:?}"
        );
        std::process::exit(2);
    }
    let baseline_url = match baseline.signing_url() {
        Ok(url) => url,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let experiment_url = match experiment.signing_url() {
        Ok(url) => url,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let config = match engine_config(&baseline) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };

    run(config, move |signer| {
        let shutdown = signer.clone();
        let runtime = tokio::runtime::Runtime::new().expect("could not create Tokio runtime");
        let result = runtime.block_on(async move {
            let sdk = collect_sdk_evidence(&signer, &baseline_url)
                .await
                .context("could not identify the loaded SDK")?;
            let mut baseline_samples = Vec::with_capacity(arguments.repetitions);
            let mut experiment_samples = Vec::with_capacity(arguments.repetitions);
            for round in 0..arguments.repetitions {
                baseline_samples.push(
                    sign_once(&signer, &baseline, &baseline_url)
                        .await
                        .context("baseline signing sample failed")?,
                );
                tokio::time::sleep(Duration::from_millis(arguments.interval_ms)).await;
                experiment_samples.push(
                    sign_once(&signer, &experiment, &experiment_url)
                        .await
                        .context("experiment signing sample failed")?,
                );
                if round + 1 != arguments.repetitions {
                    tokio::time::sleep(Duration::from_millis(arguments.interval_ms)).await;
                }
            }
            let baseline_trace = build_signing_trace(sdk.clone(), baseline_samples)?;
            let experiment_trace = build_signing_trace(sdk, experiment_samples)?;
            let differential = compare_signing_traces(&baseline_trace, &experiment_trace)?;
            let baseline_directory = write_signing_trace(&arguments.output, &baseline_trace)?;
            let experiment_directory = write_signing_trace(&arguments.output, &experiment_trace)?;
            println!(
                "{}",
                serde_json::json!({
                    "baseline_directory": baseline_directory,
                    "experiment_directory": experiment_directory,
                    "differential": differential,
                })
            );
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => shutdown.shutdown(),
            Err(error) => {
                eprintln!("paired signing trace failed: {error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

async fn sign_once(
    signer: &ttl_sign_webview::Signer,
    experiment: &ttl_sign_lab::ExperimentCase,
    unsigned_url: &str,
) -> Result<ttl_sign_lab::SignedUrlObservation> {
    let signed = signer
        .sign_url(unsigned_url)
        .await
        .map_err(SafeSignError::from)?;
    let effective_environment = EnvironmentProfile::from_preset(
        &signer.preset(),
        experiment.environment.timestamp.clone(),
        experiment.environment.session,
    );
    observe_signed_url(
        experiment,
        effective_environment,
        "webview",
        unsigned_url,
        &signed.url,
        &signed.cookies,
        &signed.user_agent,
    )
    .context("could not sanitize signing sample")
}

struct Arguments {
    plan: PathBuf,
    experiment_id: String,
    output: PathBuf,
    repetitions: usize,
    interval_ms: u64,
}

fn arguments() -> Result<Arguments> {
    let usage = "usage: ttl-sign-paired-trace <plan.json> <query-experiment-id> <output-directory> [repetitions] [interval-ms]";
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(args.next().context(usage)?);
    let experiment_id = args.next().context(usage)?.to_string_lossy().into_owned();
    let output = PathBuf::from(args.next().context(usage)?);
    let repetitions = args
        .next()
        .map(|value| value.to_string_lossy().parse())
        .transpose()
        .context("repetitions must be an integer")?
        .unwrap_or(DEFAULT_REPETITIONS);
    if !(2..=MAX_REPETITIONS).contains(&repetitions) {
        anyhow::bail!("repetitions must be between 2 and {MAX_REPETITIONS}");
    }
    let interval_ms = args
        .next()
        .map(|value| value.to_string_lossy().parse())
        .transpose()
        .context("interval-ms must be an integer")?
        .unwrap_or(DEFAULT_INTERVAL_MS);
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok(Arguments {
        plan,
        experiment_id,
        output,
        repetitions,
        interval_ms,
    })
}

#[derive(Debug, thiserror::Error)]
#[error("WebView signer returned {0}")]
struct SafeSignError(&'static str);

impl From<SignError> for SafeSignError {
    fn from(error: SignError) -> Self {
        Self(match error {
            SignError::SdkNotReady => "sdk_not_ready",
            SignError::NoInstanceAvailable => "no_instance_available",
            SignError::BackendUnavailable(_) => "backend_unavailable",
            SignError::Bridge(_) => "bridge",
            SignError::EngineGone(_) => "engine_gone",
            SignError::LoginTimeout(_) => "login_timeout",
            SignError::Timeout(_) => "timeout",
            SignError::Transport(_) => "transport",
            SignError::Decode(_) => "decode",
            SignError::Refused(_) => "refused",
        })
    }
}
