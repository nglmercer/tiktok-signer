//! Backend contract shared by the HTTP server and signing implementations.
//!
//! The contract deliberately describes the observable transport result rather than how it
//! is produced. A backend may use the WebView oracle, deterministic fixtures, or a native
//! pipeline without exposing those implementation details to its consumers.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use crate::{SignError, SignOutcome};
use serde::{Deserialize, Serialize};

/// Boxed future used to keep [`SignerBackend`] object-safe on the workspace's MSRV.
pub type BackendFuture<'a> = Pin<Box<dyn Future<Output = SignOutcome> + Send + 'a>>;

/// Minimal input required by the sign server's transport endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransportRequest {
    pub room_id: String,
}

impl TransportRequest {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
        }
    }
}

/// Stable, non-secret identity metadata consumers need after signing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientIdentity {
    pub user_agent: String,
}

impl ClientIdentity {
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self {
            user_agent: user_agent.into(),
        }
    }
}

/// Produces signed transport metadata for a room.
///
/// This is intentionally capability-specific. Room discovery, gift lookup, browser
/// navigation, and research controls do not belong in the server's signing dependency.
pub trait SignerBackend: Send + Sync {
    fn transport(&self, request: TransportRequest) -> BackendFuture<'_>;

    /// Identity used for health reporting before a request has completed.
    fn identity(&self) -> ClientIdentity;
}

/// Deterministic backend for unit and HTTP contract tests.
///
/// Responses are keyed by room id. An unconfigured request fails explicitly instead of
/// inventing transport values or falling through to a network implementation.
#[derive(Debug, Clone)]
pub struct MockBackend {
    identity: ClientIdentity,
    responses: Arc<RwLock<HashMap<String, SignOutcome>>>,
}

impl MockBackend {
    pub fn new(identity: ClientIdentity) -> Self {
        Self {
            identity,
            responses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_response(self, room_id: impl Into<String>, outcome: SignOutcome) -> Self {
        self.set_response(room_id, outcome);
        self
    }

    pub fn set_response(&self, room_id: impl Into<String>, outcome: SignOutcome) {
        self.responses
            .write()
            .expect("mock response lock was poisoned")
            .insert(room_id.into(), outcome);
    }
}

impl SignerBackend for MockBackend {
    fn transport(&self, request: TransportRequest) -> BackendFuture<'_> {
        let response = self
            .responses
            .read()
            .expect("mock response lock was poisoned")
            .get(&request.room_id)
            .cloned()
            .unwrap_or_else(|| {
                SignOutcome::Transport(SignError::BackendUnavailable(format!(
                    "no mock response configured for room {}",
                    request.room_id
                )))
            });
        Box::pin(async move { response })
    }

    fn identity(&self) -> ClientIdentity {
        self.identity.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RejectReason;

    #[test]
    fn mock_responses_are_deterministic_and_missing_cases_fail() {
        let mock = MockBackend::new(ClientIdentity::new("fixture-agent"))
            .with_response("123", SignOutcome::Rejected(RejectReason::EmptyBody));

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let configured = runtime.block_on(mock.transport(TransportRequest::new("123")));
        assert!(matches!(
            configured,
            SignOutcome::Rejected(RejectReason::EmptyBody)
        ));

        let missing = runtime.block_on(mock.transport(TransportRequest::new("999")));
        assert!(matches!(
            missing,
            SignOutcome::Transport(SignError::BackendUnavailable(_))
        ));
    }
}
