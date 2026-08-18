//! Structural Oracle-vs-Oracle regression detection over route subgraphs.
//!
//! This complements [`crate::compare_signing_traces`], which compares the *URL* layer (parameter
//! order, stability, lengths). This module compares the *VM* layer: which frames a route reaches,
//! how they call each other, which handler slots run, and what shapes cross the boundary.
//!
//! # What counts as a difference
//!
//! Sets and booleans: reachable frames, call edges, handler slots, operand widths and helper
//! kinds, argument and return *shape classes*, environment capability flags, register value
//! classes, route phase, and route observation.
//!
//! # What is deliberately ignored
//!
//! Every quantity that carries expected entropy:
//!
//! - **Byte lengths.** A signed field legitimately varies run to run (`msToken` 124–172,
//!   `X-Dynosaur` 388/392/444). Lengths remain in the document as evidence for dependency
//!   classification, but a length move is not a structural regression.
//! - **Counts.** Call counts, step counts, handler execution counts, edge observation counts, and
//!   register read/write counts depend on how many times the page happened to sign.
//! - **Digests, cookies, and session identifiers.** They are not in the subgraph model at all.
//!
//! The consequence, asserted by test: two captures of the same build that differ only in random
//! values produce zero structural differences.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::subgraph::{
    CallEdge, RouteName, RouteSubgraph, ShapeClass, SigningSubgraphDocument, TracePhase,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubgraphDifferenceKind {
    /// A route was reached in one document and not the other.
    RouteObservation,
    /// The route was rooted in a different execution phase.
    Phase,
    /// The reachable frame set changed.
    ReachableFrames,
    /// The parent → child edge set changed.
    CallEdges,
    /// The set of handler/opcode slots executed changed.
    HandlerSet,
    /// A handler's operand widths or helper kinds changed.
    OperandShapes,
    /// A frame's argument shape classes changed.
    ArgumentShapes,
    /// A frame's return shape classes changed.
    ReturnShapes,
    /// A route's environment capability flags changed.
    EnvironmentDependencies,
    /// A route's classified dependency set changed.
    DependencyClassification,
    /// The value classes seen in register traffic changed.
    RegisterProfile,
    /// Reachability hit the frame cap in one document only.
    Truncation,
}

/// One structural difference. `detail` is a bounded, sanitized description — frame entries,
/// opcode slots, and fixed-vocabulary labels only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubgraphDifference {
    pub route: RouteName,
    pub kind: SubgraphDifferenceKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubgraphDifferentialResult {
    pub baseline_case_id: String,
    pub candidate_case_id: String,
    /// True when both documents describe the same bundle. A bundle drift makes structural
    /// differences expected rather than a regression, so it is reported separately.
    pub same_bundle: bool,
    pub differences: Vec<SubgraphDifference>,
}

impl SubgraphDifferentialResult {
    pub fn is_structurally_equivalent(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two subgraph documents structurally.
pub fn compare_subgraphs(
    baseline: &SigningSubgraphDocument,
    candidate: &SigningSubgraphDocument,
) -> SubgraphDifferentialResult {
    let mut differences = Vec::new();

    let routes: BTreeSet<RouteName> = baseline
        .routes
        .iter()
        .chain(candidate.routes.iter())
        .map(|route| route.route)
        .collect();

    for name in routes {
        let left = baseline.routes.iter().find(|route| route.route == name);
        let right = candidate.routes.iter().find(|route| route.route == name);
        match (left, right) {
            (Some(left), Some(right)) => compare_route(left, right, &mut differences),
            (Some(_), None) => differences.push(SubgraphDifference {
                route: name,
                kind: SubgraphDifferenceKind::RouteObservation,
                detail: "route present in baseline only".into(),
            }),
            (None, Some(_)) => differences.push(SubgraphDifference {
                route: name,
                kind: SubgraphDifferenceKind::RouteObservation,
                detail: "route present in candidate only".into(),
            }),
            (None, None) => unreachable!("route came from one of the two documents"),
        }
    }

    differences.sort();
    differences.dedup();

    SubgraphDifferentialResult {
        baseline_case_id: baseline.source.case_id.clone(),
        candidate_case_id: candidate.source.case_id.clone(),
        same_bundle: baseline.source.bundle == candidate.source.bundle,
        differences,
    }
}

fn compare_route(
    baseline: &RouteSubgraph,
    candidate: &RouteSubgraph,
    differences: &mut Vec<SubgraphDifference>,
) {
    let route = baseline.route;
    let mut push = |kind: SubgraphDifferenceKind, detail: String| {
        differences.push(SubgraphDifference {
            route,
            kind,
            detail,
        })
    };

    if baseline.observed != candidate.observed {
        push(
            SubgraphDifferenceKind::RouteObservation,
            format!("observed {} → {}", baseline.observed, candidate.observed),
        );
    }
    if baseline.phase != candidate.phase {
        push(
            SubgraphDifferenceKind::Phase,
            format!(
                "{} → {}",
                phase_label(baseline.phase),
                phase_label(candidate.phase)
            ),
        );
    }
    if baseline.truncated != candidate.truncated {
        push(
            SubgraphDifferenceKind::Truncation,
            format!("truncated {} → {}", baseline.truncated, candidate.truncated),
        );
    }

    let baseline_frames: BTreeSet<u32> = baseline.frames.iter().map(|frame| frame.entry).collect();
    let candidate_frames: BTreeSet<u32> =
        candidate.frames.iter().map(|frame| frame.entry).collect();
    for entry in baseline_frames.difference(&candidate_frames) {
        push(
            SubgraphDifferenceKind::ReachableFrames,
            format!("frame {entry} reachable in baseline only"),
        );
    }
    for entry in candidate_frames.difference(&baseline_frames) {
        push(
            SubgraphDifferenceKind::ReachableFrames,
            format!("frame {entry} reachable in candidate only"),
        );
    }

    // Edges compare as parent → child pairs; observation counts are entropy.
    let baseline_edges: BTreeSet<(Option<u32>, u32)> =
        baseline.call_edges.iter().map(edge).collect();
    let candidate_edges: BTreeSet<(Option<u32>, u32)> =
        candidate.call_edges.iter().map(edge).collect();
    for (parent, child) in baseline_edges.difference(&candidate_edges) {
        push(
            SubgraphDifferenceKind::CallEdges,
            format!("{} → {child} in baseline only", parent_label(*parent)),
        );
    }
    for (parent, child) in candidate_edges.difference(&baseline_edges) {
        push(
            SubgraphDifferenceKind::CallEdges,
            format!("{} → {child} in candidate only", parent_label(*parent)),
        );
    }

    let baseline_handlers: BTreeSet<u32> = baseline
        .handlers
        .iter()
        .map(|handler| handler.opcode)
        .collect();
    let candidate_handlers: BTreeSet<u32> = candidate
        .handlers
        .iter()
        .map(|handler| handler.opcode)
        .collect();
    for opcode in baseline_handlers.difference(&candidate_handlers) {
        push(
            SubgraphDifferenceKind::HandlerSet,
            format!("handler {opcode} in baseline only"),
        );
    }
    for opcode in candidate_handlers.difference(&baseline_handlers) {
        push(
            SubgraphDifferenceKind::HandlerSet,
            format!("handler {opcode} in candidate only"),
        );
    }

    for opcode in baseline_handlers.intersection(&candidate_handlers) {
        let left = baseline
            .handlers
            .iter()
            .find(|handler| handler.opcode == *opcode)
            .expect("opcode is in the baseline set");
        let right = candidate
            .handlers
            .iter()
            .find(|handler| handler.opcode == *opcode)
            .expect("opcode is in the candidate set");
        if left.operand_widths != right.operand_widths {
            push(
                SubgraphDifferenceKind::OperandShapes,
                format!("handler {opcode} operand widths changed"),
            );
        }
        if left.operand_helpers != right.operand_helpers {
            push(
                SubgraphDifferenceKind::OperandShapes,
                format!("handler {opcode} operand helper kinds changed"),
            );
        }
        if left.handler_tags != right.handler_tags {
            push(
                SubgraphDifferenceKind::HandlerSet,
                format!("handler {opcode} tags changed"),
            );
        }
        if left.environment != right.environment {
            push(
                SubgraphDifferenceKind::EnvironmentDependencies,
                format!("handler {opcode} environment flags changed"),
            );
        }
    }

    for entry in baseline_frames.intersection(&candidate_frames) {
        let left = baseline
            .frames
            .iter()
            .find(|frame| frame.entry == *entry)
            .expect("frame is in the baseline set");
        let right = candidate
            .frames
            .iter()
            .find(|frame| frame.entry == *entry)
            .expect("frame is in the candidate set");

        if left.environment != right.environment {
            push(
                SubgraphDifferenceKind::EnvironmentDependencies,
                format!("frame {entry} environment flags changed"),
            );
        }
        if left.parents != right.parents {
            push(
                SubgraphDifferenceKind::CallEdges,
                format!("frame {entry} parents changed"),
            );
        }
        if left.handlers != right.handlers {
            push(
                SubgraphDifferenceKind::HandlerSet,
                format!("frame {entry} handler set changed"),
            );
        }
        if left.register_ops.value_classes != right.register_ops.value_classes {
            push(
                SubgraphDifferenceKind::RegisterProfile,
                format!("frame {entry} register value classes changed"),
            );
        }

        let baseline_arguments = argument_classes(left);
        let candidate_arguments = argument_classes(right);
        if baseline_arguments != candidate_arguments {
            push(
                SubgraphDifferenceKind::ArgumentShapes,
                format!("frame {entry} argument shape classes changed"),
            );
        }

        if shape_classes(&left.return_shapes) != shape_classes(&right.return_shapes) {
            push(
                SubgraphDifferenceKind::ReturnShapes,
                format!("frame {entry} return shape classes changed"),
            );
        }
    }

    if baseline.dependencies != candidate.dependencies {
        push(
            SubgraphDifferenceKind::DependencyClassification,
            "classified dependencies changed".into(),
        );
    }
}

fn edge(edge: &CallEdge) -> (Option<u32>, u32) {
    (edge.parent, edge.child)
}

fn parent_label(parent: Option<u32>) -> String {
    parent.map_or_else(|| "root".to_string(), |entry| entry.to_string())
}

fn phase_label(phase: TracePhase) -> &'static str {
    match phase {
        TracePhase::Eval => "eval",
        TracePhase::Invocation => "invocation",
        TracePhase::Unknown => "unknown",
    }
}

/// A shape reduced to its comparable class: byte lengths are entropy and are excluded.
type ShapeClassKey = (crate::subgraph::ValueType, Option<String>, Vec<String>);

fn shape_classes(shapes: &[ShapeClass]) -> BTreeSet<ShapeClassKey> {
    shapes
        .iter()
        .map(|shape| {
            (
                shape.value_type,
                shape.value_class.clone(),
                shape.object_keys.clone(),
            )
        })
        .collect()
}

fn argument_classes(
    frame: &crate::subgraph::SubgraphFrame,
) -> Vec<(usize, BTreeSet<ShapeClassKey>)> {
    frame
        .argument_shapes
        .iter()
        .map(|argument| (argument.position, shape_classes(&argument.shapes)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subgraph::{
        ArgumentShape, DependencyEvidence, DependencyKind, DependencySource, EnvironmentFlags,
        HandlerUse, HelperReads, Provenance, RegisterOps, RouteDependency, SubgraphFrame,
        SubgraphSource, ValueType, SIGNING_SUBGRAPH_VERSION,
    };
    use crate::ValueDigest;

    fn shape(bytes: usize) -> ShapeClass {
        ShapeClass {
            value_type: ValueType::String,
            value_class: None,
            byte_lengths: vec![bytes],
            object_keys: Vec::new(),
        }
    }

    fn frame(entry: u32, argument_bytes: usize, return_bytes: usize) -> SubgraphFrame {
        SubgraphFrame {
            entry,
            parents: vec![Some(56)],
            calls: 3,
            steps: 383,
            argument_shapes: vec![ArgumentShape {
                position: 0,
                shapes: vec![shape(argument_bytes)],
            }],
            return_shapes: vec![shape(return_bytes)],
            register_ops: RegisterOps {
                reads: 12,
                writes: 9,
                distinct_registers: 6,
                value_classes: vec!["typed_array".into()],
            },
            environment: EnvironmentFlags {
                reads_window: true,
                ..EnvironmentFlags::default()
            },
            handlers: vec![11, 12],
        }
    }

    fn document(
        case_id: &str,
        argument_bytes: usize,
        return_bytes: usize,
    ) -> SigningSubgraphDocument {
        SigningSubgraphDocument {
            subgraph_version: SIGNING_SUBGRAPH_VERSION,
            source: SubgraphSource {
                provenance: Provenance::ExtractedFromVmTrace,
                case_id: case_id.into(),
                bundle_endpoint: "https://example.test/webmssdk.js".into(),
                bundle: ValueDigest::of("bundle"),
                trace_version: 2,
                product: "fetch".into(),
                clock_ms: 1_700_000_000_000,
            },
            routes: vec![RouteSubgraph {
                route: RouteName::XGnarly,
                roots: vec![48886],
                observed: true,
                phase: TracePhase::Invocation,
                truncated: false,
                frames: vec![frame(48886, argument_bytes, return_bytes)],
                call_edges: vec![CallEdge {
                    parent: Some(56),
                    child: 48886,
                    observations: 3,
                }],
                handlers: vec![HandlerUse {
                    opcode: 11,
                    executions: 42,
                    operand_widths: vec![2],
                    operand_helpers: vec!["N".into()],
                    helper_reads: HelperReads { n: 1, j: 0, x: 0 },
                    handler_tags: vec!["arithmetic".into()],
                    environment: EnvironmentFlags::default(),
                }],
                dependencies: vec![RouteDependency {
                    kind: DependencyKind::Cookie,
                    evidence: DependencyEvidence::ObservedDependency,
                    source: DependencySource::ControlledExperiment,
                }],
            }],
        }
    }

    #[test]
    fn identical_documents_are_equivalent() {
        let result = compare_subgraphs(&document("a", 1274, 332), &document("a", 1274, 332));
        assert!(result.is_structurally_equivalent());
        assert!(result.same_bundle);
    }

    /// The headline entropy rule: only signed-value lengths moved, so there is no regression.
    #[test]
    fn a_length_change_alone_is_not_a_structural_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1282, 332);
        // Also vary every count the way repeated signing would.
        let route = &mut candidate.routes[0];
        route.frames[0].calls = 7;
        route.frames[0].steps = 401;
        route.frames[0].register_ops.reads = 30;
        route.frames[0].register_ops.writes = 21;
        route.call_edges[0].observations = 7;
        route.handlers[0].executions = 99;

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(
            result.is_structurally_equivalent(),
            "expected no differences, got {:?}",
            result.differences
        );
    }

    #[test]
    fn a_new_reachable_frame_is_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].frames.push(frame(50818, 16, 8));

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result.differences.iter().any(|difference| {
            difference.kind == SubgraphDifferenceKind::ReachableFrames
                && difference.detail.contains("50818")
        }));
    }

    #[test]
    fn a_new_call_edge_is_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].call_edges.push(CallEdge {
            parent: Some(48886),
            child: 50818,
            observations: 1,
        });

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::CallEdges));
    }

    #[test]
    fn a_changed_handler_set_is_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].handlers[0].opcode = 13;

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::HandlerSet));
    }

    #[test]
    fn changed_operand_widths_are_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].handlers[0].operand_widths = vec![2, 4];

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::OperandShapes));
    }

    #[test]
    fn a_changed_argument_class_is_a_regression_but_a_changed_length_is_not() {
        let baseline = document("baseline", 1274, 332);

        let mut length_only = document("candidate", 9999, 332);
        assert!(compare_subgraphs(&baseline, &length_only).is_structurally_equivalent());

        length_only.routes[0].frames[0].argument_shapes[0].shapes[0].value_class =
            Some("typed_array".into());
        let result = compare_subgraphs(&baseline, &length_only);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::ArgumentShapes));
    }

    #[test]
    fn a_changed_return_class_is_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].frames[0].return_shapes[0].value_type = ValueType::Object;

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::ReturnShapes));
    }

    #[test]
    fn changed_environment_flags_are_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].frames[0].environment.reads_crypto = true;

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::EnvironmentDependencies));
    }

    #[test]
    fn a_route_disappearing_is_a_regression() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.routes[0].observed = false;
        candidate.routes[0].frames.clear();

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(result
            .differences
            .iter()
            .any(|difference| difference.kind == SubgraphDifferenceKind::RouteObservation));
    }

    #[test]
    fn bundle_drift_is_reported_separately_from_differences() {
        let baseline = document("baseline", 1274, 332);
        let mut candidate = document("candidate", 1274, 332);
        candidate.source.bundle = ValueDigest::of("a different bundle");

        let result = compare_subgraphs(&baseline, &candidate);
        assert!(!result.same_bundle);
        assert!(result.is_structurally_equivalent());
    }
}
