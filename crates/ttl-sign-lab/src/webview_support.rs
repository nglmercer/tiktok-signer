//! Shared setup for explicitly enabled WebView research binaries.

use std::path::Path;

use anyhow::{Context, Result};
use ttl_sign_core::CookieJar;
use ttl_sign_webview::{session, EngineConfig, PageEnvironmentOverride};

use crate::{ExperimentCase, ExperimentPlan, SessionMode, TimestampMode};

/// Ceiling on the inspected bundle. The served bundle is ~230 KB; anything near this limit is a
/// different resource and should fail rather than be parsed.
const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;

/// Download the signing bundle with the page's own User-Agent.
///
/// Shared by every research binary that has to read the bundle: they must all fetch the same
/// bytes the page did, or the digest they pin is meaningless.
pub async fn download_bundle(endpoint: &str, user_agent: &str) -> Result<Vec<u8>> {
    let response = reqwest::Client::builder()
        .user_agent(user_agent)
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(endpoint)
        .send()
        .await?
        .error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BUNDLE_BYTES as u64)
    {
        anyhow::bail!("webmssdk bundle exceeds the inspection limit");
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_BUNDLE_BYTES {
        anyhow::bail!("webmssdk bundle exceeds the inspection limit");
    }
    Ok(bytes.to_vec())
}

pub fn load_selected_case(path: &Path, id: &str) -> Result<ExperimentCase> {
    let plan = load_plan(path)?;
    Ok(plan
        .select(id)
        .context("invalid or unknown controlled experiment")?
        .clone())
}

pub fn load_plan(path: &Path) -> Result<ExperimentPlan> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("could not read experiment plan {}", path.display()))?;
    let plan: ExperimentPlan = serde_json::from_str(&raw)
        .with_context(|| format!("could not parse experiment plan {}", path.display()))?;
    plan.validate()
        .context("invalid controlled experiment plan")?;
    Ok(plan)
}

pub fn engine_config(case: &ExperimentCase) -> Result<EngineConfig> {
    let session = match case.environment.session {
        SessionMode::Guest => CookieJar::new(),
        SessionMode::Configured => load_configured_session()?,
    };
    let timestamp_ms = match &case.environment.timestamp {
        TimestampMode::System => None,
        TimestampMode::Fixed { timestamp_ms } => Some(*timestamp_ms),
    };
    let mut session = session;
    let mut page_cookies = CookieJar::new();
    if let Some(signing) = &case.signing {
        if let Some(cookie_mutation) = &signing.cookie_mutation {
            match cookie_mutation {
                crate::CookieMutation::Remove { name } => session.remove(name),
                crate::CookieMutation::SetProbe { .. } => cookie_mutation.apply(&mut page_cookies),
            }
        }
    }
    Ok(EngineConfig {
        preset: case.environment.preset(),
        session,
        incognito: matches!(case.environment.session, SessionMode::Guest),
        page_cookies,
        page_environment: Some(PageEnvironmentOverride {
            language: Some(case.environment.language.clone()),
            browser_language: Some(case.environment.browser_language.clone()),
            browser_platform: Some(case.environment.browser_platform.clone()),
            tz_name: Some(case.environment.tz_name.clone()),
            region: Some(case.environment.region.clone()),
            screen_width: Some(case.environment.screen_width),
            screen_height: Some(case.environment.screen_height),
            timestamp_ms,
        }),
        ..EngineConfig::default()
    })
}

fn load_configured_session() -> Result<CookieJar> {
    let path = session::configured_path().context("no configured session path is available")?;
    let cookies = session::load(&path)
        .with_context(|| format!("could not read configured session {}", path.display()))?
        .context("configured session file does not exist")?;
    if !session::is_logged_in(&cookies) {
        anyhow::bail!("configured session does not contain a non-empty sessionid");
    }
    Ok(cookies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EnvironmentProfile, SigningInput};
    use ttl_sign_core::TransportRequest;

    fn case() -> ExperimentCase {
        ExperimentCase {
            id: "baseline".into(),
            description: "baseline".into(),
            request: TransportRequest::new("7300000000000000001"),
            signing: Some(SigningInput {
                device_id: "7123456789012345678".into(),
                cursor: String::new(),
                internal_ext: String::new(),
                contact_us: String::new(),
                sup_ws_ds_opt: 1,
                query_mutation: None,
                cookie_mutation: None,
            }),
            environment: EnvironmentProfile::chrome_linux_guest(),
        }
    }

    #[test]
    fn guest_config_uses_the_declared_profile() {
        let case = case();
        let config = engine_config(&case).unwrap();
        assert_eq!(config.preset, case.environment.preset());
        assert!(config.session.is_empty());
    }

    #[test]
    fn config_emulates_a_fixed_clock_instead_of_mislabeling_it() {
        let mut case = case();
        case.environment.timestamp = TimestampMode::Fixed { timestamp_ms: 1 };
        let config = engine_config(&case).unwrap();
        assert_eq!(
            config
                .page_environment
                .as_ref()
                .and_then(|override_| override_.timestamp_ms),
            Some(1)
        );
        assert_eq!(
            config
                .page_environment
                .as_ref()
                .and_then(|override_| override_.tz_name.as_deref()),
            Some("America/New_York")
        );
    }
}
