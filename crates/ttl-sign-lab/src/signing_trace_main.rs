//! Repeat one identical WebView signing input and classify stable versus variable fields.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use ttl_sign_core::SignError;
use ttl_sign_lab::webview_support::{engine_config, load_selected_case};
use ttl_sign_lab::{
    build_signing_trace, collect_sdk_evidence, observe_signed_url, write_signing_trace,
    EnvironmentProfile, ParameterOrigin, RepetitionStability,
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
    let selected = match load_selected_case(&arguments.plan, &arguments.case_id) {
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
    let unsigned_url = match selected.signing_url() {
        Ok(url) => url,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    run(config, move |signer| {
        let shutdown = signer.clone();
        let runtime = tokio::runtime::Runtime::new().expect("could not create Tokio runtime");
        let result = runtime.block_on(async move {
            let sdk = collect_sdk_evidence(&signer, &unsigned_url)
                .await
                .context("could not identify the loaded SDK")?;
            let mut samples = Vec::with_capacity(arguments.repetitions);
            for index in 0..arguments.repetitions {
                let signed = signer
                    .sign_url(&unsigned_url)
                    .await
                    .map_err(SafeSignError::from)?;
                let effective_environment = EnvironmentProfile::from_preset(
                    &signer.preset(),
                    selected.environment.timestamp.clone(),
                    selected.environment.session,
                );
                samples.push(
                    observe_signed_url(
                        &selected,
                        effective_environment,
                        "webview",
                        &unsigned_url,
                        &signed.url,
                        &signed.cookies,
                        &signed.user_agent,
                    )
                    .context("could not sanitize a signing sample")?,
                );
                if index + 1 != arguments.repetitions {
                    tokio::time::sleep(Duration::from_millis(arguments.interval_ms)).await;
                }
            }
            let trace = build_signing_trace(sdk, samples)?;
            let directory = write_signing_trace(&arguments.output, &trace)?;
            let variable_parameters: Vec<_> = trace
                .parameter_stability
                .iter()
                .filter(|slot| {
                    slot.origin == ParameterOrigin::AddedBySdk
                        && slot.stability != RepetitionStability::Stable
                })
                .map(|slot| format!("{}#{}", slot.name, slot.occurrence))
                .collect();
            let candidate_resources: Vec<_> = trace
                .sdk
                .resources
                .iter()
                .filter(|resource| resource.likely_sdk || !resource.markers.is_empty())
                .map(|resource| {
                    serde_json::json!({
                        "endpoint": resource.endpoint,
                        "markers": resource.markers,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "case_id": trace.case_id,
                    "repetitions": trace.repetitions,
                    "capture_directory": directory,
                    "sdk_version": trace.sdk.version,
                    "sdk_symbols": trace.sdk.symbols,
                    "variable_sdk_parameters": variable_parameters,
                    "candidate_sdk_resources": candidate_resources,
                })
            );
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => shutdown.shutdown(),
            Err(error) => {
                eprintln!("signing trace failed: {error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

struct Arguments {
    plan: PathBuf,
    case_id: String,
    output: PathBuf,
    repetitions: usize,
    interval_ms: u64,
}

fn arguments() -> Result<Arguments> {
    let usage = "usage: ttl-sign-trace <plan.json> <case-id> <output-directory> [repetitions] [interval-ms]";
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(args.next().context(usage)?);
    let case_id = args.next().context(usage)?.to_string_lossy().into_owned();
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
        case_id,
        output,
        repetitions,
        interval_ms,
    })
}

/// Deliberately omits page-provided messages, which may contain request material.
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
