//! Typed, controlled research plans.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ttl_sign_core::{
    params::FETCH_ENDPOINT, CookieJar, DevicePreset, FetchParams, LocationPreset, Preset, Query,
    ScreenPreset, TransportRequest,
};

use crate::{EnvironmentInput, ResearchCase, ValueDigest};

pub const EXPERIMENT_PLAN_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentPlan {
    pub plan_version: u32,
    pub baseline: ExperimentCase,
    #[serde(default)]
    pub experiments: Vec<ExperimentCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentCase {
    pub id: String,
    pub description: String,
    pub request: TransportRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<SigningInput>,
    pub environment: EnvironmentProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningInput {
    pub device_id: String,
    pub cursor: String,
    pub internal_ext: String,
    pub contact_us: String,
    pub sup_ws_ds_opt: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_mutation: Option<QueryMutation>,
    /// Safe, synthetic cookie probes. Raw cookie values are intentionally not accepted here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_mutation: Option<CookieMutation>,
}

/// Controlled cookie changes using only fixed non-secret probe values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CookieMutation {
    SetProbe {
        name: String,
        value: CookieProbeValue,
    },
    Remove {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookieProbeValue {
    Empty,
    Alpha,
    Numeric,
}

impl CookieProbeValue {
    fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Alpha => "ttl-probe-alpha",
            Self::Numeric => "1700000000",
        }
    }
}

impl CookieMutation {
    fn validate(&self, case_id: &str) -> Result<(), ExperimentError> {
        let name = match self {
            Self::SetProbe { name, .. } | Self::Remove { name } => name,
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(ExperimentError::InvalidCookieMutation {
                case_id: case_id.to_string(),
                message: "cookie name must contain only ASCII letters, digits, '_' or '-'".into(),
            });
        }
        Ok(())
    }

    pub fn apply(&self, jar: &mut CookieJar) {
        match self {
            Self::SetProbe { name, value } => {
                jar.set(name, value.as_str());
            }
            Self::Remove { name } => jar.remove(name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryMutation {
    Remove {
        name: String,
        #[serde(default)]
        occurrence: usize,
    },
    Duplicate {
        name: String,
        #[serde(default)]
        occurrence: usize,
    },
    Set {
        name: String,
        #[serde(default)]
        occurrence: usize,
        value: String,
    },
    Move {
        name: String,
        #[serde(default)]
        occurrence: usize,
        new_index: usize,
    },
}

/// Declared experiment input safe to embed in durable observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedExperimentCase {
    pub id: String,
    pub description: String,
    pub request: TransportRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<ObservedSigningInput>,
    pub environment: EnvironmentProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSigningInput {
    pub device_id: ValueDigest,
    pub cursor: ValueDigest,
    pub internal_ext: ValueDigest,
    pub contact_us: ValueDigest,
    pub sup_ws_ds_opt: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_mutation: Option<ObservedQueryMutation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_mutation: Option<CookieMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservedQueryMutation {
    Remove {
        name: String,
        occurrence: usize,
    },
    Duplicate {
        name: String,
        occurrence: usize,
    },
    Set {
        name: String,
        occurrence: usize,
        value: ValueDigest,
    },
    Move {
        name: String,
        occurrence: usize,
        new_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentProfile {
    pub browser_name: String,
    pub browser_version: String,
    pub browser_platform: String,
    pub os: String,
    pub language: String,
    pub browser_language: String,
    pub tz_name: String,
    pub region: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub timestamp: TimestampMode,
    pub session: SessionMode,
}

impl EnvironmentProfile {
    pub fn chrome_linux_guest() -> Self {
        let preset = Preset::new(
            DevicePreset::chrome_linux(),
            LocationPreset::us_east(),
            ScreenPreset::FHD,
        );
        Self::from_preset(&preset, TimestampMode::System, SessionMode::Guest)
    }

    pub fn from_preset(preset: &Preset, timestamp: TimestampMode, session: SessionMode) -> Self {
        Self {
            browser_name: preset.device.browser_name.clone(),
            browser_version: preset.device.browser_version.clone(),
            browser_platform: preset.device.browser_platform.clone(),
            os: preset.device.os.clone(),
            language: preset.location.language.clone(),
            browser_language: preset.location.browser_language.clone(),
            tz_name: preset.location.tz_name.clone(),
            region: preset.location.region.clone(),
            screen_width: preset.screen.width,
            screen_height: preset.screen.height,
            timestamp,
            session,
        }
    }

    pub fn preset(&self) -> Preset {
        Preset::new(
            DevicePreset {
                browser_name: self.browser_name.clone(),
                browser_version: self.browser_version.clone(),
                browser_platform: self.browser_platform.clone(),
                os: self.os.clone(),
            },
            LocationPreset {
                language: self.language.clone(),
                browser_language: self.browser_language.clone(),
                tz_name: self.tz_name.clone(),
                region: self.region.clone(),
            },
            ScreenPreset {
                width: self.screen_width,
                height: self.screen_height,
            },
        )
    }

    fn differences(&self, other: &Self) -> Vec<ExperimentDimension> {
        let mut differences = Vec::new();
        macro_rules! compare {
            ($field:ident, $dimension:ident) => {
                if self.$field != other.$field {
                    differences.push(ExperimentDimension::$dimension);
                }
            };
        }
        compare!(browser_name, BrowserName);
        compare!(browser_version, BrowserVersion);
        compare!(browser_platform, BrowserPlatform);
        compare!(os, Os);
        compare!(language, Language);
        compare!(browser_language, BrowserLanguage);
        compare!(tz_name, Timezone);
        compare!(region, Region);
        compare!(screen_width, ScreenWidth);
        compare!(screen_height, ScreenHeight);
        compare!(timestamp, Timestamp);
        compare!(session, Session);
        differences
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimestampMode {
    System,
    Fixed { timestamp_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Guest,
    Configured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentDimension {
    RoomId,
    SigningInput,
    DeviceId,
    Cursor,
    InternalExt,
    ContactUs,
    SupWsDsOpt,
    QueryMutation,
    Cookie,
    BrowserName,
    BrowserVersion,
    BrowserPlatform,
    Os,
    Language,
    BrowserLanguage,
    Timezone,
    Region,
    ScreenWidth,
    ScreenHeight,
    Timestamp,
    Session,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExperimentError {
    #[error("unsupported experiment plan version {found}; expected {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },
    #[error("case id is empty or contains characters outside [A-Za-z0-9_-]: {0}")]
    UnsafeCaseId(String),
    #[error("duplicate case id: {0}")]
    DuplicateCaseId(String),
    #[error("case {case_id} has a non-numeric room id")]
    InvalidRoomId { case_id: String },
    #[error("case {case_id} has an incomplete environment field: {field}")]
    IncompleteEnvironment {
        case_id: String,
        field: &'static str,
    },
    #[error("experiment {case_id} must change exactly one dimension; changed {changed:?}")]
    UncontrolledMutation {
        case_id: String,
        changed: Vec<ExperimentDimension>,
    },
    #[error("unknown experiment case: {0}")]
    UnknownCase(String),
    #[error("case {case_id} has no signing input")]
    MissingSigningInput { case_id: String },
    #[error("case {case_id} has an invalid query mutation: {message}")]
    InvalidQueryMutation { case_id: String, message: String },
    #[error("experiment {case_id} has an invalid cookie mutation: {message}")]
    InvalidCookieMutation { case_id: String, message: String },
}

impl ExperimentPlan {
    pub fn validate(&self) -> Result<(), ExperimentError> {
        if self.plan_version != EXPERIMENT_PLAN_VERSION {
            return Err(ExperimentError::UnsupportedVersion {
                found: self.plan_version,
                expected: EXPERIMENT_PLAN_VERSION,
            });
        }

        let mut ids = HashSet::new();
        validate_case(&self.baseline)?;
        ids.insert(self.baseline.id.as_str());
        for experiment in &self.experiments {
            validate_case(experiment)?;
            if !ids.insert(experiment.id.as_str()) {
                return Err(ExperimentError::DuplicateCaseId(experiment.id.clone()));
            }
            let changed = experiment.changed_dimensions_from(&self.baseline);
            if changed.len() != 1 {
                return Err(ExperimentError::UncontrolledMutation {
                    case_id: experiment.id.clone(),
                    changed,
                });
            }
        }
        Ok(())
    }

    pub fn select(&self, id: &str) -> Result<&ExperimentCase, ExperimentError> {
        self.validate()?;
        if self.baseline.id == id {
            return Ok(&self.baseline);
        }
        self.experiments
            .iter()
            .find(|case| case.id == id)
            .ok_or_else(|| ExperimentError::UnknownCase(id.to_string()))
    }
}

impl ExperimentCase {
    pub fn research_case(&self) -> ResearchCase {
        ResearchCase {
            id: self.id.clone(),
            description: self.description.clone(),
            request: self.request.clone(),
            environment: EnvironmentInput {
                preset: format!("{}/{}", self.environment.os, self.environment.region),
                timestamp_mode: match self.environment.timestamp {
                    TimestampMode::System => "system".into(),
                    TimestampMode::Fixed { timestamp_ms } => format!("fixed:{timestamp_ms}"),
                },
                session: match self.environment.session {
                    SessionMode::Guest => "guest".into(),
                    SessionMode::Configured => "configured".into(),
                },
            },
        }
    }

    pub fn changed_dimensions_from(&self, baseline: &Self) -> Vec<ExperimentDimension> {
        let mut changed = baseline.environment.differences(&self.environment);
        if baseline.request.room_id != self.request.room_id {
            changed.insert(0, ExperimentDimension::RoomId);
        }
        match (&baseline.signing, &self.signing) {
            (Some(baseline), Some(current)) => {
                if baseline.device_id != current.device_id {
                    changed.push(ExperimentDimension::DeviceId);
                }
                if baseline.cursor != current.cursor {
                    changed.push(ExperimentDimension::Cursor);
                }
                if baseline.internal_ext != current.internal_ext {
                    changed.push(ExperimentDimension::InternalExt);
                }
                if baseline.contact_us != current.contact_us {
                    changed.push(ExperimentDimension::ContactUs);
                }
                if baseline.sup_ws_ds_opt != current.sup_ws_ds_opt {
                    changed.push(ExperimentDimension::SupWsDsOpt);
                }
                if baseline.query_mutation != current.query_mutation {
                    changed.push(ExperimentDimension::QueryMutation);
                }
                if baseline.cookie_mutation != current.cookie_mutation {
                    changed.push(ExperimentDimension::Cookie);
                }
            }
            (None, None) => {}
            _ => changed.push(ExperimentDimension::SigningInput),
        }
        changed
    }

    pub fn observed_input(&self) -> ObservedExperimentCase {
        ObservedExperimentCase {
            id: self.id.clone(),
            description: self.description.clone(),
            request: self.request.clone(),
            signing: self.signing.as_ref().map(|signing| ObservedSigningInput {
                device_id: ValueDigest::of(&signing.device_id),
                cursor: ValueDigest::of(&signing.cursor),
                internal_ext: ValueDigest::of(&signing.internal_ext),
                contact_us: ValueDigest::of(&signing.contact_us),
                sup_ws_ds_opt: signing.sup_ws_ds_opt,
                query_mutation: signing.query_mutation.as_ref().map(QueryMutation::observed),
                cookie_mutation: signing.cookie_mutation.clone(),
            }),
            environment: self.environment.clone(),
        }
    }

    /// Build the exact ordered unsigned URL declared by this experiment.
    pub fn signing_url(&self) -> Result<String, ExperimentError> {
        let signing =
            self.signing
                .as_ref()
                .ok_or_else(|| ExperimentError::MissingSigningInput {
                    case_id: self.id.clone(),
                })?;
        let params = FetchParams {
            room_id: self.request.room_id.clone(),
            device_id: signing.device_id.clone(),
            cursor: signing.cursor.clone(),
            internal_ext: signing.internal_ext.clone(),
            contact_us: signing.contact_us.clone(),
            sup_ws_ds_opt: signing.sup_ws_ds_opt,
        };
        let mut entries: Vec<(String, String)> = params
            .build(&self.environment.preset())
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        if let Some(mutation) = &signing.query_mutation {
            mutation.apply(&self.id, &mut entries)?;
        }
        let mut query = Query::new();
        for (name, value) in entries {
            query.push_raw(name, value);
        }
        Ok(format!("{FETCH_ENDPOINT}?{}", query.encode()))
    }
}

impl ObservedExperimentCase {
    pub fn changed_dimensions_from(&self, baseline: &Self) -> Vec<ExperimentDimension> {
        let mut changed = baseline.environment.differences(&self.environment);
        if baseline.request.room_id != self.request.room_id {
            changed.insert(0, ExperimentDimension::RoomId);
        }
        match (&baseline.signing, &self.signing) {
            (Some(baseline), Some(current)) => {
                if baseline.device_id != current.device_id {
                    changed.push(ExperimentDimension::DeviceId);
                }
                if baseline.cursor != current.cursor {
                    changed.push(ExperimentDimension::Cursor);
                }
                if baseline.internal_ext != current.internal_ext {
                    changed.push(ExperimentDimension::InternalExt);
                }
                if baseline.contact_us != current.contact_us {
                    changed.push(ExperimentDimension::ContactUs);
                }
                if baseline.sup_ws_ds_opt != current.sup_ws_ds_opt {
                    changed.push(ExperimentDimension::SupWsDsOpt);
                }
                if baseline.query_mutation != current.query_mutation {
                    changed.push(ExperimentDimension::QueryMutation);
                }
                if baseline.cookie_mutation != current.cookie_mutation {
                    changed.push(ExperimentDimension::Cookie);
                }
            }
            (None, None) => {}
            _ => changed.push(ExperimentDimension::SigningInput),
        }
        changed
    }
}

impl QueryMutation {
    fn observed(&self) -> ObservedQueryMutation {
        match self {
            Self::Remove { name, occurrence } => ObservedQueryMutation::Remove {
                name: name.clone(),
                occurrence: *occurrence,
            },
            Self::Duplicate { name, occurrence } => ObservedQueryMutation::Duplicate {
                name: name.clone(),
                occurrence: *occurrence,
            },
            Self::Set {
                name,
                occurrence,
                value,
            } => ObservedQueryMutation::Set {
                name: name.clone(),
                occurrence: *occurrence,
                value: ValueDigest::of(value),
            },
            Self::Move {
                name,
                occurrence,
                new_index,
            } => ObservedQueryMutation::Move {
                name: name.clone(),
                occurrence: *occurrence,
                new_index: *new_index,
            },
        }
    }

    fn apply(
        &self,
        case_id: &str,
        entries: &mut Vec<(String, String)>,
    ) -> Result<(), ExperimentError> {
        let (name, occurrence) = match self {
            Self::Remove { name, occurrence }
            | Self::Duplicate { name, occurrence }
            | Self::Set {
                name, occurrence, ..
            }
            | Self::Move {
                name, occurrence, ..
            } => (name, *occurrence),
        };
        let index = nth_index(entries, name, occurrence).ok_or_else(|| {
            ExperimentError::InvalidQueryMutation {
                case_id: case_id.to_string(),
                message: format!("parameter {name} occurrence {occurrence} does not exist"),
            }
        })?;
        match self {
            Self::Remove { .. } => {
                entries.remove(index);
            }
            Self::Duplicate { .. } => {
                let entry = entries[index].clone();
                entries.insert(index + 1, entry);
            }
            Self::Set { value, .. } => entries[index].1 = value.clone(),
            Self::Move { new_index, .. } => {
                if *new_index >= entries.len() {
                    return Err(ExperimentError::InvalidQueryMutation {
                        case_id: case_id.to_string(),
                        message: format!("new_index {new_index} is outside the query"),
                    });
                }
                let entry = entries.remove(index);
                entries.insert(*new_index, entry);
            }
        }
        Ok(())
    }
}

fn nth_index(entries: &[(String, String)], name: &str, occurrence: usize) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, (candidate, _))| candidate == name)
        .nth(occurrence)
        .map(|(index, _)| index)
}

fn validate_case(case: &ExperimentCase) -> Result<(), ExperimentError> {
    if case.id.is_empty()
        || !case
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ExperimentError::UnsafeCaseId(case.id.clone()));
    }
    if case.request.room_id.is_empty() || !case.request.room_id.chars().all(|c| c.is_ascii_digit())
    {
        return Err(ExperimentError::InvalidRoomId {
            case_id: case.id.clone(),
        });
    }
    for (field, value) in [
        ("browser_name", case.environment.browser_name.as_str()),
        ("browser_version", case.environment.browser_version.as_str()),
        (
            "browser_platform",
            case.environment.browser_platform.as_str(),
        ),
        ("os", case.environment.os.as_str()),
        ("language", case.environment.language.as_str()),
        (
            "browser_language",
            case.environment.browser_language.as_str(),
        ),
        ("tz_name", case.environment.tz_name.as_str()),
        ("region", case.environment.region.as_str()),
    ] {
        if value.is_empty() {
            return Err(ExperimentError::IncompleteEnvironment {
                case_id: case.id.clone(),
                field,
            });
        }
    }
    if case.environment.screen_width == 0 {
        return Err(ExperimentError::IncompleteEnvironment {
            case_id: case.id.clone(),
            field: "screen_width",
        });
    }
    if case.environment.screen_height == 0 {
        return Err(ExperimentError::IncompleteEnvironment {
            case_id: case.id.clone(),
            field: "screen_height",
        });
    }
    if let Some(signing) = &case.signing {
        if signing.device_id.len() != 19 || !signing.device_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(ExperimentError::IncompleteEnvironment {
                case_id: case.id.clone(),
                field: "signing.device_id (expected 19 digits)",
            });
        }
        if signing.query_mutation.is_some() {
            case.signing_url()?;
        }
        if let Some(cookie_mutation) = &signing.cookie_mutation {
            cookie_mutation.validate(&case.id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> ExperimentPlan {
        let baseline = ExperimentCase {
            id: "baseline".into(),
            description: "baseline".into(),
            request: TransportRequest::new("7300000000000000001"),
            signing: None,
            environment: EnvironmentProfile::chrome_linux_guest(),
        };
        let mut timezone = baseline.clone();
        timezone.id = "timezone".into();
        timezone.environment.tz_name = "America/Lima".into();
        ExperimentPlan {
            plan_version: EXPERIMENT_PLAN_VERSION,
            baseline,
            experiments: vec![timezone],
        }
    }

    fn signing_case() -> ExperimentCase {
        ExperimentCase {
            id: "query".into(),
            description: "query mutation".into(),
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
    fn accepts_exactly_one_controlled_mutation() {
        let plan = plan();
        plan.validate().unwrap();
        assert_eq!(plan.select("timezone").unwrap().id, "timezone");
    }

    #[test]
    fn rejects_zero_or_multiple_mutations() {
        let mut unchanged = plan();
        unchanged.experiments[0].environment = unchanged.baseline.environment.clone();
        assert!(matches!(
            unchanged.validate(),
            Err(ExperimentError::UncontrolledMutation { changed, .. }) if changed.is_empty()
        ));

        let mut multiple = plan();
        multiple.experiments[0].environment.language = "es".into();
        assert!(matches!(
            multiple.validate(),
            Err(ExperimentError::UncontrolledMutation { changed, .. }) if changed.len() == 2
        ));
    }

    #[test]
    fn rejects_unsafe_and_duplicate_ids() {
        let mut unsafe_plan = plan();
        unsafe_plan.experiments[0].id = "../escape".into();
        assert!(matches!(
            unsafe_plan.validate(),
            Err(ExperimentError::UnsafeCaseId(_))
        ));

        let mut duplicate = plan();
        duplicate.experiments[0].id = duplicate.baseline.id.clone();
        assert!(matches!(
            duplicate.validate(),
            Err(ExperimentError::DuplicateCaseId(_))
        ));
    }

    #[test]
    fn applies_ordered_query_mutations_without_losing_duplicates() {
        let mut case = signing_case();
        case.signing.as_mut().unwrap().query_mutation = Some(QueryMutation::Duplicate {
            name: "room_id".into(),
            occurrence: 0,
        });
        let url = case.signing_url().unwrap();
        let query = Query::parse_encoded(url.split_once('?').unwrap().1);
        assert_eq!(
            query.iter().filter(|(name, _)| *name == "room_id").count(),
            2
        );

        case.signing.as_mut().unwrap().query_mutation = Some(QueryMutation::Move {
            name: "room_id".into(),
            occurrence: 0,
            new_index: 0,
        });
        let url = case.signing_url().unwrap();
        let query = Query::parse_encoded(url.split_once('?').unwrap().1);
        assert_eq!(query.iter().next().unwrap().0, "room_id");

        case.signing.as_mut().unwrap().query_mutation = Some(QueryMutation::Set {
            name: "tz_name".into(),
            occurrence: 0,
            value: "a/b c".into(),
        });
        assert!(case.signing_url().unwrap().contains("tz_name=a%2Fb%20c"));
    }

    #[test]
    fn rejects_a_query_mutation_that_does_not_resolve() {
        let mut case = signing_case();
        case.signing.as_mut().unwrap().query_mutation = Some(QueryMutation::Remove {
            name: "missing".into(),
            occurrence: 0,
        });
        assert!(matches!(
            case.signing_url(),
            Err(ExperimentError::InvalidQueryMutation { .. })
        ));
    }

    #[test]
    fn cookie_probe_is_fixed_vocabulary_and_one_controlled_dimension() {
        let baseline = signing_case();
        let mut experiment = baseline.clone();
        experiment.id = "cookie-probe".into();
        experiment.signing.as_mut().unwrap().cookie_mutation = Some(CookieMutation::SetProbe {
            name: "msToken".into(),
            value: CookieProbeValue::Alpha,
        });
        assert_eq!(
            experiment.changed_dimensions_from(&baseline),
            vec![ExperimentDimension::Cookie]
        );
        let mut jar = CookieJar::new();
        experiment
            .signing
            .as_ref()
            .unwrap()
            .cookie_mutation
            .as_ref()
            .unwrap()
            .apply(&mut jar);
        assert_eq!(jar.get("msToken"), Some("ttl-probe-alpha"));
        assert!(serde_json::to_string(&experiment.observed_input())
            .unwrap()
            .contains("alpha"));
    }
}
