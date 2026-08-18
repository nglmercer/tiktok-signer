//! Signing outcomes.
//!
//! **Rejection and transport errors never share a variant.** A 200 with an empty body is
//! not transient; it means TikTok rejected the request, and retrying makes things worse.

use crate::cookie::CookieJar;

/// Successful signature: the protobuf bytes **as returned** by TikTok, plus the context
/// required to open the WebSocket with the same identity.
#[derive(Debug, Clone)]
pub struct SignedFetch {
    /// Response body without wrapping or transformation.
    pub protobuf: Vec<u8>,
    /// Cookies from the signing session. Sent in `X-Set-TT-Cookie` and `Cookie` headers.
    pub cookies: CookieJar,
    /// Exact User-Agent used for signing. The WebSocket must use the same value.
    pub user_agent: String,
    /// Final request URL with `X-Bogus`/`X-Gnarly` added by the SDK.
    pub signed_url: String,
}

/// Why TikTok rejected the request, as observable externally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// 200 with an empty body.
    EmptyBody,
    /// 200 with valid protobuf but empty `push_server`: silent rejection.
    EmptyPushServer,
    /// HTTP status other than 200.
    HttpStatus(u16),
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBody => write!(f, "empty body (silent rejection)"),
            Self::EmptyPushServer => write!(f, "empty push_server (silent rejection)"),
            Self::HttpStatus(code) => write!(f, "TikTok returned HTTP {code}"),
        }
    }
}

/// Failures that **can** be transient or configuration-related, never detection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignError {
    /// `window.byted_acrawler` did not appear before the deadline.
    #[error("TikTok SDK was not ready before the deadline")]
    SdkNotReady,

    /// The pool has no available signer instance.
    #[error("no signer backend instance is available")]
    NoInstanceAvailable,

    /// A backend is present but cannot serve this request.
    #[error("signer backend is unavailable: {0}")]
    BackendUnavailable(String),

    /// The page returned a JavaScript error.
    #[error("page error: {0}")]
    Bridge(String),

    /// The signing engine closed or stopped responding.
    #[error("signing engine is unavailable: {0}")]
    EngineGone(String),

    /// Nobody signed in before the login deadline.
    #[error("login deadline expired ({0} s)")]
    LoginTimeout(u64),

    /// Signing exceeded the allowed time. The result expires after roughly 30 seconds.
    #[error("signing exceeded the deadline ({0} ms)")]
    Timeout(u64),

    /// Network error while communicating with TikTok.
    #[error("transport error: {0}")]
    Transport(String),

    /// The response could not be decoded.
    #[error("could not decode response: {0}")]
    Decode(String),

    /// TikTok answered `200` and refused in the envelope (`status_code != 0`).
    ///
    /// Distinct from [`SignError::Transport`] and [`SignError::Decode`] because it is
    /// neither a network problem nor a parsing bug: the request arrived and was understood.
    /// Whether retrying helps depends on the code, so this carries the raw value instead of
    /// deciding for the caller.
    #[error("{0}")]
    Refused(crate::room::WebcastRefusal),
}

impl SignError {
    /// Did TikTok understand the request and refuse it?
    ///
    /// Retrying a refusal in a tight loop is what turns a soft limit into a hard one.
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::Refused(_))
    }
}

/// The three possible signing outcomes. There is no fourth.
#[derive(Debug, Clone)]
pub enum SignOutcome {
    /// Valid signature.
    Ok(SignedFetch),
    /// TikTok rejected the request. **Do not retry**: it is not transient.
    Rejected(RejectReason),
    /// Something failed in transit. A careful retry may make sense.
    Transport(SignError),
}

impl SignOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Is this a rejection? Used by the watchdog to count consecutive rejections.
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected(_))
    }

    /// Does retrying make sense? Always `false` for rejections.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    pub fn ok(self) -> Option<SignedFetch> {
        match self {
            Self::Ok(f) => Some(f),
            _ => None,
        }
    }
}

impl From<SignError> for SignOutcome {
    fn from(e: SignError) -> Self {
        Self::Transport(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejection_is_never_retryable() {
        for reason in [
            RejectReason::EmptyBody,
            RejectReason::EmptyPushServer,
            RejectReason::HttpStatus(403),
        ] {
            let outcome = SignOutcome::Rejected(reason);
            assert!(outcome.is_rejected());
            assert!(
                !outcome.is_retryable(),
                "a rejection is never retried: it is detection, not a transient error"
            );
        }
    }

    #[test]
    fn transport_errors_are_retryable() {
        let outcome = SignOutcome::Transport(SignError::Transport("connection reset".into()));
        assert!(outcome.is_retryable());
        assert!(!outcome.is_rejected());
    }
}
