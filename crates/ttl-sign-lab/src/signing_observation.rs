//! Sanitized observation of the WebView SDK's URL-signing transformation.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ttl_sign_core::{CookieJar, Query};

use crate::{
    compare_parameters, observation_metadata, Difference, EnvironmentProfile, ExperimentCase,
    ExperimentDimension, ObservationMetadata, ObservedExperimentCase, ObservedParameter,
    ValueDigest,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedUrlObservation {
    pub experiment: ObservedExperimentCase,
    /// Profile in effect after the page reported its actual locale/timezone/screen.
    pub effective_environment: EnvironmentProfile,
    pub metadata: ObservationMetadata,
    /// Input URL scheme/authority/path only.
    pub unsigned_endpoint: String,
    /// Decoded, ordered input query with values represented by digests.
    pub normalized_params: Vec<ObservedParameter>,
    /// Digest of the exact encoded input query.
    pub canonical_query: ValueDigest,
    /// Decoded, ordered SDK output query with values represented by digests.
    pub signed_params: Vec<ObservedParameter>,
    /// Digest of the exact encoded SDK output query.
    pub signed_query: ValueDigest,
    /// SDK output URL scheme/authority/path only.
    pub signed_endpoint: String,
    /// Parameter names introduced by the SDK, preserving output order and duplicates.
    pub added_parameters: Vec<String>,
    pub cookie_names: Vec<String>,
    /// Cookie values represented individually as digests for dependency analysis.
    pub cookie_values: Vec<ObservedParameter>,
    pub cookie_header: ValueDigest,
    pub user_agent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningStage {
    Environment,
    Endpoint,
    NormalizedParameters,
    CanonicalQuery,
    SignedParameters,
    SignedQuery,
    AddedParameters,
    Cookies,
    UserAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningStageDifference {
    pub stage: SigningStage,
    pub difference: Difference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedUrlDifferentialResult {
    pub baseline_case_id: String,
    pub experiment_case_id: String,
    pub changed_dimension: ExperimentDimension,
    pub baseline: SignedUrlObservation,
    pub experiment: SignedUrlObservation,
    pub differences: Vec<SigningStageDifference>,
}

impl SignedUrlDifferentialResult {
    pub fn is_match(&self) -> bool {
        self.differences.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignedUrlObservationError {
    #[error("unsigned URL does not contain a non-empty query")]
    MissingUnsignedQuery,
    #[error("signed URL does not contain a non-empty query")]
    MissingSignedQuery,
    #[error("unsafe case id for output path: {0}")]
    UnsafeCaseId(String),
    #[error("signing observation directory already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("could not read signing observation {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse signing observation {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("comparison must change exactly one declared dimension; changed {0:?}")]
    UncontrolledComparison(Vec<ExperimentDimension>),
    #[error("could not serialize signing observation: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("could not write signing observation {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn read_signed_url_observation(
    path: impl AsRef<Path>,
) -> Result<SignedUrlObservation, SignedUrlObservationError> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path).map_err(|source| SignedUrlObservationError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| SignedUrlObservationError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Compare two observations whose declared inputs differ along exactly one dimension.
pub fn compare_signed_url_observations(
    baseline: &SignedUrlObservation,
    experiment: &SignedUrlObservation,
) -> Result<SignedUrlDifferentialResult, SignedUrlObservationError> {
    let changed = experiment
        .experiment
        .changed_dimensions_from(&baseline.experiment);
    let [changed_dimension] = changed.as_slice() else {
        return Err(SignedUrlObservationError::UncontrolledComparison(changed));
    };
    let mut differences = Vec::new();

    if baseline.unsigned_endpoint != experiment.unsigned_endpoint
        || baseline.signed_endpoint != experiment.signed_endpoint
    {
        push_difference(
            &mut differences,
            SigningStage::Endpoint,
            Difference::TransportField {
                field: "endpoint".into(),
            },
        );
    }
    if baseline.effective_environment != experiment.effective_environment {
        push_difference(
            &mut differences,
            SigningStage::Environment,
            Difference::TransportField {
                field: "effective_environment".into(),
            },
        );
    }
    compare_parameter_stage(
        SigningStage::NormalizedParameters,
        &baseline.normalized_params,
        &experiment.normalized_params,
        &mut differences,
    );
    if baseline.canonical_query != experiment.canonical_query {
        push_difference(
            &mut differences,
            SigningStage::CanonicalQuery,
            Difference::Encoding {
                field: "encoded_query".into(),
            },
        );
    }
    compare_parameter_stage(
        SigningStage::SignedParameters,
        &baseline.signed_params,
        &experiment.signed_params,
        &mut differences,
    );
    if baseline.signed_query != experiment.signed_query {
        push_difference(
            &mut differences,
            SigningStage::SignedQuery,
            Difference::Encoding {
                field: "encoded_query".into(),
            },
        );
    }
    if baseline.added_parameters != experiment.added_parameters {
        push_difference(
            &mut differences,
            SigningStage::AddedParameters,
            Difference::TransportField {
                field: "added_parameters".into(),
            },
        );
    }
    if baseline.cookie_names != experiment.cookie_names {
        push_difference(
            &mut differences,
            SigningStage::Cookies,
            Difference::Header {
                name: "cookie_names".into(),
            },
        );
    }
    compare_parameter_stage(
        SigningStage::Cookies,
        &baseline.cookie_values,
        &experiment.cookie_values,
        &mut differences,
    );
    if baseline.cookie_header != experiment.cookie_header {
        push_difference(
            &mut differences,
            SigningStage::Cookies,
            Difference::Header {
                name: "cookie".into(),
            },
        );
    }
    if baseline.user_agent != experiment.user_agent {
        push_difference(
            &mut differences,
            SigningStage::UserAgent,
            Difference::UserAgent,
        );
    }

    Ok(SignedUrlDifferentialResult {
        baseline_case_id: baseline.experiment.id.clone(),
        experiment_case_id: experiment.experiment.id.clone(),
        changed_dimension: *changed_dimension,
        baseline: baseline.clone(),
        experiment: experiment.clone(),
        differences,
    })
}

fn compare_parameter_stage(
    stage: SigningStage,
    baseline: &[ObservedParameter],
    experiment: &[ObservedParameter],
    differences: &mut Vec<SigningStageDifference>,
) {
    let mut parameter_differences = Vec::new();
    compare_parameters(baseline, experiment, &mut parameter_differences);
    differences.extend(
        parameter_differences
            .into_iter()
            .map(|difference| SigningStageDifference { stage, difference }),
    );
}

fn push_difference(
    differences: &mut Vec<SigningStageDifference>,
    stage: SigningStage,
    difference: Difference,
) {
    differences.push(SigningStageDifference { stage, difference });
}

pub fn observe_signed_url(
    experiment: &ExperimentCase,
    effective_environment: EnvironmentProfile,
    backend: &str,
    unsigned_url: &str,
    signed_url: &str,
    cookies: &CookieJar,
    user_agent: &str,
) -> Result<SignedUrlObservation, SignedUrlObservationError> {
    let (endpoint, unsigned_query) = split_query(unsigned_url)
        .filter(|(_, query)| !query.is_empty())
        .ok_or(SignedUrlObservationError::MissingUnsignedQuery)?;
    let (signed_endpoint, signed_query) = split_query(signed_url)
        .filter(|(_, query)| !query.is_empty())
        .ok_or(SignedUrlObservationError::MissingSignedQuery)?;
    let normalized = Query::parse_encoded(unsigned_query);
    let signed = Query::parse_encoded(signed_query);
    let added_parameters = added_parameter_names(&normalized, &signed);
    let normalized_params = observed_parameters(&normalized);
    let signed_params = observed_parameters(&signed);
    let cookie_header = cookies.to_cookie_string();
    let cookie_values = cookies
        .iter()
        .map(|(name, value)| ObservedParameter {
            name: name.to_string(),
            value: ValueDigest::of(value),
        })
        .collect();

    Ok(SignedUrlObservation {
        experiment: experiment.observed_input(),
        effective_environment,
        metadata: observation_metadata(backend),
        unsigned_endpoint: endpoint.to_string(),
        normalized_params,
        canonical_query: ValueDigest::of(unsigned_query),
        signed_params,
        signed_query: ValueDigest::of(signed_query),
        signed_endpoint: signed_endpoint.to_string(),
        added_parameters,
        cookie_names: cookies.iter().map(|(name, _)| name.to_string()).collect(),
        cookie_values,
        cookie_header: ValueDigest::of(cookie_header),
        user_agent: user_agent.to_string(),
    })
}

fn split_query(url: &str) -> Option<(&str, &str)> {
    let (endpoint, rest) = url.split_once('?')?;
    Some((endpoint, rest.split('#').next().unwrap_or_default()))
}

fn observed_parameters(query: &Query) -> Vec<ObservedParameter> {
    query
        .iter()
        .map(|(name, value)| ObservedParameter {
            name: name.to_string(),
            value: ValueDigest::of(value),
        })
        .collect()
}

fn added_parameter_names(input: &Query, output: &Query) -> Vec<String> {
    let mut remaining: HashMap<&str, usize> = HashMap::new();
    for (name, _) in input.iter() {
        *remaining.entry(name).or_default() += 1;
    }
    let mut added = Vec::new();
    for (name, _) in output.iter() {
        match remaining.get_mut(name) {
            Some(count) if *count > 0 => *count -= 1,
            _ => added.push(name.to_string()),
        }
    }
    added
}

/// Write a non-overwriting `<root>/<case-id>-signing/signing-observation.json` artifact.
pub fn write_signing_observation(
    root: impl AsRef<Path>,
    observation: &SignedUrlObservation,
) -> Result<PathBuf, SignedUrlObservationError> {
    let case_id = &observation.experiment.id;
    if case_id.is_empty()
        || !case_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(SignedUrlObservationError::UnsafeCaseId(case_id.clone()));
    }
    let root = root.as_ref();
    fs::create_dir_all(root).map_err(|source| SignedUrlObservationError::Write {
        path: root.to_path_buf(),
        source,
    })?;
    let directory = root.join(format!("{case_id}-signing"));
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(SignedUrlObservationError::AlreadyExists(directory));
        }
        Err(source) => {
            return Err(SignedUrlObservationError::Write {
                path: directory,
                source,
            });
        }
    }

    let path = directory.join("signing-observation.json");
    let result = (|| {
        let mut bytes = serde_json::to_vec_pretty(observation)?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| SignedUrlObservationError::Write {
                path: path.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .map_err(|source| SignedUrlObservationError::Write {
                path: path.clone(),
                source,
            })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result.map(|()| directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EnvironmentProfile;
    use ttl_sign_core::TransportRequest;

    fn experiment() -> ExperimentCase {
        ExperimentCase {
            id: "signed-url".into(),
            description: "observe signing".into(),
            request: TransportRequest::new("7300000000000000001"),
            signing: Some(crate::SigningInput {
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
    fn observes_added_fields_without_serializing_values() {
        let mut declared = experiment();
        let signing = declared.signing.as_mut().unwrap();
        signing.cursor = "raw-declared-cursor".into();
        signing.internal_ext = "raw-declared-internal".into();
        signing.contact_us = "private@example.test".into();
        let unsigned =
            "https://example.test/fetch/?room_id=1&encoded=a%2Fb&duplicate=1&duplicate=2";
        let signed = "https://example.test/fetch/?room_id=1&encoded=a%2Fb&duplicate=1&duplicate=2&X-Bogus=raw-secret&msToken=raw-token";
        let cookies = CookieJar::parse("msToken=raw-token; sessionid=raw-session");
        let observation = observe_signed_url(
            &declared,
            declared.environment.clone(),
            "webview",
            unsigned,
            signed,
            &cookies,
            "fixture-agent",
        )
        .unwrap();
        assert_eq!(observation.added_parameters, vec!["X-Bogus", "msToken"]);
        assert_eq!(observation.normalized_params[1].value.bytes, 3);

        let json = serde_json::to_string(&observation).unwrap();
        for secret in [
            "raw-secret",
            "raw-token",
            "raw-session",
            "a/b",
            "7123456789012345678",
            "raw-declared-cursor",
            "raw-declared-internal",
            "private@example.test",
        ] {
            assert!(
                !json.contains(secret),
                "signing observation leaked {secret}"
            );
        }
        assert!(!json.contains("?room_id="));
    }

    #[test]
    fn output_is_non_overwriting() {
        let declared = experiment();
        let observation = observe_signed_url(
            &declared,
            declared.environment.clone(),
            "webview",
            "https://example.test/?room_id=1",
            "https://example.test/?room_id=1&X-Bogus=secret",
            &CookieJar::new(),
            "fixture-agent",
        )
        .unwrap();
        let root =
            std::env::temp_dir().join(format!("ttl-sign-url-observation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_signing_observation(&root, &observation).unwrap();
        assert!(matches!(
            write_signing_observation(&root, &observation),
            Err(SignedUrlObservationError::AlreadyExists(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn localizes_a_controlled_change_by_signing_stage() {
        let baseline_case = experiment();
        let mut changed_case = baseline_case.clone();
        changed_case.id = "timezone-lima".into();
        changed_case.environment.tz_name = "America/Lima".into();
        let baseline = observe_signed_url(
            &baseline_case,
            baseline_case.environment.clone(),
            "webview",
            "https://example.test/?tz_name=America%2FNew_York",
            "https://example.test/?tz_name=America%2FNew_York&X-Bogus=one",
            &CookieJar::new(),
            "fixture-agent",
        )
        .unwrap();
        let changed = observe_signed_url(
            &changed_case,
            baseline_case.environment.clone(),
            "webview",
            "https://example.test/?tz_name=America%2FLima",
            "https://example.test/?tz_name=America%2FLima&X-Bogus=two",
            &CookieJar::new(),
            "fixture-agent",
        )
        .unwrap();

        let result = compare_signed_url_observations(&baseline, &changed).unwrap();
        assert_eq!(result.changed_dimension, ExperimentDimension::Timezone);
        assert!(result.differences.iter().any(|difference| {
            difference.stage == SigningStage::NormalizedParameters
                && matches!(
                    &difference.difference,
                    Difference::ParameterValue { name, .. } if name == "tz_name"
                )
        }));
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.stage == SigningStage::SignedParameters));
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("X-Bogus=one"));
        assert!(!json.contains("X-Bogus=two"));
    }
}
