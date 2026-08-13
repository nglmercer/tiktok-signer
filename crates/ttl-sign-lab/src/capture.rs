//! Safe conversion of a live backend outcome into durable research artifacts.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ttl_sign_core::{FetchResult, RejectReason, SignError, SignOutcome};
use ttl_sign_replay::{
    FixtureEnvironment, FixtureOutcome, FixtureRejectReason, FixtureRequest, FixtureSignError,
    ReplayCase, ReplayError, TransportFixture, FIXTURE_VERSION,
};

use crate::{observe_outcome, ExperimentCase, Observation, ObservedExperimentCase, ResearchCase};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureBundle {
    pub case: ResearchCase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experiment: Option<ObservedExperimentCase>,
    pub observation: Observation,
    pub replay: ReplayCase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationArtifact {
    pub case: ResearchCase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experiment: Option<ObservedExperimentCase>,
    pub observation: Observation,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("could not decode successful transport: {0}")]
    Decode(String),
    #[error("generated replay fixture is invalid: {0}")]
    InvalidReplay(#[from] ReplayError),
    #[error("unsafe case id for output path: {0}")]
    UnsafeCaseId(String),
    #[error("capture directory already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("could not write capture artifact {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not serialize capture artifact: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Produce a safe observation and a deterministic replay fixture from one backend outcome.
///
/// The input is never serialized directly. URLs lose their query, cookie values and signed
/// transport values become stable `fixture-*` placeholders, and error text is classified.
pub fn capture_outcome(
    case: &ResearchCase,
    backend: &str,
    outcome: SignOutcome,
) -> Result<CaptureBundle, CaptureError> {
    capture_with_experiment(case, None, backend, outcome)
}

/// Capture a typed controlled experiment, preserving its explicit input profile.
pub fn capture_experiment_outcome(
    experiment: &ExperimentCase,
    backend: &str,
    outcome: SignOutcome,
) -> Result<CaptureBundle, CaptureError> {
    let case = experiment.research_case();
    capture_with_experiment(&case, Some(experiment.observed_input()), backend, outcome)
}

fn capture_with_experiment(
    case: &ResearchCase,
    experiment: Option<ObservedExperimentCase>,
    backend: &str,
    outcome: SignOutcome,
) -> Result<CaptureBundle, CaptureError> {
    let observation = observe_outcome(backend, outcome.clone());
    let expected = sanitize_outcome(outcome)?;
    let replay = ReplayCase {
        fixture_version: FIXTURE_VERSION,
        case_id: case.id.clone(),
        description: case.description.clone(),
        request: FixtureRequest {
            room_id: case.request.room_id.clone(),
        },
        environment: FixtureEnvironment {
            preset: case.environment.preset.clone(),
            user_agent: observation
                .user_agent
                .clone()
                .unwrap_or_else(|| "fixture-unavailable-user-agent".into()),
            session: if case.environment.session == "guest" {
                "guest".into()
            } else {
                "sanitized".into()
            },
        },
        expected,
    };
    replay.validate()?;
    Ok(CaptureBundle {
        case: case.clone(),
        experiment,
        observation,
        replay,
    })
}

fn sanitize_outcome(outcome: SignOutcome) -> Result<FixtureOutcome, CaptureError> {
    match outcome {
        SignOutcome::Ok(signed) => {
            let fetch = FetchResult::decode(&signed.protobuf)
                .map_err(|error| CaptureError::Decode(error.to_string()))?;
            let mut placeholders = Placeholders::default();
            let route_params = fetch
                .route_params
                .into_iter()
                .map(|(name, value)| {
                    let value = placeholders.value("route", &value);
                    (name, value)
                })
                .collect();
            let cookies = signed
                .cookies
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(name, value)| (name.to_string(), placeholders.value("cookie", value)))
                .collect();
            Ok(FixtureOutcome::Success {
                transport: TransportFixture {
                    push_server: strip_query(&fetch.push_server),
                    route_params,
                    cursor: placeholders.value("cursor", &fetch.cursor),
                    internal_ext: placeholders.value("internal-ext", &fetch.internal_ext),
                    heartbeat_duration: fetch.heartbeat_duration,
                    need_ack: fetch.need_ack,
                },
                cookies,
            })
        }
        SignOutcome::Rejected(reason) => Ok(FixtureOutcome::Rejected {
            reason: match reason {
                RejectReason::EmptyBody => FixtureRejectReason::EmptyBody,
                RejectReason::EmptyPushServer => FixtureRejectReason::EmptyPushServer,
                RejectReason::HttpStatus(code) => FixtureRejectReason::HttpStatus { code },
            },
        }),
        SignOutcome::Transport(error) => Ok(FixtureOutcome::Error {
            error: match error {
                SignError::SdkNotReady => FixtureSignError::SdkNotReady,
                SignError::NoInstanceAvailable => FixtureSignError::NoInstanceAvailable,
                SignError::BackendUnavailable(_) => FixtureSignError::BackendUnavailable {
                    message: "fixture-backend-unavailable".into(),
                },
                SignError::Bridge(_) => FixtureSignError::Bridge {
                    message: "fixture-bridge-error".into(),
                },
                SignError::EngineGone(_) => FixtureSignError::EngineGone {
                    message: "fixture-engine-gone".into(),
                },
                SignError::LoginTimeout(seconds) => FixtureSignError::LoginTimeout { seconds },
                SignError::Timeout(milliseconds) => FixtureSignError::Timeout { milliseconds },
                SignError::Transport(_) => FixtureSignError::Transport {
                    message: "fixture-transport-error".into(),
                },
                SignError::Decode(_) => FixtureSignError::Decode {
                    message: "fixture-decode-error".into(),
                },
                SignError::Refused(refusal) => FixtureSignError::Refused {
                    status_code: refusal.status_code,
                    message: "fixture-upstream-refusal".into(),
                },
            },
        }),
    }
}

fn strip_query(uri: &str) -> String {
    uri.split(['?', '#']).next().unwrap_or_default().to_string()
}

#[derive(Debug, Default)]
struct Placeholders {
    next: usize,
    by_value: HashMap<String, String>,
}

impl Placeholders {
    fn value(&mut self, kind: &str, value: &str) -> String {
        if value.is_empty() {
            return String::new();
        }
        if let Some(existing) = self.by_value.get(value) {
            return existing.clone();
        }
        self.next += 1;
        let placeholder = format!("fixture-{kind}-{:04}", self.next);
        self.by_value.insert(value.to_string(), placeholder.clone());
        placeholder
    }
}

/// Write `<root>/<case-id>/{case.json,observation.json}` without overwriting prior research.
pub fn write_capture(
    root: impl AsRef<Path>,
    bundle: &CaptureBundle,
) -> Result<PathBuf, CaptureError> {
    let case_id = &bundle.replay.case_id;
    if case_id.is_empty()
        || !case_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(CaptureError::UnsafeCaseId(case_id.clone()));
    }

    let root = root.as_ref();
    fs::create_dir_all(root).map_err(|source| CaptureError::Write {
        path: root.to_path_buf(),
        source,
    })?;
    let directory = root.join(case_id);
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CaptureError::AlreadyExists(directory));
        }
        Err(source) => {
            return Err(CaptureError::Write {
                path: directory,
                source,
            });
        }
    }

    let result = (|| {
        write_json(&directory.join("case.json"), &bundle.replay)?;
        write_json(
            &directory.join("observation.json"),
            &ObservationArtifact {
                case: bundle.case.clone(),
                experiment: bundle.experiment.clone(),
                observation: bundle.observation.clone(),
            },
        )?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result.map(|()| directory)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), CaptureError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| CaptureError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&bytes)
        .map_err(|source| CaptureError::Write {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::{CookieJar, SignedFetch, TransportRequest};

    fn research_case() -> ResearchCase {
        ResearchCase {
            id: "capture-1".into(),
            description: "safe capture".into(),
            request: TransportRequest::new("7300000000000000001"),
            environment: crate::EnvironmentInput {
                preset: "linux/US".into(),
                timestamp_mode: "system".into(),
                session: "configured".into(),
            },
        }
    }

    fn success() -> SignOutcome {
        let fetch = FetchResult {
            push_server: "wss://fixture.invalid/ws/?X-Gnarly=secret-signature".into(),
            route_params: vec![
                ("wrss".into(), "secret-route".into()),
                ("duplicate".into(), "secret-route".into()),
                ("empty".into(), String::new()),
            ],
            cursor: "secret-cursor".into(),
            internal_ext: "secret-internal".into(),
            heartbeat_duration: 10_000,
            need_ack: true,
        };
        SignOutcome::Ok(SignedFetch {
            protobuf: fetch.encode(),
            cookies: CookieJar::parse("sessionid=secret-session; msToken=secret-token"),
            user_agent: "fixture-agent".into(),
            signed_url: "wss://fixture.invalid/ws/?X-Gnarly=secret-signature".into(),
        })
    }

    #[test]
    fn capture_never_serializes_raw_sensitive_values() {
        let bundle = capture_outcome(&research_case(), "webview", success()).unwrap();
        let json = serde_json::to_string(&bundle).unwrap();
        for secret in [
            "secret-signature",
            "secret-route",
            "secret-cursor",
            "secret-internal",
            "secret-session",
            "secret-token",
        ] {
            assert!(!json.contains(secret), "capture leaked {secret}");
        }
        assert_eq!(bundle.replay.environment.session, "sanitized");
        let FixtureOutcome::Success { transport, .. } = &bundle.replay.expected else {
            panic!("success fixture")
        };
        assert_eq!(transport.push_server, "wss://fixture.invalid/ws/");
        assert_eq!(transport.route_params[0].1, transport.route_params[1].1);
        assert!(transport.route_params[2].1.is_empty());
    }

    #[test]
    fn captures_every_error_class_without_raw_messages() {
        let errors = [
            SignError::SdkNotReady,
            SignError::NoInstanceAvailable,
            SignError::BackendUnavailable("secret".into()),
            SignError::Bridge("secret".into()),
            SignError::EngineGone("secret".into()),
            SignError::LoginTimeout(10),
            SignError::Timeout(20),
            SignError::Transport("secret".into()),
            SignError::Decode("secret".into()),
            SignError::Refused(ttl_sign_core::WebcastRefusal {
                status_code: 1001,
                message: "secret".into(),
            }),
        ];
        for error in errors {
            let bundle =
                capture_outcome(&research_case(), "webview", SignOutcome::Transport(error))
                    .unwrap();
            let json = serde_json::to_string(&bundle).unwrap();
            assert!(!json.contains("secret"));
            bundle.replay.validate().unwrap();
        }
    }

    #[test]
    fn writer_is_non_overwriting_and_rejects_path_traversal() {
        let root = std::env::temp_dir().join(format!(
            "ttl-sign-capture-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        let _ = fs::remove_dir_all(&root);
        let bundle = capture_outcome(&research_case(), "webview", success()).unwrap();
        let directory = write_capture(&root, &bundle).unwrap();
        assert!(directory.join("case.json").is_file());
        assert!(directory.join("observation.json").is_file());
        assert!(matches!(
            write_capture(&root, &bundle),
            Err(CaptureError::AlreadyExists(_))
        ));

        let mut unsafe_bundle = bundle;
        unsafe_bundle.replay.case_id = "../escape".into();
        assert!(matches!(
            write_capture(&root, &unsafe_bundle),
            Err(CaptureError::UnsafeCaseId(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn typed_captures_can_be_compared_as_one_variable_experiments() {
        let baseline = ExperimentCase {
            id: "baseline".into(),
            description: "baseline".into(),
            request: TransportRequest::new("7300000000000000001"),
            signing: Some(crate::SigningInput {
                device_id: "7123456789012345678".into(),
                cursor: "raw-cursor-input".into(),
                internal_ext: "raw-internal-input".into(),
                contact_us: "private@example.test".into(),
                sup_ws_ds_opt: 1,
                query_mutation: None,
                cookie_mutation: None,
            }),
            environment: crate::EnvironmentProfile::chrome_linux_guest(),
        };
        let mut experiment = baseline.clone();
        experiment.id = "timezone-lima".into();
        experiment.environment.tz_name = "America/Lima".into();

        let baseline = capture_experiment_outcome(&baseline, "webview", success()).unwrap();
        let experiment = capture_experiment_outcome(&experiment, "webview", success()).unwrap();
        let serialized = serde_json::to_string(&baseline).unwrap();
        for sensitive in [
            "7123456789012345678",
            "raw-cursor-input",
            "raw-internal-input",
            "private@example.test",
        ] {
            assert!(!serialized.contains(sensitive));
        }
        let result = crate::compare_experiment_artifacts(
            &ObservationArtifact {
                case: baseline.case,
                experiment: baseline.experiment,
                observation: baseline.observation,
            },
            &ObservationArtifact {
                case: experiment.case,
                experiment: experiment.experiment,
                observation: experiment.observation,
            },
        )
        .unwrap();
        assert_eq!(
            result.changed_dimension,
            crate::ExperimentDimension::Timezone
        );
        assert!(result.is_match());
    }
}
