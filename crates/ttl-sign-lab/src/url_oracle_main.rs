//! Observe one URL-signing transformation without replaying the browser-issued request.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_core::SignError;
use ttl_sign_lab::webview_support::{engine_config, load_selected_case};
use ttl_sign_lab::{observe_signed_url, write_signing_observation, EnvironmentProfile};
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
            let signed = signer
                .sign_url(&unsigned_url)
                .await
                .map_err(SafeSignError::from)?;
            let effective_environment = EnvironmentProfile::from_preset(
                &signer.preset(),
                selected.environment.timestamp.clone(),
                selected.environment.session,
            );
            let observation = observe_signed_url(
                &selected,
                effective_environment,
                "webview",
                &unsigned_url,
                &signed.url,
                &signed.cookies,
                &signed.user_agent,
            )
            .context("could not sanitize URL-signing result")?;
            let added_parameters = observation.added_parameters.clone();
            let directory = write_signing_observation(&output_root, &observation)
                .context("could not persist URL-signing observation")?;
            println!(
                "{}",
                serde_json::json!({
                    "case_id": selected.id,
                    "capture_directory": directory,
                    "added_parameters": added_parameters,
                })
            );
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => shutdown.shutdown(),
            Err(error) => {
                eprintln!("URL-signing observation failed: {error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

/// Deliberately omits backend-provided messages, which may contain request material.
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

fn arguments() -> Result<(PathBuf, String, PathBuf)> {
    let usage = "usage: ttl-sign-url-oracle <plan.json> <case-id> <output-directory>";
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(args.next().context(usage)?);
    let case_id = args.next().context(usage)?.to_string_lossy().into_owned();
    let output = PathBuf::from(args.next().context(usage)?);
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok((plan, case_id, output))
}
