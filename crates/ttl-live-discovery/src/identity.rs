use std::time::Duration;

use ttl_sign_core::CookieJar;

const DEFAULT_GUEST_URL: &str = "https://www.tiktok.com/live";

#[derive(Debug, Clone)]
pub struct GuestIdentity {
    pub cookies: CookieJar,
    pub device_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("guest bootstrap transport failed: {0}")]
    Transport(String),
    #[error("TikTok refused guest bootstrap with HTTP {0}")]
    Refused(u16),
    #[error("TikTok returned no usable guest cookies")]
    NoCookies,
}

/// Bootstrap one reusable anonymous web identity.
///
/// The identity is created once at process startup and passed to both discovery and the warm
/// signer. It is intentionally not persisted by this helper; deployments that want a reusable
/// account/session should supply `TTL_SESSION_COOKIE` or `TTL_SESSION_FILE` instead.
pub async fn bootstrap_guest_identity(
    user_agent: &str,
    timeout: Duration,
) -> Result<GuestIdentity, IdentityError> {
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(timeout)
        .build()
        .map_err(|error| IdentityError::Transport(error.to_string()))?;
    let mut response = client
        .get(DEFAULT_GUEST_URL)
        .header("accept-language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|error| IdentityError::Transport(error.to_string()))?;
    let status = response.status().as_u16();
    let mut cookies = CookieJar::new();
    for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
        let Ok(line) = value.to_str() else { continue };
        let pair = line.split(';').next().unwrap_or_default();
        let Some((name, cookie_value)) = pair.split_once('=') else {
            continue;
        };
        if !name.trim().is_empty() {
            cookies.set(name.trim(), cookie_value.trim());
        }
    }
    // Drain a bounded response without buffering executable/HTML content.
    let mut drained = 0usize;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| IdentityError::Transport(e.to_string()))?
    {
        drained = drained.saturating_add(chunk.len());
        if drained > 2 * 1024 * 1024 {
            break;
        }
    }

    if !(200..300).contains(&status) {
        return Err(IdentityError::Refused(status));
    }
    if cookies.get("ttwid").is_none_or(str::is_empty) {
        return Err(IdentityError::NoCookies);
    }
    let device_id = cookies
        .get("tt_webid_v2")
        .or_else(|| cookies.get("tt_webid"))
        .map(str::to_owned);
    Ok(GuestIdentity { cookies, device_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_error_does_not_include_cookie_values() {
        let error = IdentityError::NoCookies;
        assert!(!error.to_string().contains("session"));
    }
}
