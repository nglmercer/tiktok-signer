//! Native/headless signing pipeline.
//!
//! Algorithm research is injected through [`SigningAlgorithm`]. The surrounding stages are
//! deterministic and independently testable: request building, parameter normalization,
//! environment construction, signing context, and transport reconstruction.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ttl_sign_core::{
    BackendFuture, ClientIdentity, CookieJar, FetchParams, FetchResult, Preset, Query,
    RejectReason, SignError, SignOutcome, SignedFetch, SignerBackend, TransportRequest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRequest {
    pub room_id: String,
    pub device_id: String,
    pub contact_us: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedQuery {
    entries: Vec<(String, String)>,
    encoded: String,
}

impl OrderedQuery {
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEnvironment {
    pub preset: Preset,
    pub user_agent: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningContext {
    /// The page has multiple signing entry points; the transport requires the fetch patch.
    pub product: SigningProduct,
    pub query: OrderedQuery,
    pub user_agent: String,
    pub timestamp_ms: u64,
    pub environment: DeviceEnvironment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningProduct {
    /// Public helper observed to return only a 16-byte `X-Bogus` value.
    FrontierSign,
    /// Patched-fetch product that appends the complete four-parameter signing suffix.
    FetchPatch,
}

pub const FETCH_PATCH_PARAMETER_ORDER: [&str; 4] = ["X-Dynosaur", "msToken", "X-Bogus", "X-Gnarly"];
/// The fetch composition trace confirms that this field is a literal, not a generated value.
pub const FETCH_X_BOGUS_VALUE: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmedParameter {
    pub name: &'static str,
    pub value: &'static str,
}

pub const FETCH_CONFIRMED_PARAMETERS: [ConfirmedParameter; 1] = [ConfirmedParameter {
    name: "X-Bogus",
    value: FETCH_X_BOGUS_VALUE,
}];

/// No confirmed constants are available for the public frontier helper.
pub const FRONTIER_CONFIRMED_PARAMETERS: [ConfirmedParameter; 0] = [];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureMaterial {
    pub push_server: String,
    pub route_params: Vec<(String, String)>,
    pub cursor: String,
    pub internal_ext: String,
    pub heartbeat_duration: u64,
    pub need_ack: bool,
    pub signed_url: String,
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Clone)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

/// Isolated signing transformation. It has no network or browser access.
///
/// A production implementation must honor [`SigningContext::product`]. The public
/// `frontierSign` helper is not a substitute for [`SigningProduct::FetchPatch`].
pub trait SigningAlgorithm: Send + Sync {
    fn sign(&self, context: &SigningContext) -> Result<SignatureMaterial, SignError>;
}

impl SigningContext {
    /// Return only deterministic fetch parameters confirmed by the sanitized VM trace.
    ///
    /// This is an assembly-stage fact, not a complete signing result. Dynamic
    /// X-Dynosaur, msToken, and X-Gnarly values still belong to the algorithm boundary.
    pub fn confirmed_parameters(&self) -> &'static [ConfirmedParameter] {
        match self.product {
            SigningProduct::FetchPatch => &FETCH_CONFIRMED_PARAMETERS,
            SigningProduct::FrontierSign => &FRONTIER_CONFIRMED_PARAMETERS,
        }
    }
}

/// Explicit placeholder used until an algorithm is selected.
#[derive(Debug, Default)]
pub struct UnsupportedAlgorithm;

impl SigningAlgorithm for UnsupportedAlgorithm {
    fn sign(&self, _context: &SigningContext) -> Result<SignatureMaterial, SignError> {
        Err(SignError::BackendUnavailable(
            "native signing algorithm is not configured".into(),
        ))
    }
}

/// Deterministic algorithm double for native pipeline and contract tests.
#[derive(Debug, Clone, Default)]
pub struct StaticAlgorithm {
    responses: HashMap<String, Result<SignatureMaterial, SignError>>,
}

impl StaticAlgorithm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_response(
        mut self,
        room_id: impl Into<String>,
        response: Result<SignatureMaterial, SignError>,
    ) -> Self {
        self.responses.insert(room_id.into(), response);
        self
    }
}

impl SigningAlgorithm for StaticAlgorithm {
    fn sign(&self, context: &SigningContext) -> Result<SignatureMaterial, SignError> {
        let room_id = context.query.get("room_id").unwrap_or_default();
        self.responses.get(room_id).cloned().unwrap_or_else(|| {
            Err(SignError::BackendUnavailable(format!(
                "no native test material for room {room_id}"
            )))
        })
    }
}

pub struct NativeConfig {
    pub preset: Preset,
    pub device_id: String,
    pub contact_us: String,
    pub cookies: CookieJar,
    pub clock: Arc<dyn Clock>,
}

impl NativeConfig {
    pub fn new(
        preset: Preset,
        device_id: impl Into<String>,
        cookies: CookieJar,
        clock: impl Clock + 'static,
    ) -> Self {
        Self {
            preset,
            device_id: device_id.into(),
            contact_us: String::new(),
            cookies,
            clock: Arc::new(clock),
        }
    }
}

#[derive(Debug, Default)]
pub struct RequestBuilder;

impl RequestBuilder {
    pub fn build(
        &self,
        request: TransportRequest,
        config: &NativeConfig,
    ) -> Result<NativeRequest, SignError> {
        if request.room_id.is_empty() || !request.room_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(SignError::Decode("room_id must be numeric".into()));
        }
        if config.device_id.len() != 19 || !config.device_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(SignError::Decode(
                "native device_id must contain exactly 19 digits".into(),
            ));
        }
        Ok(NativeRequest {
            room_id: request.room_id,
            device_id: config.device_id.clone(),
            contact_us: config.contact_us.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub struct ParameterNormalizer;

impl ParameterNormalizer {
    pub fn normalize(&self, request: &NativeRequest, preset: &Preset) -> OrderedQuery {
        let params = FetchParams {
            room_id: request.room_id.clone(),
            device_id: request.device_id.clone(),
            cursor: String::new(),
            internal_ext: String::new(),
            contact_us: request.contact_us.clone(),
            sup_ws_ds_opt: 1,
        };
        let query: Query = params.build(preset);
        OrderedQuery {
            entries: query
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            encoded: query.encode(),
        }
    }
}

#[derive(Debug, Default)]
pub struct EnvironmentBuilder;

impl EnvironmentBuilder {
    pub fn build(&self, preset: &Preset) -> DeviceEnvironment {
        DeviceEnvironment {
            preset: preset.clone(),
            user_agent: preset.user_agent(),
        }
    }
}

#[derive(Debug, Default)]
pub struct TransportBuilder;

impl TransportBuilder {
    pub fn build(
        &self,
        material: SignatureMaterial,
        cookies: CookieJar,
        user_agent: String,
    ) -> SignOutcome {
        let fetch = FetchResult {
            push_server: material.push_server,
            route_params: material.route_params,
            cursor: material.cursor,
            internal_ext: material.internal_ext,
            heartbeat_duration: material.heartbeat_duration,
            need_ack: material.need_ack,
        };
        if fetch.rejection_reason() == Some(RejectReason::EmptyPushServer) {
            return SignOutcome::Rejected(RejectReason::EmptyPushServer);
        }
        SignOutcome::Ok(SignedFetch {
            protobuf: fetch.encode(),
            cookies,
            user_agent,
            signed_url: material.signed_url,
        })
    }
}

pub struct NativeBackend {
    config: NativeConfig,
    algorithm: Arc<dyn SigningAlgorithm>,
    request_builder: RequestBuilder,
    parameter_normalizer: ParameterNormalizer,
    environment_builder: EnvironmentBuilder,
    transport_builder: TransportBuilder,
}

impl NativeBackend {
    pub fn new(config: NativeConfig, algorithm: impl SigningAlgorithm + 'static) -> Self {
        Self {
            config,
            algorithm: Arc::new(algorithm),
            request_builder: RequestBuilder,
            parameter_normalizer: ParameterNormalizer,
            environment_builder: EnvironmentBuilder,
            transport_builder: TransportBuilder,
        }
    }

    pub fn unsupported(config: NativeConfig) -> Self {
        Self::new(config, UnsupportedAlgorithm)
    }

    /// Run deterministic pre-signing stages for L0/L1 differential tests.
    pub fn prepare(&self, request: TransportRequest) -> Result<SigningContext, SignError> {
        let request = self.request_builder.build(request, &self.config)?;
        let environment = self.environment_builder.build(&self.config.preset);
        let query = self
            .parameter_normalizer
            .normalize(&request, &self.config.preset);
        Ok(SigningContext {
            product: SigningProduct::FetchPatch,
            query,
            user_agent: environment.user_agent.clone(),
            timestamp_ms: self.config.clock.now_ms(),
            environment,
        })
    }

    fn execute(&self, request: TransportRequest) -> SignOutcome {
        let context = match self.prepare(request) {
            Ok(context) => context,
            Err(error) => return SignOutcome::Transport(error),
        };
        let material = match self.algorithm.sign(&context) {
            Ok(material) => material,
            Err(error) => return SignOutcome::Transport(error),
        };
        self.transport_builder
            .build(material, self.config.cookies.clone(), context.user_agent)
    }
}

impl SignerBackend for NativeBackend {
    fn transport(&self, request: TransportRequest) -> BackendFuture<'_> {
        Box::pin(async move { self.execute(request) })
    }

    fn identity(&self) -> ClientIdentity {
        ClientIdentity::new(self.config.preset.user_agent())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::{DevicePreset, LocationPreset, ScreenPreset};

    const ROOM: &str = "7300000000000000001";
    const DEVICE_ID: &str = "7123456789012345678";

    fn preset() -> Preset {
        Preset::new(
            DevicePreset::chrome_linux(),
            LocationPreset::us_east(),
            ScreenPreset::FHD,
        )
    }

    fn material() -> SignatureMaterial {
        SignatureMaterial {
            push_server: "wss://fixture.invalid/ws/".into(),
            route_params: vec![("wrss".into(), "fixture-route".into())],
            cursor: "fixture-cursor".into(),
            internal_ext: "fixture-internal".into(),
            heartbeat_duration: 10_000,
            need_ack: true,
            signed_url: "wss://fixture.invalid/ws/?signed=fixture".into(),
        }
    }

    fn backend(algorithm: impl SigningAlgorithm + 'static) -> NativeBackend {
        NativeBackend::new(
            NativeConfig::new(
                preset(),
                DEVICE_ID,
                CookieJar::parse("msToken=fixture-token"),
                FixedClock(1_700_000_000_000),
            ),
            algorithm,
        )
    }

    #[test]
    fn preparation_is_deterministic_and_preserves_canonical_order() {
        let native = backend(UnsupportedAlgorithm);
        let first = native.prepare(TransportRequest::new(ROOM)).unwrap();
        let second = native.prepare(TransportRequest::new(ROOM)).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.timestamp_ms, 1_700_000_000_000);
        assert_eq!(first.product, SigningProduct::FetchPatch);
        assert_eq!(
            first.confirmed_parameters(),
            &[ConfirmedParameter {
                name: "X-Bogus",
                value: "1"
            }]
        );
        assert_eq!(
            FETCH_PATCH_PARAMETER_ORDER,
            ["X-Dynosaur", "msToken", "X-Bogus", "X-Gnarly"]
        );
        assert_eq!(first.query.get("room_id"), Some(ROOM));
        assert_eq!(first.query.get("device_id"), Some(DEVICE_ID));
        assert!(first
            .query
            .encoded()
            .contains("room_id=7300000000000000001"));
        assert_eq!(
            first.user_agent,
            first.environment.preset.user_agent(),
            "query environment and UA share one preset"
        );
    }

    #[tokio::test]
    async fn static_algorithm_passes_the_backend_success_contract() {
        let native = backend(StaticAlgorithm::new().with_response(ROOM, Ok(material())));
        let outcome = native.transport(TransportRequest::new(ROOM)).await;
        let signed = outcome.ok().expect("native success");
        let fetch = FetchResult::decode(&signed.protobuf).unwrap();
        assert_eq!(fetch.cursor, "fixture-cursor");
        assert_eq!(signed.cookies.get("msToken"), Some("fixture-token"));
        assert_eq!(signed.user_agent, preset().user_agent());
    }

    #[tokio::test]
    async fn incomplete_algorithm_is_an_explicit_error() {
        let outcome = backend(UnsupportedAlgorithm)
            .transport(TransportRequest::new(ROOM))
            .await;
        assert!(matches!(
            outcome,
            SignOutcome::Transport(SignError::BackendUnavailable(_))
        ));
    }

    /// The default production constructor must fail loudly rather than silently degrade.
    ///
    /// A silent degradation would be any of: an `Ok` with fabricated transport, or a
    /// `Rejected` outcome (which a caller reasonably reads as "the server refused a valid
    /// request", e.g. room closed). Neither is acceptable while the algorithm is unimplemented:
    /// the only correct outcome is an explicit backend error.
    #[tokio::test]
    async fn unsupported_backend_never_produces_signed_or_rejected_output() {
        // A spread of room ids: the failure must not depend on the specific input.
        let rooms = [
            "7300000000000000001",
            "7300000000000000002",
            "7000000000000000000",
            "7999999999999999999",
        ];
        for room in rooms {
            let outcome = NativeBackend::unsupported(NativeConfig::new(
                preset(),
                DEVICE_ID,
                CookieJar::parse("msToken=fixture-token"),
                FixedClock(1_700_000_000_000),
            ))
            .transport(TransportRequest::new(room))
            .await;

            match outcome {
                SignOutcome::Transport(SignError::BackendUnavailable(_)) => {}
                SignOutcome::Ok(_) => {
                    panic!("unsupported native backend fabricated a signed transport for {room}")
                }
                SignOutcome::Rejected(reason) => panic!(
                    "unsupported native backend silently degraded to a rejection ({reason:?}) \
                     for {room}; callers would mistake this for a legitimate server refusal"
                ),
                SignOutcome::Transport(other) => {
                    panic!("unexpected error class for {room}: {other:?}")
                }
            }
        }
    }

    /// The confirmed-constant surface must stay minimal: only the one assembly-stage fact
    /// (`X-Bogus=1`) is encoded. If a future edit tries to hardcode a guessed `msToken`,
    /// `X-Dynosaur`, or `X-Gnarly` value as a "confirmed" constant, this fails.
    #[test]
    fn confirmed_parameters_encode_no_dynamic_signing_fields() {
        let context = backend(UnsupportedAlgorithm)
            .prepare(TransportRequest::new(ROOM))
            .unwrap();

        assert_eq!(
            context.confirmed_parameters(),
            &[ConfirmedParameter {
                name: "X-Bogus",
                value: "1"
            }],
            "only the confirmed X-Bogus=1 assembly constant may be encoded"
        );

        // The dynamic fields are known to exist in the suffix order, but must never appear as
        // confirmed constants until they converge against authorized oracle observations.
        for dynamic in ["msToken", "X-Dynosaur", "X-Gnarly"] {
            assert!(
                FETCH_PATCH_PARAMETER_ORDER.contains(&dynamic),
                "{dynamic} should remain a documented suffix field"
            );
            assert!(
                !FETCH_CONFIRMED_PARAMETERS.iter().any(|p| p.name == dynamic),
                "{dynamic} must not be encoded as a confirmed constant"
            );
        }

        // The public frontier route has no confirmed constants at all.
        let mut frontier = context;
        frontier.product = SigningProduct::FrontierSign;
        assert_eq!(
            frontier.confirmed_parameters(),
            &[],
            "the public frontierSign route must expose no confirmed constants"
        );
    }

    /// Deterministic pre-signing stages are allowed to succeed; only the algorithm is
    /// unsupported. This documents exactly where the boundary sits: preparation is real work,
    /// signing is the unimplemented step.
    #[tokio::test]
    async fn preparation_succeeds_but_signing_is_the_unsupported_boundary() {
        let native = backend(UnsupportedAlgorithm);
        // L0/L1 preparation is deterministic and does not fail.
        let context = native.prepare(TransportRequest::new(ROOM)).unwrap();
        assert_eq!(context.product, SigningProduct::FetchPatch);
        // The algorithm boundary is what refuses.
        let outcome = native.transport(TransportRequest::new(ROOM)).await;
        assert!(matches!(
            outcome,
            SignOutcome::Transport(SignError::BackendUnavailable(_))
        ));
    }

    /// A malformed request fails at the deterministic stage as a transport error, never as a
    /// success or a silent rejection.
    #[tokio::test]
    async fn invalid_request_fails_loudly_before_signing() {
        let native = backend(StaticAlgorithm::new().with_response(ROOM, Ok(material())));
        // Non-numeric room id is rejected by the request builder.
        let outcome = native.transport(TransportRequest::new("not-a-room")).await;
        assert!(matches!(
            outcome,
            SignOutcome::Transport(SignError::Decode(_))
        ));
    }

    #[tokio::test]
    async fn incomplete_transport_is_a_rejection_not_a_success() {
        let mut incomplete = material();
        incomplete.route_params.clear();
        let outcome = backend(StaticAlgorithm::new().with_response(ROOM, Ok(incomplete)))
            .transport(TransportRequest::new(ROOM))
            .await;
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyPushServer)
        ));
    }
}
