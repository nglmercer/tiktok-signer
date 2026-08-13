//! Deterministic, offline implementation of the signing backend contract.
//!
//! A replay fixture contains sanitized observable transport metadata. Loading never reaches
//! the network, and an unknown request fails explicitly instead of synthesizing a fallback.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ttl_sign_core::{
    BackendFuture, ClientIdentity, CookieJar, FetchResult, RejectReason, SignError, SignOutcome,
    SignedFetch, SignerBackend, TransportRequest,
};

pub const FIXTURE_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("could not read replay fixture {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse replay fixture {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("unsupported fixture version {found} in {path}; expected {expected}")]
    UnsupportedVersion {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    #[error("invalid replay fixture {path}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error("duplicate replay request for room {room_id}")]
    Duplicate { room_id: String },
    #[error("fixture corpus contains different user agents")]
    MixedIdentity,
    #[error("fixture corpus {0} contains no case.json files")]
    EmptyCorpus(PathBuf),
}

/// Serializable research input. No ambient preset or room state is implied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRequest {
    pub room_id: String,
}

/// Reproducibility metadata needed to interpret an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureEnvironment {
    pub preset: String,
    pub user_agent: String,
    pub session: String,
}

/// One versioned, sanitized replay case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCase {
    pub fixture_version: u32,
    pub case_id: String,
    pub description: String,
    pub request: FixtureRequest,
    pub environment: FixtureEnvironment,
    pub expected: FixtureOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixtureOutcome {
    Success {
        transport: TransportFixture,
        cookies: Vec<(String, String)>,
    },
    Rejected {
        reason: FixtureRejectReason,
    },
    Error {
        error: FixtureSignError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportFixture {
    pub push_server: String,
    pub route_params: Vec<(String, String)>,
    pub cursor: String,
    pub internal_ext: String,
    pub heartbeat_duration: u64,
    pub need_ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixtureRejectReason {
    EmptyBody,
    EmptyPushServer,
    HttpStatus { code: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixtureSignError {
    SdkNotReady,
    NoInstanceAvailable,
    BackendUnavailable { message: String },
    Bridge { message: String },
    EngineGone { message: String },
    LoginTimeout { seconds: u64 },
    Timeout { milliseconds: u64 },
    Transport { message: String },
    Decode { message: String },
    Refused { status_code: i64, message: String },
}

impl ReplayCase {
    /// Validate fixture version, required fields, and the sanitization contract.
    pub fn validate(&self) -> Result<(), ReplayError> {
        self.validate_at(Path::new("<memory>"))
    }

    fn validate_at(&self, path: &Path) -> Result<(), ReplayError> {
        if self.fixture_version != FIXTURE_VERSION {
            return Err(ReplayError::UnsupportedVersion {
                path: path.to_path_buf(),
                found: self.fixture_version,
                expected: FIXTURE_VERSION,
            });
        }
        if self.case_id.is_empty() {
            return invalid(path, "case_id is empty");
        }
        if self.request.room_id.is_empty()
            || !self.request.room_id.chars().all(|c| c.is_ascii_digit())
        {
            return invalid(path, "room_id must be non-empty and numeric");
        }
        if self.environment.user_agent.is_empty() {
            return invalid(path, "environment.user_agent is empty");
        }
        if !matches!(self.environment.session.as_str(), "guest" | "sanitized")
            && !is_placeholder(&self.environment.session)
        {
            return invalid(
                path,
                "environment.session must be guest, sanitized, or a fixture-* placeholder",
            );
        }
        if let FixtureOutcome::Success { transport, cookies } = &self.expected {
            if transport.push_server.contains('?') || transport.push_server.contains('#') {
                return invalid(path, "push_server must not contain signed query material");
            }
            if transport.push_server.is_empty() || transport.route_params.is_empty() {
                return invalid(path, "successful transport is incomplete");
            }
            for (name, value) in cookies.iter().chain(transport.route_params.iter()) {
                if name.is_empty() || (!value.is_empty() && !is_placeholder(value)) {
                    return invalid(
                        path,
                        "cookie and route parameter values must use fixture-* placeholders",
                    );
                }
            }
            for value in [&transport.cursor, &transport.internal_ext] {
                if !value.is_empty() && !is_placeholder(value) {
                    return invalid(
                        path,
                        "cursor and internal_ext must use fixture-* placeholders",
                    );
                }
            }
        }
        if let FixtureOutcome::Error { error } = &self.expected {
            let message = match error {
                FixtureSignError::BackendUnavailable { message }
                | FixtureSignError::Bridge { message }
                | FixtureSignError::EngineGone { message }
                | FixtureSignError::Transport { message }
                | FixtureSignError::Decode { message }
                | FixtureSignError::Refused { message, .. } => Some(message),
                FixtureSignError::SdkNotReady
                | FixtureSignError::NoInstanceAvailable
                | FixtureSignError::LoginTimeout { .. }
                | FixtureSignError::Timeout { .. } => None,
            };
            if message.is_some_and(|message| !is_placeholder(message)) {
                return invalid(path, "error messages must use fixture-* placeholders");
            }
        }
        Ok(())
    }

    fn into_outcome(self) -> SignOutcome {
        match self.expected {
            FixtureOutcome::Success { transport, cookies } => {
                let fetch = FetchResult {
                    push_server: transport.push_server,
                    route_params: transport.route_params,
                    cursor: transport.cursor,
                    internal_ext: transport.internal_ext,
                    heartbeat_duration: transport.heartbeat_duration,
                    need_ack: transport.need_ack,
                };
                SignOutcome::Ok(SignedFetch {
                    protobuf: fetch.encode(),
                    cookies: cookies.into_iter().collect::<CookieJar>(),
                    user_agent: self.environment.user_agent,
                    signed_url: fetch.push_server,
                })
            }
            FixtureOutcome::Rejected { reason } => SignOutcome::Rejected(match reason {
                FixtureRejectReason::EmptyBody => RejectReason::EmptyBody,
                FixtureRejectReason::EmptyPushServer => RejectReason::EmptyPushServer,
                FixtureRejectReason::HttpStatus { code } => RejectReason::HttpStatus(code),
            }),
            FixtureOutcome::Error { error } => SignOutcome::Transport(match error {
                FixtureSignError::SdkNotReady => SignError::SdkNotReady,
                FixtureSignError::NoInstanceAvailable => SignError::NoInstanceAvailable,
                FixtureSignError::BackendUnavailable { message } => {
                    SignError::BackendUnavailable(message)
                }
                FixtureSignError::Bridge { message } => SignError::Bridge(message),
                FixtureSignError::EngineGone { message } => SignError::EngineGone(message),
                FixtureSignError::LoginTimeout { seconds } => SignError::LoginTimeout(seconds),
                FixtureSignError::Timeout { milliseconds } => SignError::Timeout(milliseconds),
                FixtureSignError::Transport { message } => SignError::Transport(message),
                FixtureSignError::Decode { message } => SignError::Decode(message),
                FixtureSignError::Refused {
                    status_code,
                    message,
                } => SignError::Refused(ttl_sign_core::WebcastRefusal {
                    status_code,
                    message,
                }),
            }),
        }
    }
}

fn invalid<T>(path: &Path, message: impl Into<String>) -> Result<T, ReplayError> {
    Err(ReplayError::Invalid {
        path: path.to_path_buf(),
        message: message.into(),
    })
}

fn is_placeholder(value: &str) -> bool {
    value.starts_with("fixture-")
}

/// Fixture-backed backend. Construction validates and indexes the entire corpus.
#[derive(Debug, Clone)]
pub struct ReplayBackend {
    identity: ClientIdentity,
    responses: HashMap<String, SignOutcome>,
}

impl ReplayBackend {
    /// Load one `case.json`, useful for research corpora where multiple controlled cases use
    /// the same room id and therefore cannot share a server-oriented replay index.
    pub fn load_case(path: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let case = read_case(path.as_ref())?;
        Ok(Self::from_valid_case(case))
    }

    pub fn load(root: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let root = root.as_ref();
        let mut paths = Vec::new();
        collect_cases(root, &mut paths)?;
        paths.sort();
        if paths.is_empty() {
            return Err(ReplayError::EmptyCorpus(root.to_path_buf()));
        }

        let mut identity: Option<ClientIdentity> = None;
        let mut responses = HashMap::new();
        for path in paths {
            let case = read_case(&path)?;
            let case_identity = ClientIdentity::new(&case.environment.user_agent);
            if identity
                .as_ref()
                .is_some_and(|current| current != &case_identity)
            {
                return Err(ReplayError::MixedIdentity);
            }
            identity = Some(case_identity);
            let room_id = case.request.room_id.clone();
            if responses
                .insert(room_id.clone(), case.into_outcome())
                .is_some()
            {
                return Err(ReplayError::Duplicate { room_id });
            }
        }

        Ok(Self {
            identity: identity.expect("non-empty corpus has an identity"),
            responses,
        })
    }

    fn from_valid_case(case: ReplayCase) -> Self {
        let identity = ClientIdentity::new(&case.environment.user_agent);
        let room_id = case.request.room_id.clone();
        Self {
            identity,
            responses: HashMap::from([(room_id, case.into_outcome())]),
        }
    }

    pub fn len(&self) -> usize {
        self.responses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.responses.is_empty()
    }

    /// Requests represented by this corpus, in deterministic room-id order.
    pub fn requests(&self) -> Vec<TransportRequest> {
        let mut requests: Vec<_> = self
            .responses
            .keys()
            .cloned()
            .map(TransportRequest::new)
            .collect();
        requests.sort_by(|left, right| left.room_id.cmp(&right.room_id));
        requests
    }
}

impl SignerBackend for ReplayBackend {
    fn transport(&self, request: TransportRequest) -> BackendFuture<'_> {
        let response = self
            .responses
            .get(&request.room_id)
            .cloned()
            .unwrap_or_else(|| {
                SignOutcome::Transport(SignError::BackendUnavailable(format!(
                    "no replay case for room {}",
                    request.room_id
                )))
            });
        Box::pin(async move { response })
    }

    fn identity(&self) -> ClientIdentity {
        self.identity.clone()
    }
}

fn collect_cases(root: &Path, paths: &mut Vec<PathBuf>) -> Result<(), ReplayError> {
    let entries = fs::read_dir(root).map_err(|source| ReplayError::Read {
        path: root.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ReplayError::Read {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_cases(&path, paths)?;
        } else if path.file_name().is_some_and(|name| name == "case.json") {
            paths.push(path);
        }
    }
    Ok(())
}

fn read_case(path: &Path) -> Result<ReplayCase, ReplayError> {
    let raw = fs::read_to_string(path).map_err(|source| ReplayError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let case: ReplayCase = serde_json::from_str(&raw).map_err(|source| ReplayError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    case.validate_at(path)?;
    Ok(case)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/signing")
    }

    #[tokio::test]
    async fn loads_success_rejection_and_error_cases_offline() {
        let backend = ReplayBackend::load(corpus()).unwrap();
        assert_eq!(backend.len(), 9);

        let success = backend
            .transport(TransportRequest::new("7300000000000000001"))
            .await;
        let signed = success.ok().expect("success fixture");
        let decoded = FetchResult::decode(&signed.protobuf).unwrap();
        assert_eq!(decoded.cursor, "fixture-cursor");
        assert_eq!(signed.cookies.get("msToken"), Some("fixture-ms-token"));

        let rejected = backend
            .transport(TransportRequest::new("7300000000000000002"))
            .await;
        assert!(matches!(
            rejected,
            SignOutcome::Rejected(RejectReason::HttpStatus(404))
        ));

        let timeout = backend
            .transport(TransportRequest::new("7300000000000000003"))
            .await;
        assert!(matches!(
            timeout,
            SignOutcome::Transport(SignError::Timeout(15000))
        ));
    }

    #[tokio::test]
    async fn missing_case_fails_loudly() {
        let backend = ReplayBackend::load(corpus()).unwrap();
        let outcome = backend.transport(TransportRequest::new("999")).await;
        assert!(matches!(
            outcome,
            SignOutcome::Transport(SignError::BackendUnavailable(message))
                if message.contains("no replay case")
        ));
    }

    #[tokio::test]
    async fn loads_one_case_without_building_a_corpus_index() {
        let backend = ReplayBackend::load_case(corpus().join("baseline-guest/case.json")).unwrap();
        assert_eq!(backend.len(), 1);
        assert!(backend
            .transport(TransportRequest::new("7300000000000000001"))
            .await
            .is_ok());
    }

    #[test]
    fn rejects_unsupported_versions_and_unsanitized_values() {
        let mut case = read_case(&corpus().join("baseline-guest/case.json")).unwrap();
        case.fixture_version = FIXTURE_VERSION + 1;
        assert!(matches!(
            case.validate_at(Path::new("case.json")),
            Err(ReplayError::UnsupportedVersion { .. })
        ));

        let FixtureOutcome::Success { cookies, .. } = &mut case.expected else {
            panic!("success fixture")
        };
        case.fixture_version = FIXTURE_VERSION;
        cookies[0].1 = "real-secret".into();
        assert!(matches!(
            case.validate_at(Path::new("case.json")),
            Err(ReplayError::Invalid { .. })
        ));
    }
}
