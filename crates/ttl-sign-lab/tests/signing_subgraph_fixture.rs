//! Regression gate for the committed sanitized subgraph fixture.
//!
//! The fixture is the repository's evidence-backed statement of what each signing route reaches.
//! These tests keep it deterministic, sanitized, and honest about its own provenance.
//!
//! If a legitimate change makes the canonical form differ, rewrite it in place with:
//!
//! ```sh
//! TTL_REWRITE_SUBGRAPH_FIXTURE=1 cargo test -p ttl-sign-lab --test signing_subgraph_fixture
//! ```

use std::path::PathBuf;

use ttl_sign_lab::{
    compare_subgraphs, read_subgraph_document, subgraph_document_json, ControlledObservation,
    DependencyEvidence, DependencySource, Provenance, RouteName, SigningSubgraphDocument,
    SIGNING_SUBGRAPH_VERSION,
};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/research/signing-subgraph-v1.json")
}

fn observations_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/research/controlled-observations-2026-08-13.json")
}

fn fixture() -> SigningSubgraphDocument {
    read_subgraph_document(fixture_path()).expect("committed subgraph fixture parses")
}

/// The committed bytes must already be the canonical serialization, so a regenerated document
/// diffs cleanly against it instead of producing formatting noise.
#[test]
fn committed_fixture_is_canonical() {
    let document = fixture();
    let canonical = subgraph_document_json(&document);
    let committed = std::fs::read_to_string(fixture_path()).unwrap();

    if canonical != committed && std::env::var_os("TTL_REWRITE_SUBGRAPH_FIXTURE").is_some() {
        std::fs::write(fixture_path(), &canonical).unwrap();
        return;
    }
    assert_eq!(
        canonical, committed,
        "the committed fixture is not in canonical form; rerun with \
         TTL_REWRITE_SUBGRAPH_FIXTURE=1 to rewrite it"
    );
}

#[test]
fn every_major_signing_route_is_covered() {
    let document = fixture();
    assert_eq!(document.subgraph_version, SIGNING_SUBGRAPH_VERSION);

    for route in [
        RouteName::FetchComposition,
        RouteName::MsToken,
        RouteName::XDynosaur,
        RouteName::XGnarly,
    ] {
        let covered = document
            .routes
            .iter()
            .find(|entry| entry.route == route)
            .unwrap_or_else(|| panic!("{} has no committed subgraph", route.field()));
        assert!(
            covered.observed && !covered.frames.is_empty(),
            "{} must have at least one reachable frame",
            route.field()
        );
        assert!(
            !covered.roots.is_empty(),
            "{} must declare its roots",
            route.field()
        );
    }
}

/// Routes are emitted in the declared order, never in discovery order.
#[test]
fn routes_are_in_canonical_order() {
    let document = fixture();
    let order: Vec<RouteName> = document.routes.iter().map(|route| route.route).collect();
    let expected: Vec<RouteName> = RouteName::ALL
        .iter()
        .copied()
        .filter(|route| order.contains(route))
        .collect();
    assert_eq!(order, expected);
}

/// Frames, edges, and dependencies must each be sorted by their total key.
#[test]
fn every_collection_is_sorted() {
    for route in fixture().routes {
        let entries: Vec<u32> = route.frames.iter().map(|frame| frame.entry).collect();
        let mut sorted = entries.clone();
        sorted.sort_unstable();
        assert_eq!(entries, sorted, "{:?} frames are unsorted", route.route);

        let mut edges = route.call_edges.clone();
        edges.sort();
        assert_eq!(
            route.call_edges, edges,
            "{:?} edges are unsorted",
            route.route
        );

        let mut dependencies = route.dependencies.clone();
        dependencies.sort();
        assert_eq!(
            route.dependencies, dependencies,
            "{:?} dependencies are unsorted",
            route.route
        );

        for frame in &route.frames {
            let mut roots = route.roots.clone();
            roots.sort_unstable();
            assert_eq!(route.roots, roots);
            let mut handlers = frame.handlers.clone();
            handlers.sort_unstable();
            assert_eq!(frame.handlers, handlers);
        }
    }
}

/// Structural capability can never be presented as a demonstrated dependency.
#[test]
fn no_structural_evidence_claims_causality() {
    for route in fixture().routes {
        for dependency in route.dependencies {
            if dependency.source == DependencySource::Structural {
                assert_eq!(
                    dependency.evidence,
                    DependencyEvidence::CandidateDependency,
                    "{:?}/{:?} claims causality from structural evidence only",
                    route.route,
                    dependency.kind
                );
            }
        }
    }
}

/// Every controlled classification in the fixture must be backed by the committed observation
/// corpus — no dependency may be asserted without its experiment.
#[test]
fn controlled_classifications_are_backed_by_the_observation_corpus() {
    let raw = std::fs::read_to_string(observations_path()).unwrap();
    let observations: Vec<ControlledObservation> = serde_json::from_str(&raw).unwrap();

    for route in fixture().routes {
        for dependency in route.dependencies {
            if dependency.source != DependencySource::ControlledExperiment {
                continue;
            }
            assert!(
                observations.iter().any(|observation| {
                    observation.route == route.route && observation.dimension == dependency.kind
                }),
                "{:?}/{:?} is classified from a controlled experiment that is not in the corpus",
                route.route,
                dependency.kind
            );
        }
    }
}

/// A profile-derived fixture legitimately lacks per-opcode attribution; a real extraction must
/// not. This keeps the provenance label meaningful instead of decorative.
#[test]
fn provenance_matches_the_level_of_detail_present() {
    let document = fixture();
    match document.source.provenance {
        Provenance::DerivedFromProfileV1 => {
            for route in &document.routes {
                assert!(
                    route.handlers.is_empty(),
                    "{:?} carries handler attribution the v1 profile cannot supply",
                    route.route
                );
            }
        }
        Provenance::ExtractedFromVmTrace => {
            for route in &document.routes {
                for frame in &route.frames {
                    let attributed: usize = route
                        .handlers
                        .iter()
                        .filter(|handler| frame.handlers.contains(&handler.opcode))
                        .map(|handler| handler.executions)
                        .sum();
                    assert!(
                        frame.steps == 0 || attributed > 0,
                        "{:?} frame {} has steps but no handler attribution",
                        route.route,
                        frame.entry
                    );
                }
            }
        }
    }
}

/// The fixture is its own baseline: comparing it to itself must be silent, so any future drift is
/// attributable to the change rather than to the comparison.
#[test]
fn the_fixture_is_structurally_equivalent_to_itself() {
    let document = fixture();
    let result = compare_subgraphs(&document, &document);
    assert!(result.is_structurally_equivalent());
    assert!(result.same_bundle);
}

/// The one guarantee the whole differential rests on.
#[test]
fn entropy_alone_never_reports_a_regression() {
    let baseline = fixture();
    let mut candidate = baseline.clone();
    candidate.source.case_id = "second-capture".into();
    for route in &mut candidate.routes {
        for frame in &mut route.frames {
            // Repeated signing: counts move and signed values change length.
            frame.calls += 4;
            frame.steps += 7;
            frame.register_ops.reads += 11;
            frame.register_ops.writes += 3;
            for shape in &mut frame.return_shapes {
                for length in &mut shape.byte_lengths {
                    *length += 8;
                }
            }
        }
        for edge in &mut route.call_edges {
            edge.observations += 4;
        }
    }

    let result = compare_subgraphs(&baseline, &candidate);
    assert!(
        result.is_structurally_equivalent(),
        "entropy produced false structural regressions: {:?}",
        result.differences
    );
}

/// No signing material may reach a committed subgraph. The hygiene crate enforces this over the
/// whole corpus; this asserts it at the shape the extractor could plausibly leak.
#[test]
fn the_fixture_contains_no_operand_values_or_signing_material() {
    let committed = std::fs::read_to_string(fixture_path()).unwrap();
    for forbidden in [
        "operand_values",
        "operands",
        "string_table",
        "bytes_hex",
        "msToken=",
        "X-Gnarly=",
        "X-Dynosaur=",
        "_signature",
        "sessionid",
        "ttwid",
    ] {
        assert!(
            !committed.contains(forbidden),
            "committed subgraph contains {forbidden}"
        );
    }
}
