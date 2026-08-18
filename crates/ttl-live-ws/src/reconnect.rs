//! Staying connected.
//!
//! A socket signature ages out, rooms restart their push servers, and TikTok closes connections it
//! considers stale. [`LiveConnection`] deliberately does none of that thinking: it reports the close
//! reason and stops. Something has to decide what happens next, and until now nothing did — every
//! caller either ran for a fixed few seconds or was expected to invent its own policy.
//!
//! [`ReconnectingConnection`] is that decision, made once. It takes a [`SignerBackend`] — the same
//! one the sign server uses, so a fix to the signing path reaches both — and re-derives a fresh
//! signed URI on every attempt. Re-signing is the whole point: reopening the *same* URI after its
//! signature has aged is the failure this is meant to avoid, not a shortcut worth taking.
//!
//! What it will not do is retry forever, or retry something that cannot succeed. A rejection is a
//! verdict about the request, not a transient fault; retrying it produces a loop that looks like a
//! connection problem and is really a signing one.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, info, warn};

use ttl_sign_core::{RejectReason, SignError, SignOutcome, SignerBackend, TransportRequest};

use crate::{ConnectConfig, LiveConnection, LiveMessage, WsError};

/// How hard to try, and how long to wait between attempts.
///
/// The defaults are deliberately patient rather than aggressive: a room that closed our socket is
/// not helped by an immediate reconnect, and a signing subprocess costs seconds of its own.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    /// Consecutive failed attempts before giving up. `0` disables reconnection entirely.
    pub max_attempts: u32,
    /// Delay before the first retry. Doubles per consecutive failure.
    pub initial_backoff: Duration,
    /// Ceiling on that doubling.
    pub max_backoff: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(60),
        }
    }
}

impl ReconnectPolicy {
    /// Never reconnect: one connection, and the caller handles the close.
    pub fn none() -> Self {
        Self {
            max_attempts: 0,
            ..Self::default()
        }
    }

    /// Delay before the `attempt`-th consecutive retry, counting from 1.
    ///
    /// Exposed because a policy whose timing cannot be asserted is a policy nobody can review.
    pub fn backoff(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return self.initial_backoff;
        }
        let doubled = self
            .initial_backoff
            .checked_mul(1u32 << (attempt - 1).min(16))
            .unwrap_or(self.max_backoff);
        doubled.min(self.max_backoff)
    }
}

/// Why a stream stopped for good.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// The signer or the service refused. Not retried: the next attempt would be refused too.
    #[error("the transport was refused: {0}")]
    Refused(RejectReason),

    /// The backend could not produce a signature at all.
    #[error("the signer failed: {0}")]
    Signer(SignError),

    /// Reconnection ran out of attempts. Carries the failure that ended the last one.
    #[error("gave up after {attempts} reconnection attempt(s): {last}")]
    Exhausted { attempts: u32, last: WsError },

    /// The first connection never opened.
    #[error(transparent)]
    Connect(WsError),
}

/// A message stream that outlives any single socket.
///
/// Use it exactly like [`LiveConnection`]: call [`ReconnectingConnection::next_message`] in a loop.
/// The difference is what happens on a close — instead of ending the stream, it re-signs, reopens,
/// and keeps going until the policy runs out.
pub struct ReconnectingConnection {
    backend: Arc<dyn SignerBackend>,
    room_id: String,
    config: ConnectConfig,
    policy: ReconnectPolicy,
    connection: Option<LiveConnection>,
    reconnects: u64,
}

impl ReconnectingConnection {
    /// Open the first connection, or fail with the reason it could not open.
    pub async fn open(
        backend: Arc<dyn SignerBackend>,
        room_id: impl Into<String>,
        config: ConnectConfig,
        policy: ReconnectPolicy,
    ) -> Result<Self, StreamError> {
        let room_id = room_id.into();
        let connection = connect_once(backend.as_ref(), &room_id, &config).await?;
        Ok(Self {
            backend,
            room_id,
            config,
            policy,
            connection: Some(connection),
            reconnects: 0,
        })
    }

    /// How many times the stream has re-established itself. Worth logging: a climbing count with
    /// messages still arriving is a healthy stream on a flaky path, and one without is not.
    pub fn reconnects(&self) -> u64 {
        self.reconnects
    }

    /// The next event frame, reconnecting as needed.
    ///
    /// Returns `None` only when the stream is finished and will not resume.
    pub async fn next_message(&mut self) -> Option<Result<LiveMessage, StreamError>> {
        loop {
            let connection = self.connection.as_mut()?;
            match connection.next_message().await {
                Some(Ok(message)) => return Some(Ok(message)),
                // A clean end and a closed socket are the same thing here: the socket is gone and
                // the only question is whether to build another.
                None => {
                    debug!(room_id = %self.room_id, "socket closed cleanly");
                    match self.reconnect(WsError::Closed("closed by peer".into())).await {
                        Ok(()) => continue,
                        Err(error) => {
                            self.connection = None;
                            return Some(Err(error));
                        }
                    }
                }
                Some(Err(error)) => match self.reconnect(error).await {
                    Ok(()) => continue,
                    Err(error) => {
                        self.connection = None;
                        return Some(Err(error));
                    }
                },
            }
        }
    }

    /// Re-sign and reopen, backing off between attempts.
    async fn reconnect(&mut self, cause: WsError) -> Result<(), StreamError> {
        self.connection = None;
        if self.policy.max_attempts == 0 {
            return Err(StreamError::Exhausted {
                attempts: 0,
                last: cause,
            });
        }

        warn!(room_id = %self.room_id, %cause, "connection lost; reconnecting");
        let mut last = cause;
        for attempt in 1..=self.policy.max_attempts {
            let delay = self.policy.backoff(attempt);
            debug!(room_id = %self.room_id, attempt, ?delay, "waiting before reconnect");
            tokio::time::sleep(delay).await;

            match connect_once(self.backend.as_ref(), &self.room_id, &self.config).await {
                Ok(connection) => {
                    self.connection = Some(connection);
                    self.reconnects += 1;
                    info!(
                        room_id = %self.room_id,
                        attempt,
                        reconnects = self.reconnects,
                        "reconnected"
                    );
                    return Ok(());
                }
                // A refusal will be refused again. Failing here, with the reason, beats spending
                // the remaining attempts to arrive at the same place with a timing error instead.
                Err(error @ (StreamError::Refused(_) | StreamError::Signer(_))) => return Err(error),
                Err(StreamError::Connect(error)) => {
                    warn!(room_id = %self.room_id, attempt, %error, "reconnect attempt failed");
                    last = error;
                }
                Err(other) => return Err(other),
            }
        }

        Err(StreamError::Exhausted {
            attempts: self.policy.max_attempts,
            last,
        })
    }

    /// Close the current socket, if one is open.
    pub async fn close(mut self) {
        if let Some(connection) = self.connection.take() {
            connection.close().await;
        }
    }
}

/// Sign a fresh URI and open it. One attempt, no retries, no waiting.
async fn connect_once(
    backend: &dyn SignerBackend,
    room_id: &str,
    config: &ConnectConfig,
) -> Result<LiveConnection, StreamError> {
    let signed = match backend.transport(TransportRequest::new(room_id)).await {
        SignOutcome::Ok(signed) => signed,
        SignOutcome::Rejected(reason) => return Err(StreamError::Refused(reason)),
        SignOutcome::Transport(error) => return Err(StreamError::Signer(error)),
    };
    LiveConnection::open_uri(
        &signed.signed_url,
        &signed.cookies,
        &signed.user_agent,
        "",
        config,
    )
    .await
    .map_err(StreamError::Connect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::{BackendFuture, ClientIdentity};

    /// A backend that refuses, so no test here can reach the network.
    struct RefusingBackend;

    impl SignerBackend for RefusingBackend {
        fn transport(&self, _request: TransportRequest) -> BackendFuture<'_> {
            Box::pin(async { SignOutcome::Rejected(RejectReason::EmptyBody) })
        }

        fn identity(&self) -> ClientIdentity {
            ClientIdentity::new("test")
        }
    }

    /// The timing is the policy, so it is asserted rather than described.
    #[test]
    fn backoff_doubles_and_then_stops_doubling() {
        let policy = ReconnectPolicy {
            max_attempts: 8,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(16),
        };
        assert_eq!(policy.backoff(1), Duration::from_secs(2));
        assert_eq!(policy.backoff(2), Duration::from_secs(4));
        assert_eq!(policy.backoff(3), Duration::from_secs(8));
        assert_eq!(policy.backoff(4), Duration::from_secs(16));
        // Capped, not overflowing, however many attempts a caller allows.
        assert_eq!(policy.backoff(30), Duration::from_secs(16));
    }

    #[tokio::test]
    async fn a_refusal_is_reported_rather_than_retried() {
        let opened = ReconnectingConnection::open(
            Arc::new(RefusingBackend),
            "7000000000000000000",
            ConnectConfig::default(),
            ReconnectPolicy::default(),
        )
        .await;
        let Err(error) = opened else {
            panic!("a refusing backend cannot open a connection");
        };
        assert!(matches!(
            error,
            StreamError::Refused(RejectReason::EmptyBody)
        ));
    }

    /// `max_attempts: 0` has to mean "do not reconnect", not "reconnect forever".
    #[test]
    fn a_disabled_policy_reconnects_zero_times() {
        assert_eq!(ReconnectPolicy::none().max_attempts, 0);
    }

    /// A backend that signs successfully but points at a closed local port, so the retry loop runs
    /// for real — attempts, backoff, exhaustion — without reaching the network.
    struct DeadPortBackend {
        attempts: Arc<std::sync::atomic::AtomicU32>,
    }

    impl SignerBackend for DeadPortBackend {
        fn transport(&self, _request: TransportRequest) -> BackendFuture<'_> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(async {
                SignOutcome::Ok(ttl_sign_core::SignedFetch {
                    protobuf: Vec::new(),
                    cookies: ttl_sign_core::CookieJar::parse("sessionid=test"),
                    user_agent: "test".into(),
                    // Port 1 on loopback: refused immediately, and never leaves the machine.
                    signed_url: "wss://127.0.0.1:1/webcast/im/ws_proxy/ws_reuse_supplement/\
?room_id=7000000000000000000"
                        .into(),
                })
            })
        }

        fn identity(&self) -> ClientIdentity {
            ClientIdentity::new("test")
        }
    }

    /// Every attempt is spent, each one re-signing, and the failure names how many it took.
    #[tokio::test]
    async fn a_dead_socket_is_retried_and_then_reported() {
        let attempts = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let backend = Arc::new(DeadPortBackend {
            attempts: Arc::clone(&attempts),
        });
        let policy = ReconnectPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(4),
        };

        // The first connection cannot open either, which is a Connect error rather than a stream
        // that later fails — the retry loop is reached through `reconnect`, so drive it directly.
        let mut stream = ReconnectingConnection {
            backend,
            room_id: "7000000000000000000".into(),
            config: ConnectConfig::default(),
            policy,
            connection: None,
            reconnects: 0,
        };
        let error = stream
            .reconnect(WsError::Closed("test".into()))
            .await
            .expect_err("a dead port cannot be reconnected to");

        match error {
            StreamError::Exhausted { attempts: spent, .. } => assert_eq!(spent, 3),
            other => panic!("expected exhaustion, got {other}"),
        }
        // Re-signed once per attempt: reusing a stale URI is the failure this exists to avoid.
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 3);
    }
}
