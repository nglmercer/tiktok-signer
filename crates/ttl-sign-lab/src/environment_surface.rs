//! The browser environment surface the signing bundle actually touches.
//!
//! Both viable routes to a browser-free build (see `docs/11-webview-removal.md`) need the same
//! thing first: the exact set of browser properties `webmssdk` reads while signing. An embedded
//! JS engine must shim them; a native VM interpreter must resolve them. Guessing the list means
//! debugging a rejected transport with no signal.
//!
//! This module owns the sanitized artifact. Recording is deliberately shaped so a value cannot be
//! captured even by accident: a [`PropertyAccess`] has room for a path, operation counts, a type
//! class, and byte lengths, and nowhere to put the value itself. `document.cookie` is recorded as
//! "read once, string, 214 bytes" — never as the cookie.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::subgraph::ValueType;
use crate::ValueDigest;

/// Version of the emitted surface document. Bump on any incompatible format change.
pub const ENVIRONMENT_SURFACE_VERSION: u32 = 1;

/// Which browser object a property hangs off. Bounded: an unrecognized root becomes `Other`
/// rather than widening the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRoot {
    Window,
    Document,
    Navigator,
    Screen,
    Location,
    Storage,
    Crypto,
    Intl,
    Date,
    Other,
}

impl SurfaceRoot {
    /// Classify a dotted property path by its first segment.
    pub fn of(path: &str) -> Self {
        match path.split('.').next().unwrap_or_default() {
            "window" => SurfaceRoot::Window,
            "document" => SurfaceRoot::Document,
            "navigator" => SurfaceRoot::Navigator,
            "screen" => SurfaceRoot::Screen,
            "location" => SurfaceRoot::Location,
            "localStorage" | "sessionStorage" => SurfaceRoot::Storage,
            "crypto" => SurfaceRoot::Crypto,
            "Intl" => SurfaceRoot::Intl,
            "Date" => SurfaceRoot::Date,
            _ => SurfaceRoot::Other,
        }
    }
}

/// How a property was used. Counts only — the operands are never retained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessOps {
    pub gets: usize,
    pub sets: usize,
    pub calls: usize,
    pub has: usize,
}

impl AccessOps {
    fn merge(&mut self, other: &AccessOps) {
        self.gets += other.gets;
        self.sets += other.sets;
        self.calls += other.calls;
        self.has += other.has;
    }

    pub fn total(&self) -> usize {
        self.gets + self.sets + self.calls + self.has
    }
}

/// One property the bundle touched.
///
/// There is intentionally no field for the observed value. A shim needs the shape, not the datum,
/// and a type that cannot hold a secret cannot leak one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyAccess {
    /// Dotted path, e.g. `navigator.userAgent` or `document.cookie`.
    pub path: String,
    pub root: SurfaceRoot,
    pub operations: AccessOps,
    pub value_type: ValueType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_class: Option<String>,
    /// Sorted, deduplicated observed byte lengths for string-shaped values.
    pub byte_lengths: Vec<usize>,
}

/// Whether a trap was actually installed on a root.
///
/// A failed trap yields an empty surface for that root, which is indistinguishable from "the
/// bundle never touched it" unless the failure is recorded. It is recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentationCoverage {
    pub root: SurfaceRoot,
    pub installed: bool,
    /// Fixed-vocabulary reason when a trap could not be installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSource {
    pub case_id: String,
    pub bundle_endpoint: String,
    pub bundle: ValueDigest,
    pub product: String,
    pub clock_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSurfaceDocument {
    pub surface_version: u32,
    pub source: SurfaceSource,
    /// One entry per instrumented root, so an uninstrumented root is never mistaken for an
    /// untouched one.
    pub instrumentation: Vec<InstrumentationCoverage>,
    /// Sorted by path.
    pub properties: Vec<PropertyAccess>,
}

impl EnvironmentSurfaceDocument {
    /// Every path the bundle touched, sorted.
    pub fn paths(&self) -> Vec<&str> {
        self.properties
            .iter()
            .map(|property| property.path.as_str())
            .collect()
    }

    /// Roots where instrumentation failed. A non-empty result means the surface is incomplete and
    /// must not be treated as a shim specification.
    pub fn uninstrumented_roots(&self) -> Vec<SurfaceRoot> {
        self.instrumentation
            .iter()
            .filter(|coverage| !coverage.installed)
            .map(|coverage| coverage.root)
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnvironmentSurfaceError {
    #[error("cannot read environment surface: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid environment surface: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("unsupported environment surface version {found}, expected {expected}")]
    Version { found: u32, expected: u32 },
}

pub fn read_environment_surface(
    path: impl AsRef<Path>,
) -> Result<EnvironmentSurfaceDocument, EnvironmentSurfaceError> {
    let raw = std::fs::read_to_string(path)?;
    let document: EnvironmentSurfaceDocument = serde_json::from_str(&raw)?;
    if document.surface_version != ENVIRONMENT_SURFACE_VERSION {
        return Err(EnvironmentSurfaceError::Version {
            found: document.surface_version,
            expected: ENVIRONMENT_SURFACE_VERSION,
        });
    }
    Ok(document)
}

/// Serialize deterministically: pretty JSON with a trailing newline.
pub fn environment_surface_json(document: &EnvironmentSurfaceDocument) -> String {
    let mut json = serde_json::to_string_pretty(document).expect("surface document serializes");
    json.push('\n');
    json
}

/// Build a document from raw recorded accesses, normalizing order and merging duplicates.
///
/// Callers hand over whatever the page produced; ordering and duplication are resolved here so a
/// document is canonical by construction rather than by convention.
pub fn build_environment_surface(
    source: SurfaceSource,
    instrumentation: Vec<InstrumentationCoverage>,
    accesses: Vec<PropertyAccess>,
) -> EnvironmentSurfaceDocument {
    let mut merged: BTreeMap<String, PropertyAccess> = BTreeMap::new();
    for access in accesses {
        match merged.get_mut(&access.path) {
            Some(existing) => {
                existing.operations.merge(&access.operations);
                let mut lengths: BTreeSet<usize> = existing.byte_lengths.iter().copied().collect();
                lengths.extend(access.byte_lengths.iter().copied());
                existing.byte_lengths = lengths.into_iter().collect();
                // A property read as more than one type keeps the widest description.
                if existing.value_type != access.value_type {
                    existing.value_type = ValueType::Unknown;
                }
                if existing.value_class.is_none() {
                    existing.value_class = access.value_class;
                }
            }
            None => {
                let lengths: BTreeSet<usize> = access.byte_lengths.iter().copied().collect();
                let root = SurfaceRoot::of(&access.path);
                merged.insert(
                    access.path.clone(),
                    PropertyAccess {
                        path: access.path,
                        root,
                        operations: access.operations,
                        value_type: access.value_type,
                        value_class: access.value_class,
                        byte_lengths: lengths.into_iter().collect(),
                    },
                );
            }
        }
    }

    let mut instrumentation = instrumentation;
    instrumentation.sort_by_key(|coverage| coverage.root);
    instrumentation.dedup();

    EnvironmentSurfaceDocument {
        surface_version: ENVIRONMENT_SURFACE_VERSION,
        source,
        instrumentation,
        properties: merged.into_values().collect(),
    }
}

/// Paths the bundle touched that a shim does not implement.
///
/// This is the Phase 0 gate: a shim is complete when this is empty for the committed surface. A
/// missing property becomes a test failure with a name attached, instead of a rejected transport
/// with no signal.
pub fn missing_shim_coverage(
    document: &EnvironmentSurfaceDocument,
    implemented: &[impl AsRef<str>],
) -> Vec<String> {
    let implemented: BTreeSet<&str> = implemented.iter().map(|path| path.as_ref()).collect();
    document
        .properties
        .iter()
        .filter(|property| !implemented.contains(property.path.as_str()))
        .map(|property| property.path.clone())
        .collect()
}

/// A change between two recorded surfaces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "path")]
pub enum SurfaceDifference {
    /// The candidate touches a property the baseline did not — a shim gap opens here.
    Added(String),
    /// The candidate no longer touches a property the baseline did.
    Removed(String),
    /// The property is still touched, but its value type changed.
    TypeChanged(String),
}

/// Compare two surfaces. Access counts and byte lengths are entropy and are ignored; the set of
/// properties and their types is what a shim has to satisfy.
pub fn compare_environment_surfaces(
    baseline: &EnvironmentSurfaceDocument,
    candidate: &EnvironmentSurfaceDocument,
) -> Vec<SurfaceDifference> {
    let baseline_map: BTreeMap<&str, &PropertyAccess> = baseline
        .properties
        .iter()
        .map(|property| (property.path.as_str(), property))
        .collect();
    let candidate_map: BTreeMap<&str, &PropertyAccess> = candidate
        .properties
        .iter()
        .map(|property| (property.path.as_str(), property))
        .collect();

    let mut differences = Vec::new();
    for (path, property) in &candidate_map {
        match baseline_map.get(path) {
            None => differences.push(SurfaceDifference::Added((*path).to_string())),
            Some(existing) if existing.value_type != property.value_type => {
                differences.push(SurfaceDifference::TypeChanged((*path).to_string()));
            }
            Some(_) => {}
        }
    }
    for path in baseline_map.keys() {
        if !candidate_map.contains_key(path) {
            differences.push(SurfaceDifference::Removed((*path).to_string()));
        }
    }
    differences.sort();
    differences
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(path: &str, gets: usize, value_type: ValueType, bytes: &[usize]) -> PropertyAccess {
        PropertyAccess {
            path: path.into(),
            root: SurfaceRoot::of(path),
            operations: AccessOps {
                gets,
                ..AccessOps::default()
            },
            value_type,
            value_class: None,
            byte_lengths: bytes.to_vec(),
        }
    }

    fn source() -> SurfaceSource {
        SurfaceSource {
            case_id: "baseline".into(),
            bundle_endpoint: "https://example.invalid/webmssdk.js".into(),
            bundle: ValueDigest::of("bundle"),
            product: "fetch".into(),
            clock_ms: 1_700_000_000_000,
        }
    }

    fn document(accesses: Vec<PropertyAccess>) -> EnvironmentSurfaceDocument {
        build_environment_surface(
            source(),
            vec![InstrumentationCoverage {
                root: SurfaceRoot::Navigator,
                installed: true,
                note: None,
            }],
            accesses,
        )
    }

    #[test]
    fn roots_are_classified_from_the_path() {
        assert_eq!(
            SurfaceRoot::of("navigator.userAgent"),
            SurfaceRoot::Navigator
        );
        assert_eq!(SurfaceRoot::of("document.cookie"), SurfaceRoot::Document);
        assert_eq!(
            SurfaceRoot::of("localStorage.getItem"),
            SurfaceRoot::Storage
        );
        assert_eq!(SurfaceRoot::of("sessionStorage.x"), SurfaceRoot::Storage);
        assert_eq!(
            SurfaceRoot::of("crypto.getRandomValues"),
            SurfaceRoot::Crypto
        );
        assert_eq!(SurfaceRoot::of("Intl.DateTimeFormat"), SurfaceRoot::Intl);
        assert_eq!(SurfaceRoot::of("somethingElse"), SurfaceRoot::Other);
    }

    #[test]
    fn duplicate_accesses_merge_into_one_property() {
        let document = document(vec![
            access("document.cookie", 1, ValueType::String, &[214]),
            access("document.cookie", 2, ValueType::String, &[220]),
        ]);

        assert_eq!(document.properties.len(), 1);
        let cookie = &document.properties[0];
        assert_eq!(cookie.operations.gets, 3);
        assert_eq!(cookie.byte_lengths, vec![214, 220]);
        assert_eq!(cookie.root, SurfaceRoot::Document);
    }

    #[test]
    fn a_property_read_as_two_types_is_recorded_as_unknown() {
        let document = document(vec![
            access("window.x", 1, ValueType::String, &[4]),
            access("window.x", 1, ValueType::Number, &[]),
        ]);
        assert_eq!(document.properties[0].value_type, ValueType::Unknown);
    }

    #[test]
    fn output_is_stable_when_access_order_changes() {
        let forward = document(vec![
            access("navigator.userAgent", 1, ValueType::String, &[112]),
            access("document.cookie", 1, ValueType::String, &[214]),
            access("screen.width", 1, ValueType::Number, &[]),
        ]);
        let reversed = document(vec![
            access("screen.width", 1, ValueType::Number, &[]),
            access("document.cookie", 1, ValueType::String, &[214]),
            access("navigator.userAgent", 1, ValueType::String, &[112]),
        ]);

        assert_eq!(
            environment_surface_json(&forward),
            environment_surface_json(&reversed),
            "serialization must not depend on recording order"
        );
        assert_eq!(
            forward.paths(),
            vec!["document.cookie", "navigator.userAgent", "screen.width"]
        );
    }

    #[test]
    fn document_round_trips() {
        let document = document(vec![access(
            "navigator.userAgent",
            1,
            ValueType::String,
            &[112],
        )]);
        let json = environment_surface_json(&document);
        let parsed: EnvironmentSurfaceDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, document);
        assert_eq!(environment_surface_json(&parsed), json);
    }

    /// The Phase 0 gate: an incomplete shim names what it is missing.
    #[test]
    fn missing_shim_coverage_names_every_unimplemented_path() {
        let document = document(vec![
            access("navigator.userAgent", 1, ValueType::String, &[112]),
            access("document.cookie", 1, ValueType::String, &[214]),
            access("screen.width", 1, ValueType::Number, &[]),
        ]);

        let missing = missing_shim_coverage(&document, &["navigator.userAgent"]);
        assert_eq!(missing, vec!["document.cookie", "screen.width"]);

        let complete = missing_shim_coverage(
            &document,
            &["navigator.userAgent", "document.cookie", "screen.width"],
        );
        assert!(complete.is_empty());
    }

    /// A failed trap must be visible, because an uninstrumented root looks exactly like an
    /// untouched one in the property list.
    #[test]
    fn failed_instrumentation_is_reported_not_silently_empty() {
        let document = build_environment_surface(
            source(),
            vec![
                InstrumentationCoverage {
                    root: SurfaceRoot::Navigator,
                    installed: true,
                    note: None,
                },
                InstrumentationCoverage {
                    root: SurfaceRoot::Storage,
                    installed: false,
                    note: Some("trap_install_failed".into()),
                },
            ],
            vec![access("navigator.userAgent", 1, ValueType::String, &[112])],
        );

        assert_eq!(document.uninstrumented_roots(), vec![SurfaceRoot::Storage]);
        assert!(
            !document
                .properties
                .iter()
                .any(|property| property.root == SurfaceRoot::Storage),
            "storage looks untouched, which is exactly why the failure must be recorded"
        );
    }

    #[test]
    fn comparison_reports_shim_gaps_and_ignores_counts() {
        let baseline = document(vec![
            access("navigator.userAgent", 1, ValueType::String, &[112]),
            access("document.cookie", 1, ValueType::String, &[214]),
        ]);
        let candidate = document(vec![
            // Same property, different access count and length: not a difference.
            access("navigator.userAgent", 99, ValueType::String, &[400]),
            // A new property: a shim gap.
            access("crypto.getRandomValues", 1, ValueType::Function, &[]),
        ]);

        let differences = compare_environment_surfaces(&baseline, &candidate);
        assert_eq!(
            differences,
            vec![
                SurfaceDifference::Added("crypto.getRandomValues".into()),
                SurfaceDifference::Removed("document.cookie".into()),
            ]
        );
    }

    #[test]
    fn a_changed_value_type_is_reported() {
        let baseline = document(vec![access("window.x", 1, ValueType::String, &[4])]);
        let candidate = document(vec![access("window.x", 1, ValueType::Function, &[])]);
        assert_eq!(
            compare_environment_surfaces(&baseline, &candidate),
            vec![SurfaceDifference::TypeChanged("window.x".into())]
        );
    }

    /// The type has nowhere to put a value, so a recorded cookie is a length and nothing else.
    #[test]
    fn a_recorded_property_cannot_carry_its_value() {
        let document = document(vec![access(
            "document.cookie",
            1,
            ValueType::String,
            &[214],
        )]);
        let json = environment_surface_json(&document);
        assert!(json.contains("document.cookie"));
        assert!(json.contains("214"));
        for secret in ["msToken=", "ttwid=", "sessionid"] {
            assert!(!json.contains(secret));
        }
    }
}
