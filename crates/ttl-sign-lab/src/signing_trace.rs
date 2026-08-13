//! Repeated, sanitized measurements of one identical signing input.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    compare_parameters, Difference, ExperimentDimension, ObservedParameter, SignedUrlObservation,
    SigningStage, ValueDigest,
};

pub const SIGNING_TRACE_VERSION: u32 = 1;
#[cfg(feature = "webview")]
const MAX_SDK_RESOURCES: usize = 64;
#[cfg(feature = "webview")]
const MAX_SDK_RESOURCE_BYTES: usize = 8 * 1024 * 1024;

/// Evaluated only after the WebView readiness gate reports `byted_acrawler`.
#[cfg(feature = "webview")]
const SDK_EVIDENCE_JS: &str = r#"JSON.stringify((function(){
  var sdk=window.byted_acrawler;
  var urls=[];
  try { urls=urls.concat(Array.from(document.scripts||[]).map(function(s){return s.src||'';})); } catch(e) {}
  try { urls=urls.concat(performance.getEntriesByType('resource').map(function(e){return e.name||'';})); } catch(e) {}
  return {
    version:sdk&&sdk.version!=null?String(sdk.version):null,
    symbols:sdk?Object.keys(sdk).sort():[],
    frontier_source:sdk&&typeof sdk.frontierSign==='function'?Function.prototype.toString.call(sdk.frontierSign):null,
    resources:Array.from(new Set(urls.filter(function(u){return /^https:\/\//.test(u)&&(/\.js(?:[?#]|$)/i.test(u)||/webmssdk|acrawler|secsdk/i.test(u));})))
  };
})())"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkEvidence {
    pub version: Option<String>,
    pub symbols: Vec<String>,
    pub frontier_sign: Option<SdkFunctionEvidence>,
    /// Result of calling the SDK's public `frontierSign` on the same unsigned URL.
    pub frontier_probe: Option<FrontierSignEvidence>,
    pub resources: Vec<SdkResourceEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkFunctionEvidence {
    pub source: ValueDigest,
    pub native_code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrontierSignEvidence {
    pub parameters: Vec<ObservedParameter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkResourceEvidence {
    /// Scheme, authority, and path only; resource queries are never persisted.
    pub endpoint: String,
    pub likely_sdk: bool,
    pub status: SdkResourceStatus,
    pub body: Option<ValueDigest>,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdkResourceStatus {
    Candidate,
    Downloaded,
    RejectedHost,
    TooLarge,
    HttpError,
    NetworkError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterOrigin {
    Input,
    AddedBySdk,
    Cookie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepetitionStability {
    Stable,
    Varies,
    Intermittent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotStability {
    pub name: String,
    /// Zero-based occurrence among fields with this name; duplicates remain distinct.
    pub occurrence: usize,
    pub origin: ParameterOrigin,
    pub present_samples: usize,
    pub total_samples: usize,
    pub distinct_values: usize,
    pub byte_lengths: Vec<usize>,
    pub stability: RepetitionStability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningTrace {
    pub trace_version: u32,
    pub case_id: String,
    pub repetitions: usize,
    pub sdk: SdkEvidence,
    /// Every sample is already sanitized and contains no reusable signed URL.
    pub samples: Vec<SignedUrlObservation>,
    pub parameter_stability: Vec<SlotStability>,
    pub cookie_stability: Vec<SlotStability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDifferenceKind {
    EffectiveEnvironment,
    SdkIdentity,
    ParameterStructure,
    Stability,
    ByteLengths,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningTraceDifference {
    pub stage: SigningStage,
    pub kind: TraceDifferenceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningTraceDifferentialResult {
    pub baseline_case_id: String,
    pub experiment_case_id: String,
    pub changed_dimension: ExperimentDimension,
    pub input_differences: Vec<Difference>,
    /// Random values are ignored; structure, stability and lengths are compared.
    pub behavioral_differences: Vec<SigningTraceDifference>,
}

impl SigningTraceDifferentialResult {
    pub fn is_behaviorally_equivalent(&self) -> bool {
        self.behavioral_differences.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SigningTraceError {
    #[error("a signing trace requires at least two samples")]
    TooFewSamples,
    #[error("sample {index} does not have the same case and canonical input")]
    InputMismatch { index: usize },
    #[error("signing trace contains no samples")]
    EmptyTrace,
    #[error("comparison must change exactly one declared dimension; changed {0:?}")]
    UncontrolledComparison(Vec<ExperimentDimension>),
    #[error("could not read signing trace {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse signing trace {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("could not inspect the page SDK: {0}")]
    SdkInspection(String),
    #[error("unsafe case id for output path: {0}")]
    UnsafeCaseId(String),
    #[error("signing trace directory already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("could not serialize signing trace: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("could not write signing trace {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn read_signing_trace(path: impl AsRef<Path>) -> Result<SigningTrace, SigningTraceError> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path).map_err(|source| SigningTraceError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| SigningTraceError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

pub fn compare_signing_traces(
    baseline: &SigningTrace,
    experiment: &SigningTrace,
) -> Result<SigningTraceDifferentialResult, SigningTraceError> {
    let baseline_sample = baseline
        .samples
        .first()
        .ok_or(SigningTraceError::EmptyTrace)?;
    let experiment_sample = experiment
        .samples
        .first()
        .ok_or(SigningTraceError::EmptyTrace)?;
    let baseline_input = &baseline_sample.experiment;
    let experiment_input = &experiment_sample.experiment;
    let changed = experiment_input.changed_dimensions_from(baseline_input);
    let [changed_dimension] = changed.as_slice() else {
        return Err(SigningTraceError::UncontrolledComparison(changed));
    };
    let mut input_differences = Vec::new();
    compare_parameters(
        &baseline_sample.normalized_params,
        &experiment_sample.normalized_params,
        &mut input_differences,
    );
    let mut behavioral_differences = Vec::new();
    if baseline_sample.effective_environment != experiment_sample.effective_environment {
        behavioral_differences.push(SigningTraceDifference {
            stage: SigningStage::Environment,
            kind: TraceDifferenceKind::EffectiveEnvironment,
            name: None,
        });
    }
    if sdk_identity(&baseline.sdk) != sdk_identity(&experiment.sdk) {
        behavioral_differences.push(SigningTraceDifference {
            stage: SigningStage::SignedParameters,
            kind: TraceDifferenceKind::SdkIdentity,
            name: None,
        });
    }
    compare_trace_slots(
        SigningStage::SignedParameters,
        &baseline.parameter_stability,
        &experiment.parameter_stability,
        true,
        &mut behavioral_differences,
    );
    compare_trace_slots(
        SigningStage::Cookies,
        &baseline.cookie_stability,
        &experiment.cookie_stability,
        false,
        &mut behavioral_differences,
    );
    Ok(SigningTraceDifferentialResult {
        baseline_case_id: baseline.case_id.clone(),
        experiment_case_id: experiment.case_id.clone(),
        changed_dimension: *changed_dimension,
        input_differences,
        behavioral_differences,
    })
}

fn sdk_identity(sdk: &SdkEvidence) -> (Option<&ValueDigest>, Vec<(&str, Option<&ValueDigest>)>) {
    let mut resources: Vec<_> = sdk
        .resources
        .iter()
        .filter(|resource| resource.likely_sdk)
        .map(|resource| (resource.endpoint.as_str(), resource.body.as_ref()))
        .collect();
    resources.sort_by_key(|(endpoint, _)| *endpoint);
    (
        sdk.frontier_sign.as_ref().map(|value| &value.source),
        resources,
    )
}

fn compare_trace_slots(
    stage: SigningStage,
    baseline: &[SlotStability],
    experiment: &[SlotStability],
    compare_byte_lengths: bool,
    differences: &mut Vec<SigningTraceDifference>,
) {
    let baseline: BTreeMap<_, _> = baseline
        .iter()
        .map(|slot| ((slot.name.as_str(), slot.occurrence), slot))
        .collect();
    let experiment: BTreeMap<_, _> = experiment
        .iter()
        .map(|slot| ((slot.name.as_str(), slot.occurrence), slot))
        .collect();
    let keys: BTreeSet<_> = baseline.keys().chain(experiment.keys()).copied().collect();
    for (name, occurrence) in keys {
        let label = format!("{name}#{occurrence}");
        match (
            baseline.get(&(name, occurrence)),
            experiment.get(&(name, occurrence)),
        ) {
            (Some(baseline), Some(experiment)) => {
                if baseline.stability != experiment.stability
                    || baseline.present_samples * experiment.total_samples
                        != experiment.present_samples * baseline.total_samples
                {
                    differences.push(SigningTraceDifference {
                        stage,
                        kind: TraceDifferenceKind::Stability,
                        name: Some(label.clone()),
                    });
                }
                // Entropic signing fields can expose more than one encoded length even for
                // identical input. Overlapping support is therefore compatible evidence;
                // only disjoint length sets identify a reproducible shape difference.
                // Cookie lengths are deliberately excluded because the page rotates state
                // between the interleaved baseline and experiment calls.
                if compare_byte_lengths
                    && byte_length_sets_are_disjoint(
                        &baseline.byte_lengths,
                        &experiment.byte_lengths,
                    )
                {
                    differences.push(SigningTraceDifference {
                        stage,
                        kind: TraceDifferenceKind::ByteLengths,
                        name: Some(label),
                    });
                }
            }
            _ => differences.push(SigningTraceDifference {
                stage,
                kind: TraceDifferenceKind::ParameterStructure,
                name: Some(label),
            }),
        }
    }
}

fn byte_length_sets_are_disjoint(baseline: &[usize], experiment: &[usize]) -> bool {
    !baseline.is_empty()
        && !experiment.is_empty()
        && !baseline
            .iter()
            .any(|length| experiment.binary_search(length).is_ok())
}

pub fn build_signing_trace(
    sdk: SdkEvidence,
    samples: Vec<SignedUrlObservation>,
) -> Result<SigningTrace, SigningTraceError> {
    if samples.len() < 2 {
        return Err(SigningTraceError::TooFewSamples);
    }
    let first = &samples[0];
    for (index, sample) in samples.iter().enumerate().skip(1) {
        if sample.experiment != first.experiment
            || sample.effective_environment != first.effective_environment
            || sample.unsigned_endpoint != first.unsigned_endpoint
            || sample.normalized_params != first.normalized_params
            || sample.canonical_query != first.canonical_query
            || sample.user_agent != first.user_agent
        {
            return Err(SigningTraceError::InputMismatch { index });
        }
    }

    let input_counts = counts(&first.normalized_params);
    let parameter_stability = classify_slots(
        samples.iter().map(|sample| sample.signed_params.as_slice()),
        samples.len(),
        |name, occurrence| {
            if occurrence < input_counts.get(name).copied().unwrap_or_default() {
                ParameterOrigin::Input
            } else {
                ParameterOrigin::AddedBySdk
            }
        },
    );
    let cookie_stability = classify_slots(
        samples.iter().map(|sample| sample.cookie_values.as_slice()),
        samples.len(),
        |_, _| ParameterOrigin::Cookie,
    );

    Ok(SigningTrace {
        trace_version: SIGNING_TRACE_VERSION,
        case_id: first.experiment.id.clone(),
        repetitions: samples.len(),
        sdk,
        samples,
        parameter_stability,
        cookie_stability,
    })
}

fn counts(parameters: &[ObservedParameter]) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for parameter in parameters {
        *counts.entry(parameter.name.as_str()).or_default() += 1;
    }
    counts
}

fn classify_slots<'a>(
    samples: impl Iterator<Item = &'a [ObservedParameter]>,
    total_samples: usize,
    origin: impl Fn(&str, usize) -> ParameterOrigin,
) -> Vec<SlotStability> {
    let mut slots: BTreeMap<(String, usize), Vec<ValueDigest>> = BTreeMap::new();
    for sample in samples {
        let mut occurrences: HashMap<&str, usize> = HashMap::new();
        for parameter in sample {
            let occurrence = occurrences.entry(&parameter.name).or_default();
            slots
                .entry((parameter.name.clone(), *occurrence))
                .or_default()
                .push(parameter.value.clone());
            *occurrence += 1;
        }
    }
    slots
        .into_iter()
        .map(|((name, occurrence), values)| {
            let distinct: BTreeSet<_> = values
                .iter()
                .map(|value| (value.sha256.as_str(), value.bytes))
                .collect();
            let byte_lengths: BTreeSet<_> = values.iter().map(|value| value.bytes).collect();
            let present_samples = values.len();
            let stability = if present_samples != total_samples {
                RepetitionStability::Intermittent
            } else if distinct.len() == 1 {
                RepetitionStability::Stable
            } else {
                RepetitionStability::Varies
            };
            SlotStability {
                origin: origin(&name, occurrence),
                name,
                occurrence,
                present_samples,
                total_samples,
                distinct_values: distinct.len(),
                byte_lengths: byte_lengths.into_iter().collect(),
                stability,
            }
        })
        .collect()
}

/// Write a non-overwriting `<root>/<case-id>-signing-trace/signing-trace.json` artifact.
pub fn write_signing_trace(
    root: impl AsRef<Path>,
    trace: &SigningTrace,
) -> Result<PathBuf, SigningTraceError> {
    let case_id = &trace.case_id;
    if case_id.is_empty()
        || !case_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(SigningTraceError::UnsafeCaseId(case_id.clone()));
    }
    let root = root.as_ref();
    fs::create_dir_all(root).map_err(|source| SigningTraceError::Write {
        path: root.to_path_buf(),
        source,
    })?;
    let directory = root.join(format!("{case_id}-signing-trace"));
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(SigningTraceError::AlreadyExists(directory));
        }
        Err(source) => {
            return Err(SigningTraceError::Write {
                path: directory,
                source,
            });
        }
    }
    let path = directory.join("signing-trace.json");
    let result = (|| {
        let mut bytes = serde_json::to_vec_pretty(trace)?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| SigningTraceError::Write {
                path: path.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .map_err(|source| SigningTraceError::Write {
                path: path.clone(),
                source,
            })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result.map(|()| directory)
}

#[cfg(feature = "webview")]
#[derive(Debug, Deserialize)]
struct RawSdkEvidence {
    version: Option<String>,
    symbols: Vec<String>,
    frontier_source: Option<String>,
    resources: Vec<String>,
}

/// Identify the loaded SDK and hash likely public bundle resources without persisting source.
#[cfg(feature = "webview")]
pub async fn collect_sdk_evidence(
    signer: &ttl_sign_webview::Signer,
    unsigned_url: &str,
) -> Result<SdkEvidence, SigningTraceError> {
    let raw = signer
        .eval(SDK_EVIDENCE_JS)
        .await
        .map_err(|error| SigningTraceError::SdkInspection(error_class(&error).into()))?;
    let mut raw: RawSdkEvidence = serde_json::from_str(&raw)
        .map_err(|error| SigningTraceError::SdkInspection(format!("invalid_metadata:{error}")))?;
    raw.symbols.sort();
    raw.symbols.dedup();
    raw.resources.sort();
    raw.resources.dedup();
    // A TikTok page can load hundreds of application chunks. Preserve deterministic URL
    // order inside each group, but inspect explicit signing candidates before applying the
    // resource cap so an alphabetically earlier chunk cannot hide webmssdk/acrawler.
    raw.resources
        .sort_by_key(|resource| !likely_sdk_endpoint(&strip_query(resource)));
    raw.resources.truncate(MAX_SDK_RESOURCES);

    let client = reqwest::Client::builder()
        .user_agent(signer.preset().user_agent())
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| SigningTraceError::SdkInspection("http_client".into()))?;
    let mut scans = tokio::task::JoinSet::new();
    for resource in raw.resources {
        let client = client.clone();
        scans.spawn(async move { inspect_resource(&client, &resource).await });
    }
    let mut resources = Vec::new();
    while let Some(result) = scans.join_next().await {
        if let Ok(resource) = result {
            resources.push(resource);
        }
    }
    resources.sort_by(|left, right| left.endpoint.cmp(&right.endpoint));
    resources.dedup_by(|left, right| left.endpoint == right.endpoint);

    let url_json = serde_json::to_string(unsigned_url)
        .map_err(|_| SigningTraceError::SdkInspection("probe_url".into()))?;
    let probe_script = format!(
        "JSON.stringify(window.byted_acrawler&&typeof window.byted_acrawler.frontierSign==='function'?window.byted_acrawler.frontierSign({{url:{url_json}}}):null)"
    );
    let frontier_probe = signer
        .eval(&probe_script)
        .await
        .ok()
        .and_then(|raw| serde_json::from_str::<BTreeMap<String, String>>(&raw).ok())
        .map(|parameters| FrontierSignEvidence {
            parameters: parameters
                .into_iter()
                .map(|(name, value)| ObservedParameter {
                    name,
                    value: ValueDigest::of(value),
                })
                .collect(),
        });

    Ok(SdkEvidence {
        version: raw.version.filter(|version| !version.is_empty()),
        symbols: raw.symbols,
        frontier_sign: raw.frontier_source.map(|source| SdkFunctionEvidence {
            native_code: source.contains("[native code]"),
            source: ValueDigest::of(source),
        }),
        frontier_probe,
        resources,
    })
}

#[cfg(feature = "webview")]
async fn inspect_resource(client: &reqwest::Client, raw_url: &str) -> SdkResourceEvidence {
    let endpoint = strip_query(raw_url);
    let mut evidence = SdkResourceEvidence {
        likely_sdk: likely_sdk_endpoint(&endpoint),
        endpoint,
        status: SdkResourceStatus::Candidate,
        body: None,
        markers: Vec::new(),
    };
    let Ok(url) = reqwest::Url::parse(raw_url) else {
        evidence.status = SdkResourceStatus::RejectedHost;
        return evidence;
    };
    if url.scheme() != "https" || !allowed_sdk_host(url.host_str().unwrap_or_default()) {
        evidence.status = SdkResourceStatus::RejectedHost;
        return evidence;
    }
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(_) => {
            evidence.status = SdkResourceStatus::NetworkError;
            return evidence;
        }
    };
    if !response.status().is_success() {
        evidence.status = SdkResourceStatus::HttpError;
        return evidence;
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SDK_RESOURCE_BYTES as u64)
    {
        evidence.status = SdkResourceStatus::TooLarge;
        return evidence;
    }
    let body = match response.bytes().await {
        Ok(body) if body.len() <= MAX_SDK_RESOURCE_BYTES => body,
        Ok(_) => {
            evidence.status = SdkResourceStatus::TooLarge;
            return evidence;
        }
        Err(_) => {
            evidence.status = SdkResourceStatus::NetworkError;
            return evidence;
        }
    };
    const MARKERS: [&str; 5] = [
        "byted_acrawler",
        "frontierSign",
        "webmssdk",
        "X-Bogus",
        "X-Gnarly",
    ];
    evidence.markers = MARKERS
        .into_iter()
        .filter(|marker| {
            body.windows(marker.len())
                .any(|window| window == marker.as_bytes())
        })
        .map(str::to_string)
        .collect();
    evidence.body = Some(ValueDigest::of(&body));
    evidence.status = SdkResourceStatus::Downloaded;
    evidence
}

#[cfg(feature = "webview")]
fn likely_sdk_endpoint(endpoint: &str) -> bool {
    let lower = endpoint.to_ascii_lowercase();
    ["webmssdk", "secsdk", "acrawler"]
        .into_iter()
        .any(|marker| lower.contains(marker))
}

#[cfg(feature = "webview")]
fn allowed_sdk_host(host: &str) -> bool {
    [
        "tiktok.com",
        "tiktokcdn.com",
        "tiktokcdn-us.com",
        "ttwstatic.com",
        "byteoversea.com",
    ]
    .into_iter()
    .any(|suffix| host == suffix || host.ends_with(&format!(".{suffix}")))
}

#[cfg(feature = "webview")]
fn error_class(error: &ttl_sign_core::SignError) -> &'static str {
    use ttl_sign_core::SignError;
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

#[cfg(feature = "webview")]
fn strip_query(uri: &str) -> String {
    uri.split(['?', '#']).next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        observe_signed_url, EnvironmentProfile, ExperimentCase, QueryMutation, SigningInput,
    };
    use ttl_sign_core::{CookieJar, TransportRequest};

    fn experiment() -> ExperimentCase {
        ExperimentCase {
            id: "repeat".into(),
            description: "repeat identical signing input".into(),
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

    fn sample(signature: &str, token: &str, optional: bool) -> SignedUrlObservation {
        let case = experiment();
        let suffix = if optional { "&optional=present" } else { "" };
        observe_signed_url(
            &case,
            case.environment.clone(),
            "webview",
            "https://example.test/?room_id=1&duplicate=a&duplicate=b",
            &format!(
                "https://example.test/?room_id=1&duplicate=a&duplicate=b&X-Gnarly={signature}{suffix}"
            ),
            &CookieJar::parse(&format!("msToken={token}; stable=same")),
            "fixture-agent",
        )
        .unwrap()
    }

    #[test]
    fn classifies_stable_variable_and_intermittent_slots() {
        let trace = build_signing_trace(
            SdkEvidence {
                version: None,
                symbols: vec!["frontierSign".into()],
                frontier_sign: None,
                frontier_probe: None,
                resources: Vec::new(),
            },
            vec![sample("one", "a", true), sample("two", "b", false)],
        )
        .unwrap();
        let gnarly = trace
            .parameter_stability
            .iter()
            .find(|slot| slot.name == "X-Gnarly")
            .unwrap();
        assert_eq!(gnarly.origin, ParameterOrigin::AddedBySdk);
        assert_eq!(gnarly.stability, RepetitionStability::Varies);
        let optional = trace
            .parameter_stability
            .iter()
            .find(|slot| slot.name == "optional")
            .unwrap();
        assert_eq!(optional.stability, RepetitionStability::Intermittent);
        let room = trace
            .parameter_stability
            .iter()
            .find(|slot| slot.name == "room_id")
            .unwrap();
        assert_eq!(room.origin, ParameterOrigin::Input);
        assert_eq!(room.stability, RepetitionStability::Stable);
        assert!(trace
            .cookie_stability
            .iter()
            .any(|slot| slot.name == "msToken" && slot.stability == RepetitionStability::Varies));
        let json = serde_json::to_string(&trace).unwrap();
        for secret in ["X-Gnarly=one", "X-Gnarly=two", "msToken=a", "msToken=b"] {
            assert!(!json.contains(secret));
        }
    }

    #[test]
    fn rejects_non_identical_canonical_inputs() {
        let first = sample("one", "a", false);
        let mut second = sample("two", "b", false);
        second.canonical_query = ValueDigest::of("different");
        assert!(matches!(
            build_signing_trace(
                SdkEvidence {
                    version: None,
                    symbols: Vec::new(),
                    frontier_sign: None,
                    frontier_probe: None,
                    resources: Vec::new(),
                },
                vec![first, second]
            ),
            Err(SigningTraceError::InputMismatch { index: 1 })
        ));
    }

    #[test]
    fn trace_diff_ignores_overlapping_entropy_lengths_and_cookie_rotation() {
        let mut baseline = build_signing_trace(
            empty_sdk(),
            vec![
                sample(&"a".repeat(392), &"x".repeat(124), false),
                sample(&"b".repeat(444), &"y".repeat(172), false),
            ],
        )
        .unwrap();
        let mut experiment_case = experiment();
        experiment_case.id = "query-duplicate-room-id".into();
        experiment_case.signing.as_mut().unwrap().query_mutation = Some(QueryMutation::Duplicate {
            name: "room_id".into(),
            occurrence: 0,
        });
        let experiment_url = "https://example.test/?room_id=1&room_id=1&duplicate=a&duplicate=b";
        let experiment_samples = [
            (&"c".repeat(444), &"z".repeat(172)),
            (&"d".repeat(444), &"w".repeat(200)),
        ]
        .into_iter()
        .map(|(signature, token)| {
            observe_signed_url(
                &experiment_case,
                experiment_case.environment.clone(),
                "webview",
                experiment_url,
                &format!("{experiment_url}&X-Gnarly={signature}"),
                &CookieJar::parse(&format!("msToken={token}; stable=same")),
                "fixture-agent",
            )
            .unwrap()
        })
        .collect();
        let mut experiment = build_signing_trace(empty_sdk(), experiment_samples).unwrap();

        // Model the observed X-Dynosaur shape: baseline {392, 444}, experiment {444}.
        baseline.parameter_stability.push(slot(
            "X-Dynosaur",
            ParameterOrigin::AddedBySdk,
            vec![392, 444],
        ));
        experiment.parameter_stability.push(slot(
            "X-Dynosaur",
            ParameterOrigin::AddedBySdk,
            vec![444],
        ));

        let result = compare_signing_traces(&baseline, &experiment).unwrap();
        assert!(result.behavioral_differences.iter().any(|difference| {
            difference.kind == TraceDifferenceKind::ParameterStructure
                && difference.name.as_deref() == Some("room_id#1")
        }));
        assert!(!result.behavioral_differences.iter().any(|difference| {
            difference.kind == TraceDifferenceKind::ByteLengths
                && matches!(
                    difference.name.as_deref(),
                    Some("X-Dynosaur#0" | "msToken#0")
                )
        }));
    }

    #[test]
    fn trace_diff_reports_disjoint_signed_parameter_lengths() {
        let mut differences = Vec::new();
        compare_trace_slots(
            SigningStage::SignedParameters,
            &[slot("X-Gnarly", ParameterOrigin::AddedBySdk, vec![332])],
            &[slot("X-Gnarly", ParameterOrigin::AddedBySdk, vec![344])],
            true,
            &mut differences,
        );
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, TraceDifferenceKind::ByteLengths);
    }

    #[test]
    fn committed_profile_preserves_confirmed_webmssdk_boundary() {
        let profile: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/research/webmssdk-profile-2026-08-13.json"
        ))
        .unwrap();
        assert_eq!(profile["profile_version"], 1);
        assert_eq!(
            profile["bundle"]["sha256"],
            "dee22566273d398e074df6db40f39cfd827f6b8efd6fc382de03c44c501299ac"
        );
        assert_eq!(
            profile["vm"]["entry_points"]["frontierSign"]["offset"],
            69021
        );
        assert_eq!(profile["vm"]["opcode_catalog"]["handler_slots"], 355);
        assert_eq!(
            profile["vm"]["opcode_catalog"]["operand_helper_widths"],
            serde_json::json!([0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 16, 18, 20])
        );
        assert_eq!(
            profile["vm"]["opcode_catalog"]["trace_schema_version"],
            serde_json::json!(2)
        );
        assert_eq!(
            profile["vm"]["route_frame_map"]["msToken"]["call_edges"],
            serde_json::json!(["56>91717", "91717>92825"])
        );
        assert_eq!(
            profile["vm"]["route_frame_map"]["frontier_X-Bogus"]["step_counts"]["69171"],
            serde_json::json!(9)
        );
        assert_eq!(
            profile["vm"]["route_frame_map"]["X-Dynosaur"]["input_shape"]["value_class"],
            serde_json::json!("typed_array")
        );
        assert_eq!(
            profile["vm"]["route_frame_map"]["X-Gnarly"]["step_counts"]["48886"],
            serde_json::json!(383)
        );
        assert_eq!(
            profile["fetch_patch"]["added_parameters_in_order"],
            serde_json::json!(["X-Dynosaur", "msToken", "X-Bogus", "X-Gnarly"])
        );
        assert_eq!(
            profile["frontier_sign_probe"]["parameters"][0]["name"],
            "X-Bogus"
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_msToken"]["entries"],
            serde_json::json!([8039, 92825])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_msToken"]["observed_byte_lengths"],
            serde_json::json!([124, 132])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_X-Dynosaur"]["entries"],
            serde_json::json!([55188])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_X-Dynosaur"]["observed_byte_lengths"],
            serde_json::json!([388, 392, 444])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_X-Gnarly"]["entries"],
            serde_json::json!([48886])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_X-Gnarly"]["observed_byte_lengths"],
            serde_json::json!([332])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_x_bogus"]["entries"],
            serde_json::json!([58628])
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_x_bogus"]["value_class"],
            serde_json::json!("literal_one")
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_assembly"]["entry"],
            serde_json::json!(58628)
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_assembly"]["field_key_slots"],
            serde_json::json!({
                "X-Dynosaur": 656,
                "msToken": 657,
                "X-Bogus": 658,
                "X-Gnarly": 660
            })
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_assembly"]["marker_slot"]["slot"],
            serde_json::json!(659)
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_assembly"]["marker_slot"]["value_class"],
            serde_json::json!("literal_one")
        );
        assert_eq!(
            profile["subroute_candidates"]["fetch_assembly"]["observed_cases"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        assert_eq!(
            profile["controlled_probe_observations"]["timestamp_fixed"]["observed_clock_ms"],
            serde_json::json!([1_700_000_000_000u64])
        );
        assert_eq!(
            profile["controlled_probe_observations"]["cookie_msToken_alpha"]
                ["changed_candidate_entries"],
            serde_json::json!([8039, 92825])
        );
        assert_eq!(
            profile["controlled_probe_observations"]["cookie_msToken_alpha"]["probe_lengths"]
                ["msToken"],
            serde_json::json!([132])
        );
        assert_eq!(
            profile["trace_shape_observations"]["cookie_msToken_alpha"]["msToken_result_bytes"],
            serde_json::json!(132)
        );
        assert_eq!(
            profile["trace_shape_observations"]["query_duplicate_room_id"]["X-Gnarly_input_bytes"],
            serde_json::json!(1302)
        );
        assert_eq!(
            profile["trace_shape_observations"]["timezone_lima"]["effective_tz_name"],
            serde_json::json!("America/Lima")
        );
    }

    fn empty_sdk() -> SdkEvidence {
        SdkEvidence {
            version: None,
            symbols: Vec::new(),
            frontier_sign: None,
            frontier_probe: None,
            resources: Vec::new(),
        }
    }

    fn slot(name: &str, origin: ParameterOrigin, byte_lengths: Vec<usize>) -> SlotStability {
        SlotStability {
            name: name.into(),
            occurrence: 0,
            origin,
            present_samples: 2,
            total_samples: 2,
            distinct_values: 2,
            byte_lengths,
            stability: RepetitionStability::Varies,
        }
    }
}
