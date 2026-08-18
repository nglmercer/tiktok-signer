//! Route-oriented reduction of a sanitized VM trace to the reachable signing subgraph.
//!
//! A full VM trace describes everything the bundle executed. This module reduces it to the
//! *minimum reachable subgraph* rooted at a confirmed signing entry point, so the repository can
//! state what a native implementation of one route would have to reproduce — without modelling
//! the whole 355-handler machine.
//!
//! # What is retained
//!
//! Frame entry and parents, call edges, handler/opcode slots, operand *widths* and helper kinds,
//! sanitized argument and return shape classes, register read/write counts, and environment
//! dependency flags.
//!
//! # What is dropped, by construction
//!
//! Raw operand *values* and operand byte strings (they index the VM string and numeric constant
//! tables), the string table itself, decoded string slots, bundle source, and every field of the
//! trace that is not needed to describe execution structure. These fields are never read into the
//! output types, so no future edit can leak them by forgetting to sanitize.
//!
//! The extractor is a pure function over an already-sanitized artifact: it needs no WebView and
//! builds without the `webview` feature.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::vm_trace::{
    VmCallInput, VmOpcodeCatalogEntry, VmTrace, VmTraceReport, VmValueShape, VM_TRACE_VERSION,
};
use crate::ValueDigest;

/// Version of the emitted subgraph document. Bump on any incompatible format change.
pub const SIGNING_SUBGRAPH_VERSION: u32 = 1;

/// Upper bound on frames pulled into one route, so a root near the VM bootstrap cannot drag in
/// the entire trace. Exceeding it is reported rather than silently truncating the model.
const MAX_ROUTE_FRAMES: usize = 512;

/// Signing routes this extractor knows how to root.
///
/// The variant order is the document order: serialization never depends on discovery order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteName {
    FetchComposition,
    MsToken,
    XDynosaur,
    XGnarly,
    FrontierXBogus,
}

impl RouteName {
    pub const ALL: [RouteName; 5] = [
        RouteName::FetchComposition,
        RouteName::MsToken,
        RouteName::XDynosaur,
        RouteName::XGnarly,
        RouteName::FrontierXBogus,
    ];

    /// Signing field this route produces, as it appears in the fetch suffix.
    pub fn field(self) -> &'static str {
        match self {
            RouteName::FetchComposition => "fetch composition",
            RouteName::MsToken => "msToken",
            RouteName::XDynosaur => "X-Dynosaur",
            RouteName::XGnarly => "X-Gnarly",
            RouteName::FrontierXBogus => "X-Bogus (frontierSign)",
        }
    }
}

/// A route and the VM entry offsets it is rooted at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSpec {
    pub route: RouteName,
    /// Sorted, deduplicated root entries.
    pub roots: Vec<u32>,
}

impl RouteSpec {
    pub fn new(route: RouteName, roots: impl IntoIterator<Item = u32>) -> Self {
        let roots: BTreeSet<u32> = roots.into_iter().collect();
        Self {
            route,
            roots: roots.into_iter().collect(),
        }
    }
}

/// The confirmed roots documented in `docs/09-signing-research.md`.
///
/// `msToken` has two roots because the sanitized corpus observes two msToken-shaped returns
/// during bundle evaluation; the wrapper frame between them is discovered by reachability rather
/// than hardcoded.
pub fn default_routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec::new(RouteName::FetchComposition, [58628]),
        RouteSpec::new(RouteName::MsToken, [8039, 92825]),
        RouteSpec::new(RouteName::XDynosaur, [55188]),
        RouteSpec::new(RouteName::XGnarly, [48886]),
        RouteSpec::new(RouteName::FrontierXBogus, [69021]),
    ]
}

/// Execution phase a frame was observed in. Bounded vocabulary: an unrecognized phase string
/// becomes `Unknown` rather than widening the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracePhase {
    Eval,
    Invocation,
    Unknown,
}

impl TracePhase {
    fn parse(raw: &str) -> Self {
        match raw {
            "eval" => TracePhase::Eval,
            "invocation" => TracePhase::Invocation,
            _ => TracePhase::Unknown,
        }
    }
}

/// Bounded JavaScript value-type vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    String,
    Number,
    Boolean,
    Object,
    Function,
    Undefined,
    Unknown,
}

impl ValueType {
    /// Map a JavaScript `typeof` string onto the bounded vocabulary.
    pub fn from_js(raw: &str) -> Self {
        match raw {
            "string" => ValueType::String,
            "number" => ValueType::Number,
            "boolean" => ValueType::Boolean,
            "object" => ValueType::Object,
            "function" => ValueType::Function,
            "undefined" => ValueType::Undefined,
            _ => ValueType::Unknown,
        }
    }
}

/// Sanitized description of a value: its type, fixed-vocabulary class, observed byte lengths, and
/// sorted key names. Never the value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeClass {
    pub value_type: ValueType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_class: Option<String>,
    /// Sorted, deduplicated observed byte lengths. Signed outputs legitimately vary.
    pub byte_lengths: Vec<usize>,
    /// Sorted, deduplicated object key names.
    pub object_keys: Vec<String>,
}

impl ShapeClass {
    fn key(shape: &VmValueShape) -> (ValueType, Option<String>, Vec<String>) {
        let mut keys = shape.object_keys.clone();
        keys.sort();
        keys.dedup();
        (
            ValueType::from_js(&shape.value_type),
            shape.value_class.clone(),
            keys,
        )
    }
}

/// Accumulates observations of one shape across repeated calls.
#[derive(Debug, Default)]
struct ShapeAccumulator {
    lengths: BTreeSet<usize>,
}

fn finish_shapes(
    accumulated: BTreeMap<(ValueType, Option<String>, Vec<String>), ShapeAccumulator>,
) -> Vec<ShapeClass> {
    let mut shapes: Vec<ShapeClass> = accumulated
        .into_iter()
        .map(
            |((value_type, value_class, object_keys), accumulator)| ShapeClass {
                value_type,
                value_class,
                byte_lengths: accumulator.lengths.into_iter().collect(),
                object_keys,
            },
        )
        .collect();
    shapes.sort();
    shapes
}

/// Argument shapes observed at one positional slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgumentShape {
    pub position: usize,
    pub shapes: Vec<ShapeClass>,
}

/// Environment capabilities reachable from a frame, unioned over the opcodes it executed.
///
/// These are *capabilities*, not causality: a frame that can read `window` has not been shown to
/// depend on it. Dependency classification treats them as candidates only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentFlags {
    pub reads_window: bool,
    pub reads_document: bool,
    pub reads_storage: bool,
    pub reads_crypto: bool,
    pub reads_fetch: bool,
    pub calls_vm: bool,
}

impl EnvironmentFlags {
    fn union(&mut self, other: &EnvironmentFlags) {
        self.reads_window |= other.reads_window;
        self.reads_document |= other.reads_document;
        self.reads_storage |= other.reads_storage;
        self.reads_crypto |= other.reads_crypto;
        self.reads_fetch |= other.reads_fetch;
        self.calls_vm |= other.calls_vm;
    }

    fn from_catalog(entry: &VmOpcodeCatalogEntry) -> Self {
        Self {
            reads_window: entry.reads_window,
            reads_document: entry.reads_document,
            reads_storage: entry.reads_storage,
            reads_crypto: entry.reads_crypto,
            reads_fetch: entry.reads_fetch,
            calls_vm: entry.calls_vm,
        }
    }
}

/// Operand helper reads, by helper width: `N` is 2 bytes, `j` 1 byte, `x` 3 bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperReads {
    pub n: usize,
    pub j: usize,
    pub x: usize,
}

/// Register traffic in a frame. Counts and value classes only — never register contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterOps {
    pub reads: usize,
    pub writes: usize,
    pub distinct_registers: usize,
    /// Sorted, deduplicated fixed-vocabulary value classes seen in register traffic.
    pub value_classes: Vec<String>,
}

/// One handler slot used somewhere in the route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlerUse {
    pub opcode: u32,
    pub executions: usize,
    /// Sorted, deduplicated operand widths in bytes. Widths, never operand values.
    pub operand_widths: Vec<usize>,
    /// Sorted, deduplicated operand helper kinds (`N`, `j`, `x`).
    pub operand_helpers: Vec<String>,
    pub helper_reads: HelperReads,
    pub handler_tags: Vec<String>,
    pub environment: EnvironmentFlags,
}

/// One reachable frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubgraphFrame {
    pub entry: u32,
    /// Sorted parent entries; `null` is the VM root.
    pub parents: Vec<Option<u32>>,
    pub calls: usize,
    /// Executed VM steps attributed to this frame. Zero when the frame was reached but not
    /// step-traced (step tracing is limited to the tracer's entry allow-list).
    pub steps: usize,
    pub argument_shapes: Vec<ArgumentShape>,
    pub return_shapes: Vec<ShapeClass>,
    pub register_ops: RegisterOps,
    pub environment: EnvironmentFlags,
    /// Sorted opcode slots executed in this frame.
    pub handlers: Vec<u32>,
}

/// A parent → child call edge, with how often it was observed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallEdge {
    /// `null` is the VM root frame.
    pub parent: Option<u32>,
    pub child: u32,
    pub observations: usize,
}

/// Bounded dependency vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Query,
    Cookie,
    Clock,
    Timezone,
    Language,
    Platform,
    Screen,
    Window,
    Document,
    Storage,
    Crypto,
    Randomness,
    SdkState,
    Constant,
    Unknown,
}

/// How strongly the dependency is supported. Never upgraded without controlled evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyEvidence {
    /// A controlled, paired, one-dimension experiment moved this route's observable shape.
    ObservedDependency,
    /// The route can reach the input, but no experiment has demonstrated an effect.
    CandidateDependency,
    /// A controlled experiment changed this dimension and the route's shape did not move.
    NoObservedEffect,
    Unknown,
}

/// Where the classification came from. Structural evidence can never claim causality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencySource {
    /// Derived from opcode capability flags in a single trace.
    Structural,
    /// Derived from a paired, one-dimension controlled experiment.
    ControlledExperiment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDependency {
    pub kind: DependencyKind,
    pub evidence: DependencyEvidence,
    pub source: DependencySource,
}

/// Outcome of one paired, one-dimension controlled experiment on a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedEffect {
    /// The route's sanitized input or output shape moved.
    ShapeChanged,
    /// Repeated paired runs showed no shape movement.
    NoChange,
}

/// A controlled experiment result fed into dependency classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlledObservation {
    pub route: RouteName,
    pub dimension: DependencyKind,
    pub effect: ObservedEffect,
}

/// The reachable subgraph of one signing route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSubgraph {
    pub route: RouteName,
    pub roots: Vec<u32>,
    /// False when no root was reached in this trace: the route is still emitted, so fixtures stay
    /// structurally stable and a disappearing route is a visible regression.
    pub observed: bool,
    pub phase: TracePhase,
    /// True when reachability hit [`MAX_ROUTE_FRAMES`] and the model is incomplete.
    pub truncated: bool,
    pub frames: Vec<SubgraphFrame>,
    pub call_edges: Vec<CallEdge>,
    pub handlers: Vec<HandlerUse>,
    pub dependencies: Vec<RouteDependency>,
}

/// How a subgraph document was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Extracted by this tool from a sanitized VM trace.
    ExtractedFromVmTrace,
    /// Transcribed from the sanitized `route_frame_map` of the v1 research profile, pending a
    /// fresh authorized extraction.
    DerivedFromProfileV1,
}

/// Identity of the trace a document was reduced from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubgraphSource {
    pub provenance: Provenance,
    pub case_id: String,
    pub bundle_endpoint: String,
    pub bundle: ValueDigest,
    pub trace_version: u32,
    pub product: String,
    pub clock_ms: u64,
}

/// The committed, deterministic artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningSubgraphDocument {
    pub subgraph_version: u32,
    pub source: SubgraphSource,
    /// Routes in [`RouteName::ALL`] order, never discovery order.
    pub routes: Vec<RouteSubgraph>,
}

#[derive(Debug, thiserror::Error)]
pub enum SubgraphError {
    #[error("cannot read subgraph document: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid subgraph document: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("unsupported subgraph version {found}, expected {expected}")]
    Version { found: u32, expected: u32 },
    #[error("unsupported VM trace version {found}, expected {expected}")]
    TraceVersion { found: u32, expected: u32 },
}

/// Read a committed subgraph document.
pub fn read_subgraph_document(
    path: impl AsRef<std::path::Path>,
) -> Result<SigningSubgraphDocument, SubgraphError> {
    let raw = std::fs::read_to_string(path)?;
    let document: SigningSubgraphDocument = serde_json::from_str(&raw)?;
    if document.subgraph_version != SIGNING_SUBGRAPH_VERSION {
        return Err(SubgraphError::Version {
            found: document.subgraph_version,
            expected: SIGNING_SUBGRAPH_VERSION,
        });
    }
    Ok(document)
}

/// Serialize a document deterministically: pretty JSON with a trailing newline.
///
/// Every collection in the model is already sorted by a total key, so equivalent traces produce
/// byte-identical output.
pub fn subgraph_document_json(document: &SigningSubgraphDocument) -> String {
    let mut json = serde_json::to_string_pretty(document).expect("subgraph document serializes");
    json.push('\n');
    json
}

/// Reduce a sanitized VM trace to the reachable subgraph of each requested route.
pub fn extract_subgraphs(
    report: &VmTraceReport,
    routes: &[RouteSpec],
    controlled: &[ControlledObservation],
) -> Result<SigningSubgraphDocument, SubgraphError> {
    if report.trace_version != VM_TRACE_VERSION {
        return Err(SubgraphError::TraceVersion {
            found: report.trace_version,
            expected: VM_TRACE_VERSION,
        });
    }
    let index = TraceIndex::build(&report.trace);
    let mut extracted: BTreeMap<RouteName, RouteSubgraph> = BTreeMap::new();
    for spec in routes {
        extracted.insert(spec.route, index.extract(spec, controlled));
    }
    let mut routes: Vec<RouteSubgraph> = RouteName::ALL
        .iter()
        .filter_map(|route| extracted.remove(route))
        .collect();
    // Any route outside the known vocabulary would have been dropped above; there is none, but
    // keep remaining entries rather than silently losing them.
    routes.extend(extracted.into_values());

    Ok(SigningSubgraphDocument {
        subgraph_version: SIGNING_SUBGRAPH_VERSION,
        source: SubgraphSource {
            provenance: Provenance::ExtractedFromVmTrace,
            case_id: report.case_id.clone(),
            bundle_endpoint: report.bundle_endpoint.clone(),
            bundle: report.bundle.clone(),
            trace_version: report.trace_version,
            product: serde_json::to_value(report.trace.product)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".into()),
            clock_ms: report.trace.clock_ms,
        },
        routes,
    })
}

/// Pre-indexed view of a trace, so each route is a graph walk rather than a rescan.
struct TraceIndex<'a> {
    /// parent → children, per phase.
    children: BTreeMap<(TracePhase, Option<u32>), BTreeMap<u32, usize>>,
    /// Frame → phases it was observed in.
    frame_phases: BTreeMap<u32, BTreeSet<TracePhase>>,
    /// Frame → parents, per phase.
    parents: BTreeMap<(TracePhase, u32), BTreeSet<Option<u32>>>,
    /// Frame → call count, per phase.
    calls: BTreeMap<(TracePhase, u32), usize>,
    /// Frame → accumulated return shapes, per phase.
    returns: BTreeMap<(TracePhase, u32), BTreeMap<ShapeKey, ShapeAccumulator>>,
    /// Frame → positional argument shapes.
    arguments: BTreeMap<u32, BTreeMap<usize, BTreeMap<ShapeKey, ShapeAccumulator>>>,
    /// Frame → opcode → executions.
    frame_handlers: BTreeMap<u32, BTreeMap<u32, usize>>,
    /// Frame → opcode → operand widths and helper kinds.
    frame_operands: BTreeMap<u32, BTreeMap<u32, OperandProfile>>,
    /// Frame → register traffic.
    registers: BTreeMap<u32, RegisterAccumulator>,
    catalog: &'a BTreeMap<String, VmOpcodeCatalogEntry>,
}

type ShapeKey = (ValueType, Option<String>, Vec<String>);

/// Operand encoding observed for one opcode: byte widths and helper kinds, never values.
type OperandProfile = (BTreeSet<usize>, BTreeSet<String>);

#[derive(Debug, Default)]
struct RegisterAccumulator {
    reads: usize,
    writes: usize,
    registers: BTreeSet<usize>,
    value_classes: BTreeSet<String>,
}

fn parse_entry(raw: &str) -> Option<u32> {
    if raw == "root" {
        None
    } else {
        raw.parse::<u32>().ok()
    }
}

impl<'a> TraceIndex<'a> {
    fn build(trace: &'a VmTrace) -> Self {
        let mut index = TraceIndex {
            children: BTreeMap::new(),
            frame_phases: BTreeMap::new(),
            parents: BTreeMap::new(),
            calls: BTreeMap::new(),
            returns: BTreeMap::new(),
            arguments: BTreeMap::new(),
            frame_handlers: BTreeMap::new(),
            frame_operands: BTreeMap::new(),
            registers: BTreeMap::new(),
            catalog: &trace.opcode_catalog,
        };

        for call in &trace.vm_call_sequence {
            let Some(entry) = raw_entry(&call.entry) else {
                continue;
            };
            let parent = parse_entry(&call.parent);
            let phase = TracePhase::parse(&call.phase);

            *index
                .children
                .entry((phase, parent))
                .or_default()
                .entry(entry)
                .or_insert(0) += 1;
            index.frame_phases.entry(entry).or_default().insert(phase);
            index
                .parents
                .entry((phase, entry))
                .or_default()
                .insert(parent);
            *index.calls.entry((phase, entry)).or_insert(0) += 1;

            let shape = VmValueShape {
                value_type: call.value_type.clone(),
                bytes: call.bytes,
                value_class: None,
                object_keys: call.object_keys.clone(),
            };
            index
                .returns
                .entry((phase, entry))
                .or_default()
                .entry(ShapeClass::key(&shape))
                .or_default()
                .lengths
                .insert(call.bytes);
        }

        for input in &trace.vm_call_inputs {
            index.absorb_input(input);
        }

        for step in &trace.function_steps {
            let frame = step.function_entry as u32;
            let opcode = step.opcode as u32;
            *index
                .frame_handlers
                .entry(frame)
                .or_default()
                .entry(opcode)
                .or_insert(0) += 1;
            let operands = index
                .frame_operands
                .entry(frame)
                .or_default()
                .entry(opcode)
                .or_default();
            operands.0.insert(step.width);
            // Operand *kinds* describe the encoding width; operand values index the VM constant
            // and string tables and are deliberately not read here.
            for operand in &step.operands {
                operands.1.insert(operand.kind.clone());
            }
        }

        for event in &trace.register_trace {
            let accumulator = index
                .registers
                .entry(event.function_entry as u32)
                .or_default();
            match event.op.as_str() {
                "read" => accumulator.reads += 1,
                "write" => accumulator.writes += 1,
                _ => {}
            }
            accumulator.registers.insert(event.register);
            if let Some(class) = &event.value_class {
                accumulator.value_classes.insert(class.clone());
            }
        }

        index
    }

    fn absorb_input(&mut self, input: &VmCallInput) {
        let frame = input.entry as u32;
        let positions = self.arguments.entry(frame).or_default();
        for (position, argument) in input.args.iter().enumerate() {
            positions
                .entry(position)
                .or_default()
                .entry(ShapeClass::key(argument))
                .or_default()
                .lengths
                .insert(argument.bytes);
        }
    }

    /// Phase a route is rooted in: the phase in which any of its roots was actually observed.
    fn route_phase(&self, roots: &[u32]) -> Option<TracePhase> {
        // Prefer invocation when a root appears in both: the invocation phase is the signing call,
        // the eval phase is bundle setup.
        let mut observed = BTreeSet::new();
        for root in roots {
            if let Some(phases) = self.frame_phases.get(root) {
                observed.extend(phases.iter().copied());
            }
        }
        if observed.contains(&TracePhase::Invocation) {
            Some(TracePhase::Invocation)
        } else {
            observed.into_iter().next()
        }
    }

    fn extract(&self, spec: &RouteSpec, controlled: &[ControlledObservation]) -> RouteSubgraph {
        let Some(phase) = self.route_phase(&spec.roots) else {
            return RouteSubgraph {
                route: spec.route,
                roots: spec.roots.clone(),
                observed: false,
                phase: TracePhase::Unknown,
                truncated: false,
                frames: Vec::new(),
                call_edges: Vec::new(),
                handlers: Vec::new(),
                dependencies: classify_dependencies(
                    spec.route,
                    &EnvironmentFlags::default(),
                    controlled,
                ),
            };
        };

        // Breadth-first reachability from the roots, following parent → child edges within the
        // route's phase only, so bundle-evaluation frames cannot leak into an invocation route.
        let mut reachable: BTreeSet<u32> = BTreeSet::new();
        let mut queue: VecDeque<u32> = VecDeque::new();
        let mut truncated = false;
        for root in &spec.roots {
            if self
                .frame_phases
                .get(root)
                .is_some_and(|phases| phases.contains(&phase))
                && reachable.insert(*root)
            {
                queue.push_back(*root);
            }
        }
        let mut edges: BTreeSet<CallEdge> = BTreeSet::new();
        while let Some(frame) = queue.pop_front() {
            let Some(children) = self.children.get(&(phase, Some(frame))) else {
                continue;
            };
            for (child, observations) in children {
                edges.insert(CallEdge {
                    parent: Some(frame),
                    child: *child,
                    observations: *observations,
                });
                if reachable.len() >= MAX_ROUTE_FRAMES {
                    truncated = true;
                    continue;
                }
                if reachable.insert(*child) {
                    queue.push_back(*child);
                }
            }
        }

        // Inbound edges of the roots, so the parent context of a route is visible without pulling
        // the parent's own subtree in.
        for root in &spec.roots {
            if !reachable.contains(root) {
                continue;
            }
            if let Some(parents) = self.parents.get(&(phase, *root)) {
                for parent in parents {
                    edges.insert(CallEdge {
                        parent: *parent,
                        child: *root,
                        observations: self
                            .children
                            .get(&(phase, *parent))
                            .and_then(|children| children.get(root))
                            .copied()
                            .unwrap_or(0),
                    });
                }
            }
        }

        let mut route_environment = EnvironmentFlags::default();
        let mut handler_totals: BTreeMap<u32, usize> = BTreeMap::new();
        let mut handler_operands: BTreeMap<u32, OperandProfile> = BTreeMap::new();

        let frames: Vec<SubgraphFrame> = reachable
            .iter()
            .map(|entry| {
                let handlers = self.frame_handlers.get(entry).cloned().unwrap_or_default();
                let mut environment = EnvironmentFlags::default();
                for opcode in handlers.keys() {
                    if let Some(catalog) = self.catalog.get(&opcode.to_string()) {
                        environment.union(&EnvironmentFlags::from_catalog(catalog));
                    }
                }
                route_environment.union(&environment);

                for (opcode, executions) in &handlers {
                    *handler_totals.entry(*opcode).or_insert(0) += executions;
                }
                if let Some(operands) = self.frame_operands.get(entry) {
                    for (opcode, (widths, helpers)) in operands {
                        let slot = handler_operands.entry(*opcode).or_default();
                        slot.0.extend(widths.iter().copied());
                        slot.1.extend(helpers.iter().cloned());
                    }
                }

                let register_ops = self
                    .registers
                    .get(entry)
                    .map(|accumulator| RegisterOps {
                        reads: accumulator.reads,
                        writes: accumulator.writes,
                        distinct_registers: accumulator.registers.len(),
                        value_classes: accumulator.value_classes.iter().cloned().collect(),
                    })
                    .unwrap_or_default();

                let argument_shapes = self
                    .arguments
                    .get(entry)
                    .map(|positions| {
                        positions
                            .iter()
                            .map(|(position, shapes)| ArgumentShape {
                                position: *position,
                                shapes: finish_shapes(clone_shapes(shapes)),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let return_shapes = self
                    .returns
                    .get(&(phase, *entry))
                    .map(|shapes| finish_shapes(clone_shapes(shapes)))
                    .unwrap_or_default();

                SubgraphFrame {
                    entry: *entry,
                    parents: self
                        .parents
                        .get(&(phase, *entry))
                        .map(|parents| parents.iter().copied().collect())
                        .unwrap_or_default(),
                    calls: self.calls.get(&(phase, *entry)).copied().unwrap_or(0),
                    steps: handlers.values().sum(),
                    argument_shapes,
                    return_shapes,
                    register_ops,
                    environment,
                    handlers: handlers.keys().copied().collect(),
                }
            })
            .collect();

        let handlers: Vec<HandlerUse> = handler_totals
            .into_iter()
            .map(|(opcode, executions)| {
                let catalog = self.catalog.get(&opcode.to_string());
                let (widths, helpers) = handler_operands.remove(&opcode).unwrap_or_default();
                HandlerUse {
                    opcode,
                    executions,
                    operand_widths: widths.into_iter().collect(),
                    operand_helpers: helpers.into_iter().collect(),
                    helper_reads: catalog
                        .map(|entry| HelperReads {
                            n: entry.helper_reads.n,
                            j: entry.helper_reads.j,
                            x: entry.helper_reads.x,
                        })
                        .unwrap_or_default(),
                    handler_tags: catalog
                        .map(|entry| {
                            let mut tags = entry.handler_tags.clone();
                            tags.sort();
                            tags.dedup();
                            tags
                        })
                        .unwrap_or_default(),
                    environment: catalog
                        .map(EnvironmentFlags::from_catalog)
                        .unwrap_or_default(),
                }
            })
            .collect();

        RouteSubgraph {
            route: spec.route,
            roots: spec.roots.clone(),
            observed: !frames.is_empty(),
            phase,
            truncated,
            frames,
            call_edges: edges.into_iter().collect(),
            handlers,
            dependencies: classify_dependencies(spec.route, &route_environment, controlled),
        }
    }
}

/// `handler_operands` is consumed per opcode above; this keeps the borrow checker honest about
/// reusing accumulated shape maps without cloning the accumulator type itself.
fn clone_shapes(
    shapes: &BTreeMap<ShapeKey, ShapeAccumulator>,
) -> BTreeMap<ShapeKey, ShapeAccumulator> {
    shapes
        .iter()
        .map(|(key, accumulator)| {
            (
                key.clone(),
                ShapeAccumulator {
                    lengths: accumulator.lengths.clone(),
                },
            )
        })
        .collect()
}

fn raw_entry(raw: &str) -> Option<u32> {
    raw.parse::<u32>().ok()
}

/// Classify a route's dependencies from structural capability plus controlled experiments.
///
/// Structural capability can only ever produce [`DependencyEvidence::CandidateDependency`]:
/// reaching `window` is not the same as depending on it. Only a paired, one-dimension controlled
/// experiment can produce [`DependencyEvidence::ObservedDependency`] or
/// [`DependencyEvidence::NoObservedEffect`].
pub fn classify_dependencies(
    route: RouteName,
    environment: &EnvironmentFlags,
    controlled: &[ControlledObservation],
) -> Vec<RouteDependency> {
    let mut classified: BTreeMap<DependencyKind, RouteDependency> = BTreeMap::new();

    let mut structural = |kind: DependencyKind| {
        classified.entry(kind).or_insert(RouteDependency {
            kind,
            evidence: DependencyEvidence::CandidateDependency,
            source: DependencySource::Structural,
        });
    };
    if environment.reads_window {
        structural(DependencyKind::Window);
    }
    if environment.reads_document {
        structural(DependencyKind::Document);
        // Cookies are read through `document.cookie`; reaching the document makes cookie
        // dependence possible, never demonstrated.
        structural(DependencyKind::Cookie);
    }
    if environment.reads_storage {
        structural(DependencyKind::Storage);
    }
    if environment.reads_crypto {
        structural(DependencyKind::Crypto);
        structural(DependencyKind::Randomness);
    }

    // Controlled evidence overrides structural candidacy in both directions.
    for observation in controlled.iter().filter(|entry| entry.route == route) {
        let evidence = match observation.effect {
            ObservedEffect::ShapeChanged => DependencyEvidence::ObservedDependency,
            ObservedEffect::NoChange => DependencyEvidence::NoObservedEffect,
        };
        classified.insert(
            observation.dimension,
            RouteDependency {
                kind: observation.dimension,
                evidence,
                source: DependencySource::ControlledExperiment,
            },
        );
    }

    classified.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm_trace::{
        VmCallReturn, VmFunctionStep, VmHelperReads, VmOperandValue, VmRegisterEvent,
    };

    fn catalog_entry(reads_window: bool, reads_crypto: bool) -> VmOpcodeCatalogEntry {
        VmOpcodeCatalogEntry {
            source_bytes: 64,
            helper_reads: VmHelperReads { n: 1, j: 0, x: 0 },
            calls_vm: false,
            reads_window,
            reads_document: false,
            reads_storage: false,
            reads_crypto,
            reads_fetch: false,
            handler_tags: vec!["arithmetic".into()],
            visited: 1,
            operand_widths: BTreeMap::new(),
            examples: Vec::new(),
        }
    }

    fn call(entry: u32, parent: &str, phase: &str, bytes: usize) -> VmCallReturn {
        VmCallReturn {
            entry: entry.to_string(),
            parent: parent.into(),
            value_type: "string".into(),
            bytes,
            phase: phase.into(),
            object_keys: Vec::new(),
        }
    }

    fn step(function_entry: usize, opcode: usize, width: usize) -> VmFunctionStep {
        VmFunctionStep {
            function_entry,
            offset: 0,
            opcode,
            width,
            bytes: "deadbeef".into(),
            operands: vec![VmOperandValue {
                kind: "N".into(),
                value: 658,
            }],
        }
    }

    /// A trace with one invocation route (48886 → 50818 → 51033), one unrelated invocation frame,
    /// and one eval-phase frame that must not leak into the invocation route.
    fn trace() -> VmTrace {
        let mut opcode_catalog = BTreeMap::new();
        opcode_catalog.insert("11".to_string(), catalog_entry(true, false));
        opcode_catalog.insert("12".to_string(), catalog_entry(false, true));

        VmTrace {
            product: crate::vm_trace::TraceProduct::Fetch,
            clock_ms: 1_700_000_000_000,
            bytecode_bytes: 94_030,
            opcode_table_slots: 355,
            string_table_slots: 1001,
            numeric_constant_slots: 180,
            opcode_executions: 6,
            distinct_opcodes: 2,
            distinct_opcode_counts: BTreeMap::new(),
            top_transitions: Vec::new(),
            function_invocations: 5,
            top_function_entries: Vec::new(),
            top_call_edges: Vec::new(),
            opcode_catalog,
            vm_call_entries: BTreeMap::new(),
            vm_call_delta: BTreeMap::new(),
            vm_call_sequence: vec![
                call(48886, "56", "invocation", 332),
                call(50818, "48886", "invocation", 16),
                call(51033, "50818", "invocation", 8),
                call(99999, "56", "invocation", 4),
                call(48886, "56", "eval", 12),
            ],
            vm_call_invocation_sequence: Vec::new(),
            vm_call_inputs: vec![VmCallInput {
                entry: 48886,
                parent: "56".into(),
                args: vec![VmValueShape {
                    value_type: "string".into(),
                    bytes: 1274,
                    value_class: None,
                    object_keys: Vec::new(),
                }],
                this_value: VmValueShape {
                    value_type: "object".into(),
                    bytes: 0,
                    value_class: None,
                    object_keys: Vec::new(),
                },
                context_value: VmValueShape {
                    value_type: "undefined".into(),
                    bytes: 0,
                    value_class: None,
                    object_keys: Vec::new(),
                },
            }],
            vm_string_returns: Vec::new(),
            decoded_string_slots: BTreeMap::new(),
            decoded_string_uses: BTreeMap::new(),
            function_steps: vec![step(48886, 11, 2), step(48886, 12, 4), step(50818, 11, 2)],
            register_trace: vec![
                VmRegisterEvent {
                    function_entry: 48886,
                    op: "read".into(),
                    register: 3,
                    value_type: "string".into(),
                    bytes: 4,
                    value_class: Some("typed_array".into()),
                    object_keys: Vec::new(),
                },
                VmRegisterEvent {
                    function_entry: 48886,
                    op: "write".into(),
                    register: 4,
                    value_type: "string".into(),
                    bytes: 4,
                    value_class: None,
                    object_keys: Vec::new(),
                },
            ],
            sdk_call_returns: Vec::new(),
            known_string_slots: BTreeMap::new(),
            mssdk_keys: Vec::new(),
            mssdk_functions: Vec::new(),
            mssdk_function_paths: Vec::new(),
            mssdk_own_function_paths: Vec::new(),
            mssdk_accessor_paths: Vec::new(),
            fetch_descriptor_installed: true,
            fetch_assignments: Vec::new(),
            fetch_metadata_after_init: None,
            min_visited_offset: Some(48886),
            max_visited_offset: Some(51033),
            rolling_trace_hash: "0".repeat(64),
            first_steps: Vec::new(),
            last_steps: Vec::new(),
            result_parameters: Vec::new(),
            field_events: Vec::new(),
        }
    }

    fn report() -> VmTraceReport {
        VmTraceReport {
            trace_version: VM_TRACE_VERSION,
            case_id: "baseline".into(),
            bundle_endpoint: "https://example.test/webmssdk.js".into(),
            bundle: ValueDigest::of("bundle"),
            effective_environment: crate::vm_trace::VmEnvironmentEvidence {
                language: "en".into(),
                browser_language: "en-US".into(),
                browser_platform: "Linux x86_64".into(),
                tz_name: "America/New_York".into(),
                region: "US".into(),
                screen_width: 1920,
                screen_height: 1080,
            },
            trace: trace(),
        }
    }

    fn gnarly() -> Vec<RouteSpec> {
        vec![RouteSpec::new(RouteName::XGnarly, [48886])]
    }

    #[test]
    fn reachability_keeps_only_frames_reachable_from_the_root() {
        let document = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();
        let route = &document.routes[0];
        let entries: Vec<u32> = route.frames.iter().map(|frame| frame.entry).collect();

        assert_eq!(entries, vec![48886, 50818, 51033]);
        // 99999 is called in the same phase but is not reachable from the root.
        assert!(!entries.contains(&99999));
        assert!(route.observed);
        assert_eq!(route.phase, TracePhase::Invocation);
        assert!(!route.truncated);
    }

    #[test]
    fn a_route_never_absorbs_frames_from_another_phase() {
        // 48886 also appears in the eval phase; the invocation route must ignore that call.
        let document = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();
        let root = &document.routes[0].frames[0];
        assert_eq!(root.calls, 1, "only the invocation-phase call is counted");
        assert_eq!(
            root.return_shapes[0].byte_lengths,
            vec![332],
            "the 12-byte eval return must not be merged into the invocation route"
        );
    }

    #[test]
    fn call_edges_include_the_root_parent_and_every_reachable_edge() {
        let document = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();
        let edges: Vec<(Option<u32>, u32)> = document.routes[0]
            .call_edges
            .iter()
            .map(|edge| (edge.parent, edge.child))
            .collect();
        assert_eq!(
            edges,
            vec![
                (Some(56), 48886),
                (Some(48886), 50818),
                (Some(50818), 51033),
            ]
        );
    }

    #[test]
    fn frames_carry_sanitized_shapes_and_register_counts() {
        let document = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();
        let root = &document.routes[0].frames[0];

        assert_eq!(root.argument_shapes[0].position, 0);
        assert_eq!(
            root.argument_shapes[0].shapes[0].value_type,
            ValueType::String
        );
        assert_eq!(root.argument_shapes[0].shapes[0].byte_lengths, vec![1274]);
        assert_eq!(root.return_shapes[0].byte_lengths, vec![332]);
        assert_eq!(root.register_ops.reads, 1);
        assert_eq!(root.register_ops.writes, 1);
        assert_eq!(root.register_ops.distinct_registers, 2);
        assert_eq!(root.register_ops.value_classes, vec!["typed_array"]);
        assert_eq!(root.steps, 2);
    }

    #[test]
    fn handlers_report_widths_and_helper_kinds_but_no_operand_values() {
        let document = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();
        let route = &document.routes[0];
        let opcodes: Vec<u32> = route
            .handlers
            .iter()
            .map(|handler| handler.opcode)
            .collect();
        assert_eq!(opcodes, vec![11, 12]);

        let eleven = &route.handlers[0];
        assert_eq!(eleven.executions, 2, "executed in 48886 and 50818");
        assert_eq!(eleven.operand_widths, vec![2]);
        assert_eq!(eleven.operand_helpers, vec!["N"]);

        // The operand value 658 (a string-table slot) must never reach the document.
        let json = subgraph_document_json(&document);
        assert!(!json.contains("658"), "operand values must not be retained");
        assert!(
            !json.contains("deadbeef"),
            "operand byte strings must not be retained"
        );
    }

    #[test]
    fn an_unreached_route_is_still_emitted_so_fixtures_stay_stable() {
        let routes = vec![RouteSpec::new(RouteName::MsToken, [8039, 92825])];
        let document = extract_subgraphs(&report(), &routes, &[]).unwrap();
        let route = &document.routes[0];
        assert!(!route.observed);
        assert_eq!(route.phase, TracePhase::Unknown);
        assert!(route.frames.is_empty());
        assert!(route.call_edges.is_empty());
    }

    #[test]
    fn routes_are_emitted_in_declaration_order_not_discovery_order() {
        let shuffled = vec![
            RouteSpec::new(RouteName::XGnarly, [48886]),
            RouteSpec::new(RouteName::FetchComposition, [58628]),
            RouteSpec::new(RouteName::MsToken, [8039]),
        ];
        let document = extract_subgraphs(&report(), &shuffled, &[]).unwrap();
        let order: Vec<RouteName> = document.routes.iter().map(|route| route.route).collect();
        assert_eq!(
            order,
            vec![
                RouteName::FetchComposition,
                RouteName::MsToken,
                RouteName::XGnarly
            ]
        );
    }

    #[test]
    fn output_is_stable_when_trace_record_order_changes() {
        let baseline = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();

        let mut shuffled = report();
        shuffled.trace.vm_call_sequence.reverse();
        shuffled.trace.function_steps.reverse();
        shuffled.trace.register_trace.reverse();
        let reordered = extract_subgraphs(&shuffled, &gnarly(), &[]).unwrap();

        assert_eq!(
            subgraph_document_json(&baseline),
            subgraph_document_json(&reordered),
            "serialization must not depend on trace record order"
        );
    }

    #[test]
    fn document_round_trips_byte_identically() {
        let document = extract_subgraphs(&report(), &default_routes(), &[]).unwrap();
        let json = subgraph_document_json(&document);
        let parsed: SigningSubgraphDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, document);
        assert_eq!(subgraph_document_json(&parsed), json);
    }

    /// The on-disk path the extractor binary uses: a serialized report must read back and reduce
    /// to the same document as the in-memory one.
    #[test]
    fn a_trace_round_trips_through_disk() {
        let report = report();
        let path = std::env::temp_dir().join(format!(
            "ttl-vm-trace-{}-{}.json",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, serde_json::to_string(&report).unwrap()).unwrap();

        let loaded = crate::vm_trace::read_vm_trace(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded, report);
        assert_eq!(
            subgraph_document_json(&extract_subgraphs(&loaded, &gnarly(), &[]).unwrap()),
            subgraph_document_json(&extract_subgraphs(&report, &gnarly(), &[]).unwrap())
        );
    }

    #[test]
    fn a_mismatched_trace_version_is_refused() {
        let mut stale = report();
        stale.trace_version = VM_TRACE_VERSION + 1;
        assert!(matches!(
            extract_subgraphs(&stale, &gnarly(), &[]),
            Err(SubgraphError::TraceVersion { .. })
        ));
    }

    #[test]
    fn structural_capability_is_only_ever_a_candidate() {
        let document = extract_subgraphs(&report(), &gnarly(), &[]).unwrap();
        let dependencies = &document.routes[0].dependencies;

        assert!(!dependencies.is_empty());
        for dependency in dependencies {
            if dependency.source == DependencySource::Structural {
                assert_eq!(
                    dependency.evidence,
                    DependencyEvidence::CandidateDependency,
                    "structural evidence must never claim causality"
                );
            }
        }
        // Opcode 11 reads window, opcode 12 reads crypto.
        let kinds: Vec<DependencyKind> = dependencies.iter().map(|entry| entry.kind).collect();
        assert!(kinds.contains(&DependencyKind::Window));
        assert!(kinds.contains(&DependencyKind::Crypto));
        assert!(kinds.contains(&DependencyKind::Randomness));
    }

    #[test]
    fn controlled_experiments_upgrade_and_downgrade_classification() {
        let controlled = [
            ControlledObservation {
                route: RouteName::XGnarly,
                dimension: DependencyKind::Cookie,
                effect: ObservedEffect::ShapeChanged,
            },
            ControlledObservation {
                route: RouteName::XGnarly,
                dimension: DependencyKind::Window,
                effect: ObservedEffect::NoChange,
            },
            ControlledObservation {
                route: RouteName::MsToken,
                dimension: DependencyKind::Language,
                effect: ObservedEffect::ShapeChanged,
            },
        ];
        let document = extract_subgraphs(&report(), &gnarly(), &controlled).unwrap();
        let dependencies = &document.routes[0].dependencies;
        let find = |kind: DependencyKind| {
            dependencies
                .iter()
                .find(|entry| entry.kind == kind)
                .copied()
                .unwrap()
        };

        let cookie = find(DependencyKind::Cookie);
        assert_eq!(cookie.evidence, DependencyEvidence::ObservedDependency);
        assert_eq!(cookie.source, DependencySource::ControlledExperiment);

        // A controlled null result overrides the structural candidate.
        let window = find(DependencyKind::Window);
        assert_eq!(window.evidence, DependencyEvidence::NoObservedEffect);
        assert_eq!(window.source, DependencySource::ControlledExperiment);

        // Another route's evidence must not bleed in.
        assert!(!dependencies
            .iter()
            .any(|entry| entry.kind == DependencyKind::Language));
    }
}
