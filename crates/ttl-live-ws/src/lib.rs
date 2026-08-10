//! TikTok LIVE WebSocket client.
//!
//! Consumes a `SignedFetch` and opens the connection. **It does not sign anything**:
//! `route_params` are already signed by TikTok inside the protobuf.
//!
//! Two things this crate deliberately **does not** do:
//!
//! - **It does not reconnect.** Parameters expire after about 30 seconds, so reconnecting
//!   means restarting the `/webcast/im/fetch/` flow. The orchestrator decides what to do
//!   with the reported close reason.
//! - **It does not parse events.** It returns decompressed `msg` payloads; event schemas
//!   belong to the consumer.

use std::time::Duration;

use flate2::read::GzDecoder;
use futures_util::{SinkExt, StreamExt};
use std::io::Read;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, warn};

use ttl_sign_core::proto::PushFrame;
use ttl_sign_core::{CookieJar, FetchResult, Preset, SignedFetch, WsParams};

const DEFAULT_HEARTBEAT_SECONDS: u64 = 10;
const INITIAL_HEARTBEAT_SEQUENCE: u64 = 1;
const HTTP_OK: u16 = 200;
#[cfg(test)]
const HEARTBEAT_DURATION_MILLISECONDS: u64 = 10_000;

/// Connection failures. `Blocked200` is intentionally separate: it is not transient and
/// retrying is counterproductive.
#[derive(Debug, thiserror::Error)]
pub enum WsError {
    /// The protobuf did not contain a `cursor`, so the response is invalid.
    #[error("response does not contain an initial cursor")]
    InitialCursorMissing,

    /// Empty `push_server` or `route_params`: TikTok rejected the request silently.
    #[error("response does not contain a WebSocket URL (silent rejection)")]
    WebsocketUrlMissing,

    /// The URI does not identify the room for the `im_enter_room` message.
    #[error("WebSocket URI does not contain a valid room_id")]
    RoomIdMissing,

    /// The cookie jar lacks the cookies required by the WebSocket.
    #[error("session cookies are missing for the WebSocket")]
    EmptyCookies,

    /// A 200 handshake means **detection**. Do not retry.
    #[error("TikTok rejected the handshake (HTTP {HTTP_OK}){}", .0.as_ref().map(|m| format!(": {m}")).unwrap_or_default())]
    Blocked200(Option<String>),

    #[error("could not decode protobuf: {0}")]
    Decode(String),

    #[error("transport error: {0}")]
    Transport(String),

    /// The connection closed. This can be expiry after the signature becomes stale.
    #[error("connection closed: {0}")]
    Closed(String),
}

/// A `msg` frame ready to parse.
#[derive(Debug, Clone)]
pub struct LiveMessage {
    pub log_id: u64,
    /// Decompressed payload. It contains a `WebcastResponse`; parsing belongs to the consumer.
    pub payload: Vec<u8>,
}

/// Connection options.
#[derive(Debug, Clone)]
pub struct ConnectConfig {
    /// Matches the `heartbeat_duration=10000` query parameter.
    pub heartbeat: Duration,
    /// Request compressed frames. Empty means no compression.
    pub compress: String,
}

impl Default for ConnectConfig {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(DEFAULT_HEARTBEAT_SECONDS),
            compress: "gzip".into(),
        }
    }
}

/// An open connection, consumed with [`LiveConnection::next_message`].
pub struct LiveConnection {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    internal_ext: String,
    room_id: u64,
    heartbeat_sequence: u64,
    heartbeat: tokio::time::Interval,
    /// Options returned by the server in `Handshake-Options`, useful for diagnostics.
    handshake_options: Vec<(String, String)>,
    uri: String,
}

impl LiveConnection {
    /// Open a connection from a valid signature.
    ///
    /// Validate **before** touching the network: an empty `push_server` in a 200 means
    /// TikTok rejected the request, and connecting will not fix it.
    pub async fn open(
        signed: &SignedFetch,
        preset: &Preset,
        room_id: &str,
        config: &ConnectConfig,
    ) -> Result<Self, WsError> {
        let result =
            FetchResult::decode(&signed.protobuf).map_err(|e| WsError::Decode(e.to_string()))?;
        Self::open_with(
            &result,
            &signed.cookies,
            &signed.user_agent,
            preset,
            room_id,
            config,
        )
        .await
    }

    /// Open a URI already built **and signed** by the caller.
    ///
    /// This is the real path: the WebSocket URI has its own signature
    /// (`byted_acrawler.frontierSign`), and the webview engine—not this crate—knows how to
    /// create it.
    pub async fn open_uri(
        uri: &str,
        cookies: &CookieJar,
        user_agent: &str,
        internal_ext: &str,
        config: &ConnectConfig,
    ) -> Result<Self, WsError> {
        if cookies.is_empty() {
            return Err(WsError::EmptyCookies);
        }
        let room_id = room_id_from_uri(uri)?;
        Self::connect(
            uri.to_string(),
            cookies,
            user_agent,
            internal_ext,
            room_id,
            config,
        )
        .await
    }

    /// Like [`LiveConnection::open`], but from an already decoded [`FetchResult`]. The F1
    /// `replay` example uses this path because it starts from a fixture.
    pub async fn open_with(
        result: &FetchResult,
        cookies: &CookieJar,
        user_agent: &str,
        preset: &Preset,
        room_id: &str,
        config: &ConnectConfig,
    ) -> Result<Self, WsError> {
        if result.cursor.is_empty() {
            return Err(WsError::InitialCursorMissing);
        }
        if result.push_server.is_empty() || result.route_params.is_empty() {
            return Err(WsError::WebsocketUrlMissing);
        }
        if cookies.is_empty() {
            return Err(WsError::EmptyCookies);
        }

        let mut params = WsParams::new(room_id);
        params.compress = config.compress.clone();
        params.cursor = result.cursor.clone();
        params.internal_ext = result.internal_ext.clone();
        let uri = params.build_uri(&result.push_server, &result.route_params, preset);

        let room_id = room_id_from_str(room_id)?;
        Self::connect(
            uri,
            cookies,
            user_agent,
            &result.internal_ext,
            room_id,
            config,
        )
        .await
    }

    /// Connection implementation shared by both entry points.
    async fn connect(
        uri: String,
        cookies: &CookieJar,
        user_agent: &str,
        internal_ext: &str,
        room_id: u64,
        config: &ConnectConfig,
    ) -> Result<Self, WsError> {
        let mut request = uri
            .as_str()
            .into_client_request()
            .map_err(|e| WsError::Transport(e.to_string()))?;
        {
            let headers = request.headers_mut();
            headers.insert(
                "Cookie",
                HeaderValue::from_str(&cookies.to_cookie_string())
                    .map_err(|e| WsError::Transport(format!("invalid cookie: {e}")))?,
            );
            headers.insert(
                "User-Agent",
                HeaderValue::from_str(user_agent)
                    .map_err(|e| WsError::Transport(format!("invalid User-Agent: {e}")))?,
            );
            // The browser sends this automatically for the page's WebSocket. TikTok can
            // accept a handshake without it, but the resulting connection may stay silent.
            headers.insert("Origin", HeaderValue::from_static("https://www.tiktok.com"));
        }

        // No protocol keepalive: TikTok does not answer `ping`, so a ping interval with
        // its timeout would close the connection. `tungstenite` does not send pings on its
        // own; the application heartbeat is the real keepalive.
        let ws_config = WebSocketConfig::default();

        let (mut stream, response) =
            tokio_tungstenite::connect_async_with_config(request, Some(ws_config), false)
                .await
                .map_err(map_handshake_error)?;

        // A 101 only opens the transport. TikTok starts publishing room events after the
        // client sends the application-level room-entry request. The browser/reference
        // clients send this before the first heartbeat; without it a valid handshake can
        // remain silent forever.
        let enter_room = PushFrame::enter_room(room_id).encode();
        stream
            .send(WsMessage::Binary(enter_room))
            .await
            .map_err(|e| WsError::Closed(format!("room entry failed: {e}")))?;
        debug!(room_id, "room entry sent");

        let handshake_options = response
            .headers()
            .get("Handshake-Options")
            .and_then(|v| v.to_str().ok())
            .map(parse_handshake_options)
            .unwrap_or_default();
        debug!(?handshake_options, "handshake aceptado");

        let mut heartbeat = tokio::time::interval(config.heartbeat);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        Ok(Self {
            stream,
            internal_ext: internal_ext.to_string(),
            room_id,
            heartbeat_sequence: INITIAL_HEARTBEAT_SEQUENCE,
            heartbeat,
            handshake_options,
            uri,
        })
    }

    /// Options announced by the server during the handshake.
    pub fn handshake_options(&self) -> &[(String, String)] {
        &self.handshake_options
    }

    /// URI used to open the connection. It contains session parameters; never log it in full.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Read the next `msg` frame.
    ///
    /// Transport frames (`hb`, `ack`, `im_enter_room_resp`, …) are consumed here and are not
    /// returned. Acks and heartbeats are sent automatically.
    ///
    /// Returns `None` when the connection closes cleanly.
    pub async fn next_message(&mut self) -> Option<Result<LiveMessage, WsError>> {
        loop {
            tokio::select! {
                _ = self.heartbeat.tick() => {
                    if let Err(e) = self.send_heartbeat().await {
                        // If the heartbeat cannot be sent, the connection is dead.
                        return Some(Err(e));
                    }
                }
                incoming = self.stream.next() => {
                    match incoming {
                        None => return None,
                        Some(Err(e)) => return Some(Err(WsError::Closed(e.to_string()))),
                        Some(Ok(WsMessage::Close(frame))) => {
                            let reason = frame
                                .map(|f| format!("{} {}", f.code, f.reason))
                                .unwrap_or_else(|| "no reason provided".into());
                            return Some(Err(WsError::Closed(reason)));
                        }
                        Some(Ok(WsMessage::Binary(bytes))) => {
                            match self.handle_frame(&bytes).await {
                                Ok(Some(msg)) => return Some(Ok(msg)),
                                Ok(None) => continue,
                                Err(e) => return Some(Err(e)),
                            }
                        }
                        Some(Ok(other)) => {
                            debug!(kind = ?std::mem::discriminant(&other), "non-binary frame discarded");
                            continue;
                        }
                    }
                }
            }
        }
    }

    /// Process a frame. Returns `Some` only for event frames.
    async fn handle_frame(&mut self, bytes: &[u8]) -> Result<Option<LiveMessage>, WsError> {
        let frame = PushFrame::decode(bytes).map_err(|e| WsError::Decode(e.to_string()))?;

        if !frame.is_message() {
            debug!(payload_type = %frame.payload_type, "transport frame discarded");
            return Ok(None);
        }

        let payload = match frame.compress_type() {
            None | Some("") | Some("none") => frame.payload.clone(),
            Some("gzip") => gunzip(&frame.payload)?,
            Some(other) => {
                // A new value here means TikTok changed the protocol. Report it and try
                // the raw payload, which is the only possible fallback.
                error!(compress_type = %other, "unknown compression; trying raw payload");
                frame.payload.clone()
            }
        };

        // Send the ack after processing, using the received frame's log_id.
        let ack = frame.ack(&self.internal_ext);
        if let Err(e) = self.stream.send(WsMessage::Binary(ack.encode())).await {
            warn!(error = %e, "could not send ack");
        }

        Ok(Some(LiveMessage {
            log_id: frame.log_id,
            payload,
        }))
    }

    async fn send_heartbeat(&mut self) -> Result<(), WsError> {
        let frame = PushFrame::heartbeat(self.room_id, self.heartbeat_sequence);
        self.heartbeat_sequence = self.heartbeat_sequence.saturating_add(1);
        self.stream
            .send(WsMessage::Binary(frame.encode()))
            .await
            .map_err(|e| WsError::Closed(format!("heartbeat failed: {e}")))
    }

    /// Close the connection cleanly.
    pub async fn close(mut self) {
        let _ = self.stream.close(None).await;
    }
}

/// Distinguish detection rejection (HTTP 200) from a transport failure.
fn map_handshake_error(err: tokio_tungstenite::tungstenite::Error) -> WsError {
    use tokio_tungstenite::tungstenite::Error;
    match err {
        Error::Http(response) if response.status().as_u16() == u16::from(HTTP_OK) => {
            let msg = response
                .headers()
                .get("Handshake-Msg")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            WsError::Blocked200(msg)
        }
        other => WsError::Transport(other.to_string()),
    }
}

fn room_id_from_uri(uri: &str) -> Result<u64, WsError> {
    let query = uri
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    query
        .split('&')
        .find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key == "room_id").then_some(value)
        })
        .and_then(|value| value.parse().ok())
        .ok_or(WsError::RoomIdMissing)
}

fn room_id_from_str(room_id: &str) -> Result<u64, WsError> {
    room_id.parse().map_err(|_| WsError::RoomIdMissing)
}

/// `Handshake-Options` viene en formato cookie: `k=v; k=v`.
fn parse_handshake_options(raw: &str) -> Vec<(String, String)> {
    CookieJar::parse(raw)
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, WsError> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| WsError::Decode(format!("gzip: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn result_with(push_server: &str, cursor: &str) -> FetchResult {
        FetchResult {
            push_server: push_server.into(),
            route_params: vec![("wss_push_room_id".into(), "1".into())],
            cursor: cursor.into(),
            internal_ext: "ext".into(),
            heartbeat_duration: HEARTBEAT_DURATION_MILLISECONDS,
            need_ack: true,
        }
    }

    async fn open(result: FetchResult, cookies: &str) -> WsError {
        LiveConnection::open_with(
            &result,
            &CookieJar::parse(cookies),
            "UA",
            &Preset::default(),
            "1",
            &ConnectConfig::default(),
        )
        .await
        .err()
        .expect("must fail before connecting")
    }

    #[tokio::test]
    async fn validates_before_touching_the_network() {
        assert!(matches!(
            open(result_with("wss://x/", ""), "msToken=a").await,
            WsError::InitialCursorMissing
        ));
        assert!(matches!(
            open(result_with("", "c"), "msToken=a").await,
            WsError::WebsocketUrlMissing
        ));
        assert!(matches!(
            open(result_with("wss://x/", "c"), "").await,
            WsError::EmptyCookies
        ));
    }

    #[test]
    fn handshake_options_parse_like_cookies() {
        let opts = parse_handshake_options("compress=gzip; ping_interval=10");
        assert_eq!(
            opts,
            vec![
                ("compress".to_string(), "gzip".to_string()),
                ("ping_interval".to_string(), "10".to_string()),
            ]
        );
    }

    #[test]
    fn gzip_payloads_roundtrip() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"hola").unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(gunzip(&compressed).unwrap(), b"hola");
        assert!(gunzip(b"not gzip").is_err());
    }

    #[test]
    fn heartbeat_frame_contains_room_and_sequence() {
        let frame = PushFrame::heartbeat(7300, 1);
        assert_eq!(frame.payload_encoding, "pb");
        assert_eq!(frame.payload_type, "hb");
        assert_eq!(frame.payload, vec![0x08, 0x84, 0x39, 0x10, 0x01]);
    }

    #[test]
    fn room_id_is_read_from_the_signed_uri() {
        assert_eq!(
            room_id_from_uri("wss://example.test/ws?foo=bar&room_id=7300&x=y").unwrap(),
            7300
        );
        assert!(matches!(
            room_id_from_uri("wss://example.test/ws?foo=bar"),
            Err(WsError::RoomIdMissing)
        ));
    }
}
