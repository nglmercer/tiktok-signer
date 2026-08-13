//! Structured, research-only differential comparison for signer backends.
//!
//! Sensitive transport values are represented by SHA-256 digests and byte lengths. This
//! permits equality and localization without serializing cookies or reusable signed URLs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ttl_sign_core::{FetchResult, SignError, SignOutcome, SignerBackend, TransportRequest};

mod capture;
mod experiment;
mod signing_observation;
mod signing_trace;

#[cfg(feature = "webview")]
pub mod webview_support;

pub use capture::{
    capture_experiment_outcome, capture_outcome, write_capture, CaptureBundle, CaptureError,
    ObservationArtifact,
};
pub use experiment::{
    CookieMutation, CookieProbeValue, EnvironmentProfile, ExperimentCase, ExperimentDimension,
    ExperimentError, ExperimentPlan, ObservedExperimentCase, ObservedQueryMutation,
    ObservedSigningInput, QueryMutation, SessionMode, SigningInput, TimestampMode,
    EXPERIMENT_PLAN_VERSION,
};
pub use signing_observation::{
    compare_signed_url_observations, observe_signed_url, read_signed_url_observation,
    write_signing_observation, SignedUrlDifferentialResult, SignedUrlObservation,
    SignedUrlObservationError, SigningStage, SigningStageDifference,
};
pub use signing_trace::{
    build_signing_trace, compare_signing_traces, read_signing_trace, write_signing_trace,
    FrontierSignEvidence, ParameterOrigin, RepetitionStability, SdkEvidence, SdkFunctionEvidence,
    SdkResourceEvidence, SdkResourceStatus, SigningTrace, SigningTraceDifference,
    SigningTraceDifferentialResult, SigningTraceError, SlotStability, TraceDifferenceKind,
    SIGNING_TRACE_VERSION,
};

#[cfg(feature = "webview")]
pub use signing_trace::collect_sdk_evidence;

pub const OBSERVATION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchCase {
    pub id: String,
    pub description: String,
    pub request: TransportRequest,
    pub environment: EnvironmentInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentInput {
    pub preset: String,
    pub timestamp_mode: String,
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationMetadata {
    pub observation_version: u32,
    pub backend: String,
    pub backend_version: String,
    pub git_commit: String,
    pub git_dirty: bool,
    pub operating_system: String,
    pub runtime: String,
    pub research_timestamp_ms: u128,
    pub sanitization_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub metadata: ObservationMetadata,
    pub outcome: ObservationOutcome,
    pub user_agent: Option<String>,
    pub cookie_names: Vec<String>,
    pub transport: Option<TransportObservation>,
    pub protobuf: Option<ValueDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservationOutcome {
    Success,
    Rejected { reason: String, status: Option<u16> },
    Error { class: String },
    DecodeError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportObservation {
    /// Scheme, authority, and path only. Signed query material is always removed.
    pub endpoint: String,
    pub route_params: Vec<ObservedParameter>,
    pub cursor: ValueDigest,
    pub internal_ext: ValueDigest,
    pub heartbeat_duration: u64,
    pub need_ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedParameter {
    pub name: String,
    pub value: ValueDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueDigest {
    pub sha256: String,
    pub bytes: usize,
}

impl ValueDigest {
    pub fn of(value: impl AsRef<[u8]>) -> Self {
        let value = value.as_ref();
        let hash = Sha256::digest(value);
        Self {
            sha256: hex(&hash),
            bytes: value.len(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialResult {
    pub case_id: String,
    pub oracle: Observation,
    pub candidate: Observation,
    pub differences: Vec<Difference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentDifferentialResult {
    pub baseline_case_id: String,
    pub experiment_case_id: String,
    pub changed_dimension: ExperimentDimension,
    pub baseline: Observation,
    pub experiment: Observation,
    pub differences: Vec<Difference>,
}

impl ExperimentDifferentialResult {
    pub fn is_match(&self) -> bool {
        self.differences.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ObservationArtifactError {
    #[error("could not read observation artifact {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse observation artifact {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("observation artifact for {case_id} has no typed experiment context")]
    MissingExperiment { case_id: String },
    #[error("comparison must change exactly one dimension; changed {0:?}")]
    UncontrolledComparison(Vec<ExperimentDimension>),
}

impl DifferentialResult {
    pub fn is_match(&self) -> bool {
        self.differences.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Difference {
    MissingParameter {
        name: String,
    },
    UnexpectedParameter {
        name: String,
    },
    ParameterValue {
        name: String,
        oracle: ValueDigest,
        candidate: ValueDigest,
    },
    ParameterOrder,
    Encoding {
        field: String,
    },
    Header {
        name: String,
    },
    UserAgent,
    Timestamp,
    TransportField {
        field: String,
    },
    ProtobufField {
        field: String,
    },
    Outcome,
}

pub struct DifferentialRunner;

impl DifferentialRunner {
    pub async fn compare(
        case: &ResearchCase,
        oracle_name: &str,
        oracle: &dyn SignerBackend,
        candidate_name: &str,
        candidate: &dyn SignerBackend,
    ) -> DifferentialResult {
        let request = case.request.clone();
        let (oracle_outcome, candidate_outcome) = tokio::join!(
            oracle.transport(request.clone()),
            candidate.transport(request)
        );
        let oracle = observe_outcome(oracle_name, oracle_outcome);
        let candidate = observe_outcome(candidate_name, candidate_outcome);
        let differences = compare_observations(&oracle, &candidate);
        DifferentialResult {
            case_id: case.id.clone(),
            oracle,
            candidate,
            differences,
        }
    }
}

pub fn read_observation_artifact(
    path: impl AsRef<Path>,
) -> Result<ObservationArtifact, ObservationArtifactError> {
    let path = path.as_ref();
    let raw = std::fs::read_to_string(path).map_err(|source| ObservationArtifactError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| ObservationArtifactError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Compare a baseline capture with one controlled experiment capture.
pub fn compare_experiment_artifacts(
    baseline: &ObservationArtifact,
    experiment: &ObservationArtifact,
) -> Result<ExperimentDifferentialResult, ObservationArtifactError> {
    let baseline_input = baseline.experiment.as_ref().ok_or_else(|| {
        ObservationArtifactError::MissingExperiment {
            case_id: baseline.case.id.clone(),
        }
    })?;
    let experiment_input = experiment.experiment.as_ref().ok_or_else(|| {
        ObservationArtifactError::MissingExperiment {
            case_id: experiment.case.id.clone(),
        }
    })?;
    let changed = experiment_input.changed_dimensions_from(baseline_input);
    let [changed_dimension] = changed.as_slice() else {
        return Err(ObservationArtifactError::UncontrolledComparison(changed));
    };
    Ok(ExperimentDifferentialResult {
        baseline_case_id: baseline.case.id.clone(),
        experiment_case_id: experiment.case.id.clone(),
        changed_dimension: *changed_dimension,
        baseline: baseline.observation.clone(),
        experiment: experiment.observation.clone(),
        differences: compare_observations(&baseline.observation, &experiment.observation),
    })
}

/// Convert a backend result into structured output without serializing raw secret values.
pub fn observe_outcome(backend: &str, outcome: SignOutcome) -> Observation {
    let metadata = observation_metadata(backend);

    match outcome {
        SignOutcome::Ok(signed) => {
            let cookie_names = signed
                .cookies
                .iter()
                .map(|(name, _)| name.to_string())
                .collect();
            let protobuf = ValueDigest::of(&signed.protobuf);
            let decoded = FetchResult::decode(&signed.protobuf);
            match decoded {
                Ok(fetch) => Observation {
                    metadata,
                    outcome: ObservationOutcome::Success,
                    user_agent: Some(signed.user_agent),
                    cookie_names,
                    transport: Some(TransportObservation {
                        endpoint: strip_query(&fetch.push_server),
                        route_params: fetch
                            .route_params
                            .into_iter()
                            .map(|(name, value)| ObservedParameter {
                                name,
                                value: ValueDigest::of(value),
                            })
                            .collect(),
                        cursor: ValueDigest::of(fetch.cursor),
                        internal_ext: ValueDigest::of(fetch.internal_ext),
                        heartbeat_duration: fetch.heartbeat_duration,
                        need_ack: fetch.need_ack,
                    }),
                    protobuf: Some(protobuf),
                },
                Err(_) => Observation {
                    metadata,
                    outcome: ObservationOutcome::DecodeError,
                    user_agent: Some(signed.user_agent),
                    cookie_names,
                    transport: None,
                    protobuf: Some(protobuf),
                },
            }
        }
        SignOutcome::Rejected(reason) => {
            let (reason, status) = match reason {
                ttl_sign_core::RejectReason::EmptyBody => ("empty_body", None),
                ttl_sign_core::RejectReason::EmptyPushServer => ("empty_push_server", None),
                ttl_sign_core::RejectReason::HttpStatus(code) => ("http_status", Some(code)),
            };
            Observation {
                metadata,
                outcome: ObservationOutcome::Rejected {
                    reason: reason.into(),
                    status,
                },
                user_agent: None,
                cookie_names: Vec::new(),
                transport: None,
                protobuf: None,
            }
        }
        SignOutcome::Transport(error) => Observation {
            metadata,
            outcome: ObservationOutcome::Error {
                class: error_class(&error).into(),
            },
            user_agent: None,
            cookie_names: Vec::new(),
            transport: None,
            protobuf: None,
        },
    }
}

pub(crate) fn observation_metadata(backend: &str) -> ObservationMetadata {
    ObservationMetadata {
        observation_version: OBSERVATION_VERSION,
        backend: backend.to_string(),
        backend_version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit: env!("TTL_GIT_COMMIT").to_string(),
        git_dirty: env!("TTL_GIT_DIRTY") == "true",
        operating_system: std::env::consts::OS.to_string(),
        runtime: "rust".to_string(),
        research_timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        sanitization_version: 1,
    }
}

fn error_class(error: &SignError) -> &'static str {
    match error {
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
    }
}

fn strip_query(uri: &str) -> String {
    uri.split(['?', '#']).next().unwrap_or_default().to_string()
}

pub fn compare_observations(oracle: &Observation, candidate: &Observation) -> Vec<Difference> {
    let mut differences = Vec::new();
    if oracle.outcome != candidate.outcome {
        differences.push(Difference::Outcome);
    }
    if oracle.user_agent != candidate.user_agent {
        differences.push(Difference::UserAgent);
    }
    if oracle.cookie_names != candidate.cookie_names {
        differences.push(Difference::Header {
            name: "cookie_names".into(),
        });
    }
    match (&oracle.transport, &candidate.transport) {
        (Some(oracle), Some(candidate)) => compare_transport(oracle, candidate, &mut differences),
        (None, None) => {}
        _ => differences.push(Difference::TransportField {
            field: "presence".into(),
        }),
    }
    if oracle.protobuf != candidate.protobuf {
        differences.push(Difference::ProtobufField {
            field: "encoded_bytes".into(),
        });
    }
    differences
}

fn compare_transport(
    oracle: &TransportObservation,
    candidate: &TransportObservation,
    differences: &mut Vec<Difference>,
) {
    if oracle.endpoint != candidate.endpoint {
        differences.push(Difference::TransportField {
            field: "endpoint".into(),
        });
    }
    if oracle.cursor != candidate.cursor {
        differences.push(Difference::TransportField {
            field: "cursor".into(),
        });
    }
    if oracle.internal_ext != candidate.internal_ext {
        differences.push(Difference::TransportField {
            field: "internal_ext".into(),
        });
    }
    if oracle.heartbeat_duration != candidate.heartbeat_duration {
        differences.push(Difference::ProtobufField {
            field: "heartbeat_duration".into(),
        });
    }
    if oracle.need_ack != candidate.need_ack {
        differences.push(Difference::ProtobufField {
            field: "need_ack".into(),
        });
    }
    compare_parameters(&oracle.route_params, &candidate.route_params, differences);
}

fn compare_parameters(
    oracle: &[ObservedParameter],
    candidate: &[ObservedParameter],
    differences: &mut Vec<Difference>,
) {
    let oracle_counts = counts(oracle);
    let candidate_counts = counts(candidate);
    for (name, count) in &oracle_counts {
        for _ in candidate_counts.get(name).copied().unwrap_or_default()..*count {
            differences.push(Difference::MissingParameter { name: name.clone() });
        }
    }
    for (name, count) in &candidate_counts {
        for _ in oracle_counts.get(name).copied().unwrap_or_default()..*count {
            differences.push(Difference::UnexpectedParameter { name: name.clone() });
        }
    }

    if oracle_counts == candidate_counts
        && oracle
            .iter()
            .map(|p| &p.name)
            .ne(candidate.iter().map(|p| &p.name))
    {
        differences.push(Difference::ParameterOrder);
    }

    let mut candidate_by_name: HashMap<&str, Vec<&ValueDigest>> = HashMap::new();
    for parameter in candidate {
        candidate_by_name
            .entry(&parameter.name)
            .or_default()
            .push(&parameter.value);
    }
    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    for parameter in oracle {
        let index = occurrences.entry(&parameter.name).or_default();
        if let Some(value) = candidate_by_name
            .get(parameter.name.as_str())
            .and_then(|values| values.get(*index))
        {
            if parameter.value != **value {
                differences.push(Difference::ParameterValue {
                    name: parameter.name.clone(),
                    oracle: parameter.value.clone(),
                    candidate: (*value).clone(),
                });
            }
        }
        *index += 1;
    }
}

fn counts(parameters: &[ObservedParameter]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for parameter in parameters {
        *counts.entry(parameter.name.clone()).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::{ClientIdentity, CookieJar, MockBackend, SignedFetch};

    fn success(route_params: Vec<(String, String)>) -> SignOutcome {
        let fetch = FetchResult {
            push_server: "wss://fixture.invalid/ws/?secret=never-observed".into(),
            route_params,
            cursor: "sensitive-cursor".into(),
            internal_ext: "sensitive-internal".into(),
            heartbeat_duration: 10_000,
            need_ack: true,
        };
        SignOutcome::Ok(SignedFetch {
            protobuf: fetch.encode(),
            cookies: CookieJar::parse("msToken=do-not-serialize"),
            user_agent: "fixture-agent".into(),
            signed_url: "wss://fixture.invalid/ws/?secret=do-not-serialize".into(),
        })
    }

    fn case() -> ResearchCase {
        ResearchCase {
            id: "case-1".into(),
            description: "controlled route comparison".into(),
            request: TransportRequest::new("123"),
            environment: EnvironmentInput {
                preset: "fixture".into(),
                timestamp_mode: "fixed".into(),
                session: "guest".into(),
            },
        }
    }

    #[tokio::test]
    async fn identical_backends_match() {
        let outcome = success(vec![("wrss".into(), "same".into())]);
        let oracle = MockBackend::new(ClientIdentity::new("fixture-agent"))
            .with_response("123", outcome.clone());
        let candidate =
            MockBackend::new(ClientIdentity::new("fixture-agent")).with_response("123", outcome);
        let result =
            DifferentialRunner::compare(&case(), "oracle", &oracle, "candidate", &candidate).await;
        assert!(result.is_match(), "{:?}", result.differences);
    }

    #[tokio::test]
    async fn differences_are_localized_and_serialized_without_secrets() {
        let oracle = MockBackend::new(ClientIdentity::new("fixture-agent")).with_response(
            "123",
            success(vec![
                ("wrss".into(), "oracle-secret".into()),
                ("imprp".into(), "same".into()),
            ]),
        );
        let candidate = MockBackend::new(ClientIdentity::new("fixture-agent")).with_response(
            "123",
            success(vec![
                ("imprp".into(), "same".into()),
                ("wrss".into(), "candidate-secret".into()),
            ]),
        );
        let result =
            DifferentialRunner::compare(&case(), "oracle", &oracle, "candidate", &candidate).await;
        assert!(result.differences.contains(&Difference::ParameterOrder));
        assert!(result.differences.iter().any(|difference| matches!(
            difference,
            Difference::ParameterValue { name, .. } if name == "wrss"
        )));

        let json = serde_json::to_string(&result).unwrap();
        for secret in [
            "oracle-secret",
            "candidate-secret",
            "sensitive-cursor",
            "sensitive-internal",
            "do-not-serialize",
        ] {
            assert!(!json.contains(secret), "serialized secret: {secret}");
        }
        assert!(!json.contains("?secret="), "signed query leaked: {json}");
    }
}
