//! One transport loop for native and mobile frontends. No FFI or signer engine.
use std::{future::Future, pin::Pin, time::Duration};
use tokio::sync::watch;
use ttl_live_events::EventBatch;
use ttl_live_ws::{ConnectConfig, LiveConnection, ReconnectPolicy};
use ttl_sign_core::{CookieJar, Preset};

pub type SignFuture<'a> = Pin<Box<dyn Future<Output = Result<String, ()>> + Send + 'a>>;
/// A warm signer handle; implementations own their engine and redact failures.
pub trait SocketSigner {
    fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Offline,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Signer,
    Socket,
    Timeout,
    Decode,
    Protocol,
    Auth(u16),
    SocketStatus(u16),
}
pub trait LiveSink {
    fn on_state(&self, state: State, detail: &str);
    fn on_error(&self, error: Error, retryable: bool);
    fn on_batch(&self, batch: EventBatch);
}
/// Resolves immediately even if cancellation was signaled before this future.
pub async fn cancelled(stop: &mut watch::Receiver<bool>) {
    if *stop.borrow() {
        return;
    }
    while stop.changed().await.is_ok() {
        if *stop.borrow() {
            return;
        }
    }
}
/// Run one stream. The caller owns its thread/runtime. `max_retries` excludes
/// the initial connection; a connected socket resets the failure budget.
pub async fn run(
    signer: &impl SocketSigner,
    cookies: &CookieJar,
    preset: &Preset,
    room: &str,
    max_retries: u32,
    sink: &impl LiveSink,
    mut stop: watch::Receiver<bool>,
) {
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());
    let policy = ReconnectPolicy::default();
    let user_agent = preset.user_agent();
    let config = ConnectConfig::default();
    let mut failures = 0u32;
    let mut reconnecting = false;
    loop {
        if *stop.borrow() {
            break;
        }
        sink.on_state(
            if reconnecting {
                State::Reconnecting
            } else {
                State::Connecting
            },
            "opening LIVE stream",
        );
        let url = ttl_sign_core::DirectSocketParams::new(room).url(preset);
        let signed = tokio::select! {
            biased;
            _ = cancelled(&mut stop) => break,
            result = tokio::time::timeout(Duration::from_secs(15), signer.sign(&url)) => match result {
                Ok(Ok(url)) => url,
                _ => { sink.on_error(Error::Signer,false); sink.on_state(State::Error,"signing failed"); break; }
            }
        };
        let opened = tokio::select! {
            biased;
            _ = cancelled(&mut stop) => break,
            result = tokio::time::timeout(Duration::from_secs(15), LiveConnection::open_uri(&signed,cookies,&user_agent,"",&config)) => result
        };
        match opened {
            Ok(Ok(mut connection)) => {
                failures = 0;
                sink.on_state(State::Connected, "LIVE stream open");
                loop {
                    let message = tokio::select! {
                        biased;
                        _ = cancelled(&mut stop) => {
                            let _ = tokio::time::timeout(Duration::from_millis(500),connection.close()).await;
                            sink.on_state(State::Disconnected,"disconnect requested"); return;
                        },
                        message = connection.next_message() => message,
                    };
                    match message {
                        Some(Ok(message)) => {
                            match ttl_live_events::decode_batch(&message.payload) {
                                Ok(batch) => sink.on_batch(batch),
                                Err(_) => sink.on_error(Error::Decode, false),
                            }
                        }
                        Some(Err(_)) => {
                            sink.on_error(Error::Socket, max_retries > 0);
                            break;
                        }
                        None => break,
                    }
                }
            }
            Ok(Err(ttl_live_ws::WsError::Blocked200(_))) => {
                sink.on_error(Error::Auth(200), false);
                sink.on_state(State::Error, "handshake refused");
                break;
            }
            Ok(Err(ttl_live_ws::WsError::EmptyCookies)) => {
                sink.on_error(Error::Auth(0), false);
                sink.on_state(State::Error, "session cookies missing");
                break;
            }
            Ok(Err(ttl_live_ws::WsError::HttpStatus(status @ (401 | 403)))) => {
                sink.on_error(Error::Auth(status), false);
                sink.on_state(State::Error, "handshake refused");
                break;
            }
            Ok(Err(ttl_live_ws::WsError::HttpStatus(status))) => {
                sink.on_error(Error::SocketStatus(status), failures < max_retries)
            }
            Ok(Err(_)) => sink.on_error(Error::Socket, failures < max_retries),
            Err(_) => sink.on_error(Error::Timeout, failures < max_retries),
        }
        if failures >= max_retries {
            sink.on_state(State::Error, "reconnect budget exhausted");
            break;
        }
        failures += 1;
        reconnecting = true;
        sink.on_state(State::Reconnecting, "waiting to retry");
        tokio::select! { biased; _ = cancelled(&mut stop) => break, _ = tokio::time::sleep(policy.backoff(failures)) => () }
    }
    sink.on_state(State::Disconnected, "stream stopped");
}
