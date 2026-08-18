//! Sanitized VM trace model.
//!
//! These types describe the artifact produced by `ttl-sign-vm-trace`. They live in the library
//! rather than in that binary so tooling which only *reads* a trace — notably the route
//! subgraph extractor — needs no browser.
//!
//! The model is already sanitized at capture time: it carries shapes, byte lengths, opcode
//! slots, and call edges, never signature bytes, cookie values, or bundle source.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use ttl_sign_core::Preset;

use crate::ValueDigest;

/// Trace schema version emitted by `ttl-sign-vm-trace`.
pub const VM_TRACE_VERSION: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum VmTraceError {
    #[error("cannot read VM trace: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid sanitized VM trace: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("unsupported VM trace version {found}, expected {expected}")]
    Version { found: u32, expected: u32 },
}

/// Read a sanitized VM trace report written by `ttl-sign-vm-trace`.
pub fn read_vm_trace(path: impl AsRef<Path>) -> Result<VmTraceReport, VmTraceError> {
    let raw = std::fs::read_to_string(path)?;
    let report: VmTraceReport = serde_json::from_str(&raw)?;
    if report.trace_version != VM_TRACE_VERSION {
        return Err(VmTraceError::Version {
            found: report.trace_version,
            expected: VM_TRACE_VERSION,
        });
    }
    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceProduct {
    Frontier,
    Fetch,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct VmTraceReport {
    pub trace_version: u32,
    pub case_id: String,
    pub bundle_endpoint: String,
    pub bundle: ValueDigest,
    pub effective_environment: VmEnvironmentEvidence,
    pub trace: VmTrace,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct VmEnvironmentEvidence {
    pub language: String,
    pub browser_language: String,
    pub browser_platform: String,
    pub tz_name: String,
    pub region: String,
    pub screen_width: u32,
    pub screen_height: u32,
}

impl From<&Preset> for VmEnvironmentEvidence {
    fn from(preset: &Preset) -> Self {
        Self {
            language: preset.location.language.clone(),
            browser_language: preset.location.browser_language.clone(),
            browser_platform: preset.device.browser_platform.clone(),
            tz_name: preset.location.tz_name.clone(),
            region: preset.location.region.clone(),
            screen_width: preset.screen.width,
            screen_height: preset.screen.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmTrace {
    pub product: TraceProduct,
    pub clock_ms: u64,
    pub bytecode_bytes: usize,
    pub opcode_table_slots: usize,
    pub string_table_slots: usize,
    pub numeric_constant_slots: usize,
    pub opcode_executions: usize,
    pub distinct_opcodes: usize,
    pub distinct_opcode_counts: BTreeMap<String, usize>,
    pub top_transitions: Vec<VmTransition>,
    pub function_invocations: usize,
    pub top_function_entries: Vec<VmFunctionEntry>,
    pub top_call_edges: Vec<VmTransition>,
    pub opcode_catalog: BTreeMap<String, VmOpcodeCatalogEntry>,
    pub vm_call_entries: BTreeMap<String, VmCallEntry>,
    pub vm_call_delta: BTreeMap<String, usize>,
    pub vm_call_sequence: Vec<VmCallReturn>,
    pub vm_call_invocation_sequence: Vec<VmCallReturn>,
    pub vm_call_inputs: Vec<VmCallInput>,
    pub vm_string_returns: Vec<VmStringReturn>,
    pub decoded_string_slots: BTreeMap<String, Vec<usize>>,
    pub decoded_string_uses: BTreeMap<String, Vec<VmDecodedStringUse>>,
    pub function_steps: Vec<VmFunctionStep>,
    pub register_trace: Vec<VmRegisterEvent>,
    pub sdk_call_returns: Vec<VmSdkCallReturn>,
    pub known_string_slots: BTreeMap<String, Vec<usize>>,
    pub mssdk_keys: Vec<String>,
    pub mssdk_functions: Vec<String>,
    pub mssdk_function_paths: Vec<String>,
    pub mssdk_own_function_paths: Vec<String>,
    pub mssdk_accessor_paths: Vec<String>,
    pub fetch_descriptor_installed: bool,
    pub fetch_assignments: Vec<VmFetchAssignment>,
    pub fetch_metadata_after_init: Option<VmFetchMetadata>,
    pub min_visited_offset: Option<usize>,
    pub max_visited_offset: Option<usize>,
    pub rolling_trace_hash: String,
    pub first_steps: Vec<VmStep>,
    pub last_steps: Vec<VmStep>,
    pub result_parameters: Vec<VmResultParameter>,
    pub field_events: Vec<VmFieldEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmTransition {
    pub edge: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmFunctionEntry {
    pub offset: usize,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmOpcodeCatalogEntry {
    pub source_bytes: usize,
    pub helper_reads: VmHelperReads,
    pub calls_vm: bool,
    pub reads_window: bool,
    pub reads_document: bool,
    pub reads_storage: bool,
    pub reads_crypto: bool,
    pub reads_fetch: bool,
    pub handler_tags: Vec<String>,
    pub visited: usize,
    pub operand_widths: BTreeMap<String, usize>,
    pub examples: Vec<VmOperandExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmCallEntry {
    pub calls: usize,
    pub types: BTreeMap<String, usize>,
    pub byte_lengths: BTreeMap<String, usize>,
    pub object_keys: Vec<String>,
    pub parents: BTreeMap<String, usize>,
    pub phases: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmCallReturn {
    pub entry: String,
    pub parent: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub bytes: usize,
    pub phase: String,
    pub object_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmCallInput {
    pub entry: usize,
    pub parent: String,
    pub args: Vec<VmValueShape>,
    pub this_value: VmValueShape,
    pub context_value: VmValueShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmValueShape {
    #[serde(rename = "type")]
    pub value_type: String,
    pub bytes: usize,
    pub value_class: Option<String>,
    pub object_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmStringReturn {
    pub entry: String,
    pub parent: String,
    pub bytes: usize,
    pub phase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmDecodedStringUse {
    pub slot: usize,
    pub function_entry: Option<usize>,
    pub entry: Option<usize>,
    pub opcode: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmFunctionStep {
    pub function_entry: usize,
    pub offset: usize,
    pub opcode: usize,
    pub width: usize,
    pub bytes: String,
    pub operands: Vec<VmOperandValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmOperandValue {
    pub kind: String,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmRegisterEvent {
    pub function_entry: usize,
    pub op: String,
    pub register: usize,
    #[serde(rename = "type")]
    pub value_type: String,
    pub bytes: usize,
    pub value_class: Option<String>,
    pub object_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmSdkCallReturn {
    pub name: String,
    pub fields: Vec<VmSdkField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmSdkField {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmFetchAssignment {
    pub assignment: usize,
    #[serde(flatten)]
    pub metadata: VmFetchMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmFetchMetadata {
    #[serde(rename = "type")]
    pub value_type: String,
    pub name: String,
    pub length: Option<usize>,
    pub phase: String,
    pub source_bytes: usize,
    pub contains_fetch: bool,
    pub contains_l: bool,
    pub contains_apply: bool,
    pub contains_call: bool,
    pub contains_arguments: bool,
    pub contains_new: bool,
    pub contains_window: bool,
    pub contains_url: bool,
    pub contains_search: bool,
    pub contains_query: bool,
    pub contains_header: bool,
    pub contains_set: bool,
    pub contains_append: bool,
    pub contains_crypto: bool,
    pub contains_bogus: bool,
    pub contains_gnarly: bool,
    pub contains_dynosaur: bool,
    pub contains_ms_token: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmHelperReads {
    #[serde(rename = "N")]
    pub n: usize,
    pub j: usize,
    pub x: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmOperandExample {
    pub offset: usize,
    pub width: usize,
    pub bytes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmStep {
    pub offset: usize,
    pub opcode: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmResultParameter {
    pub name: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmFieldEvent {
    pub action: String,
    pub name: String,
    pub bytes: usize,
    pub opcode: Option<usize>,
}
