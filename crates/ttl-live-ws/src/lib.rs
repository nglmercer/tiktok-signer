//! TikTok LIVE WebSocket client.
//!
//! Opens the connection and keeps it alive. **It does not sign anything.**
//!
//! Two ways in, and the second is the live one:
//!
//! - [`LiveConnection::open`] consumes a `SignedFetch` — the `push_server` and `route_params` an
//!   `/webcast/im/fetch/` response used to carry. Kept for fixtures and the replay example.
//! - [`LiveConnection::open_uri`] takes a URI the caller built and signed. This is what the web
//!   player does: with `wsDirect` on it builds the socket URI itself and signs the query with
//!   `registerWsSigner`, so no `im/fetch` response is involved. See
//!   [`ttl_sign_core::DirectSocketParams`].
//!
//! Two things this crate deliberately **does not** do:
//!
//! - **[`LiveConnection`] does not reconnect.** The signature ages out, so a reconnect means
//!   re-signing the URI. [`ReconnectingConnection`] is that policy, made once, over any
//!   `SignerBackend`; this type stays a single socket that reports its close reason and stops.
//! - **It does not parse events.** It returns decompressed `msg` payloads; event schemas
//!   belong to the consumer.
//!
//! A jar-less handshake is refused before the socket opens (measured: immediate 1006), which is
//! why [`WsError::EmptyCookies`] is checked before anything is sent.

use std::time::Duration;

pub mod reconnect;
pub use reconnect::{ReconnectPolicy, ReconnectingConnection, StreamError};

use flate2::read::GzDecoder;
use futures_util::{SinkExt, StreamExt};
use std::io::Read;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error};

use ttl_sign_core::proto::PushFrame;
use ttl_sign_core::{Compression, CookieJar, FetchResult, Preset, SignedFetch, WsParams};

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

    #[error("WebSocket handshake refused with HTTP {0}")]
    HttpStatus(u16),

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
    /// Compression requested in the socket query, and applied by the server to `msg` payloads.
    pub compress: Compression,
}

impl Default for ConnectConfig {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(DEFAULT_HEARTBEAT_SECONDS),
            compress: Compression::default(),
        }
    }
}

/// An open connection, consumed with [`LiveConnection::next_message`].
pub struct LiveConnection {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    internal_ext: String,
    room_id: u64,
    heartbeat_sequence: u64,
    last_ack_duration: Duration,
    last_ack_was_sent: bool,
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
        // A URI built by the browser can contain raw spaces (`browser_version=5.0 (X11; …)`)
        // that `http::Uri` refuses to parse, failing before anything is sent. Encoding them
        // does not disturb the signature.
        let uri = ttl_sign_core::sanitize_uri(uri);
        Self::connect(uri, cookies, user_agent, internal_ext, room_id, config).await
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
        params.compress = config.compress;
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
        if config.heartbeat.is_zero()
            || std::time::Instant::now()
                .checked_add(config.heartbeat)
                .is_none()
        {
            return Err(WsError::Transport(
                "heartbeat interval must be positive and representable".into(),
            ));
        }
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
        let ws_config = WebSocketConfig::default()
            .max_message_size(Some(16 * 1024 * 1024))
            .max_frame_size(Some(16 * 1024 * 1024));

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
            .send(WsMessage::Binary(enter_room.into()))
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
            last_ack_duration: Duration::ZERO,
            last_ack_was_sent: false,
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
    /// Duration of the most recent ACK write attempt.
    pub fn last_ack_duration(&self) -> Duration {
        self.last_ack_duration
    }

    /// Whether the most recent delivered batch required an ACK.
    pub fn last_ack_was_sent(&self) -> bool {
        self.last_ack_was_sent
    }

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
        let mut frame = PushFrame::decode(bytes).map_err(|e| WsError::Decode(e.to_string()))?;

        if !frame.is_message() {
            debug!(payload_type = %frame.payload_type, "transport frame discarded");
            return Ok(None);
        }

        let payload = match frame.compress_type() {
            None | Some("") | Some("none") => std::mem::take(&mut frame.payload),
            Some("gzip") => gunzip(&frame.payload)?,
            Some(other) => {
                // A new value here means TikTok changed the protocol. Report it and try
                // the raw payload, which is the only possible fallback.
                error!(compress_type = %other, "unknown compression; trying raw payload");
                std::mem::take(&mut frame.payload)
            }
        };

        // Verified against the public player's transport branch: ACK only requested batches.
        // Scan borrowed transport metadata once, before application event processing.
        let (extension, need_ack) = batch_ack_metadata(&payload)?;
        self.internal_ext.clear();
        self.internal_ext.push_str(extension);
        self.last_ack_was_sent = false;
        self.last_ack_duration = Duration::ZERO;
        if need_ack {
            let ack = frame.ack_for_batch(&self.internal_ext);
            let ack_started = std::time::Instant::now();
            self.stream
                .send(WsMessage::Binary(ack.encode().into()))
                .await
                .map_err(|e| WsError::Closed(format!("ack failed: {e}")))?;
            self.last_ack_was_sent = true;
            self.last_ack_duration = ack_started.elapsed();
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
            .send(WsMessage::Binary(frame.encode().into()))
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
        Error::Http(response) if response.status().as_u16() == HTTP_OK => {
            let msg = response
                .headers()
                .get("Handshake-Msg")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            WsError::Blocked200(msg)
        }
        Error::Http(response) => WsError::HttpStatus(response.status().as_u16()),
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

/// `Handshake-Options` uses cookie format: `k=v; k=v`.
fn parse_handshake_options(raw: &str) -> Vec<(String, String)> {
    CookieJar::parse(raw)
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Last field wins, matching protobuf scalar semantics; never reuse a previous batch's extension.
fn batch_ack_metadata(payload: &[u8]) -> Result<(&str, bool), WsError> {
    use ttl_sign_core::proto::{Reader, WireValue};
    let mut extension = "";
    let mut need_ack = false;
    let mut reader = Reader::new(payload);
    while let Some(field) = reader.next_field() {
        let (number, value) = field.map_err(|e| WsError::Decode(e.to_string()))?;
        if number == 9 {
            need_ack = match value {
                WireValue::Varint(value) => value != 0,
                _ => return Err(WsError::Decode("invalid need_ack wire type".into())),
            };
        }
        if number == 5 {
            match value {
                WireValue::Bytes(bytes) => {
                    extension =
                        std::str::from_utf8(bytes).map_err(|e| WsError::Decode(e.to_string()))?;
                }
                _ => return Err(WsError::Decode("invalid internal_ext wire type".into())),
            }
        }
    }
    Ok((extension, need_ack))
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, WsError> {
    const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
    let mut out = Vec::new();
    GzDecoder::new(data)
        .take((MAX_BATCH_BYTES + 1) as u64)
        .read_to_end(&mut out)
        .map_err(|e| WsError::Decode(format!("gzip: {e}")))?;
    if out.len() > MAX_BATCH_BYTES {
        return Err(WsError::Decode("decompressed batch exceeds 16 MiB".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn ack_write_failure_is_returned_and_not_reported_as_success() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(message)) = ws.next().await {
                if message.is_close() {
                    break;
                }
            }
        });
        let mut connection = LiveConnection::open_uri(
            &format!("ws://{address}/?room_id=1"),
            &CookieJar::parse("ttwid=offline"),
            "offline",
            "",
            &ConnectConfig::default(),
        )
        .await
        .unwrap();
        connection.stream.close(None).await.unwrap();
        let mut payload = include_bytes!("../../../fixtures/events/batch.pb").to_vec();
        payload.extend_from_slice(&[0x48, 1]);
        let frame = PushFrame {
            log_id: 42,
            payload_type: "msg".into(),
            payload,
            ..Default::default()
        };
        let result = connection.handle_frame(&frame.encode()).await;
        assert!(matches!(result, Err(WsError::Closed(message)) if message.contains("ack failed")));
        assert!(!connection.last_ack_was_sent());
        tokio::time::timeout(Duration::from_secs(1), peer)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn invalid_heartbeat_is_rejected_before_io() {
        for heartbeat in [Duration::ZERO, Duration::MAX] {
            let config = ConnectConfig {
                heartbeat,
                ..ConnectConfig::default()
            };
            let result = LiveConnection::open_uri(
                "ws://127.0.0.1:9/?room_id=1",
                &CookieJar::parse("ttwid=test"),
                "test",
                "",
                &config,
            )
            .await;
            assert!(
                matches!(result, Err(WsError::Transport(message)) if message.contains("heartbeat interval"))
            );
        }
    }

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

    #[tokio::test]
    async fn heartbeat_and_acks_progress_during_an_ordered_event_burst() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut writer, mut reader) = ws.split();
            let sender = tokio::spawn(async move {
                for id in 1..=200 {
                    let mut batch = ttl_sign_core::proto::Writer::new();
                    batch.bool_field(9, true);
                    let frame = PushFrame {
                        log_id: id,
                        payload_type: "msg".into(),
                        payload: batch.finish(),
                        ..Default::default()
                    };
                    writer
                        .send(WsMessage::Binary(frame.encode().into()))
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                writer
            });
            let mut acknowledgements = 0;
            let mut heartbeats = 0;
            while acknowledgements < 200 {
                let incoming = tokio::time::timeout(Duration::from_secs(2), reader.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                if let WsMessage::Binary(bytes) = incoming {
                    let frame = PushFrame::decode(&bytes).unwrap();
                    match frame.payload_type.as_str() {
                        "ack" => {
                            acknowledgements += 1;
                            assert_eq!(frame.log_id, acknowledgements);
                        }
                        "hb" => {
                            heartbeats += 1;
                        }
                        _ => (),
                    }
                }
            }
            let mut writer = sender.await.unwrap();
            writer.send(WsMessage::Close(None)).await.unwrap();
            assert!(
                heartbeats >= 2,
                "heartbeats must remain scheduled across the burst"
            );
        });
        let config = ConnectConfig {
            heartbeat: Duration::from_millis(5),
            ..ConnectConfig::default()
        };
        let mut connection = LiveConnection::open_uri(
            &format!("ws://{address}/?room_id=1"),
            &CookieJar::parse("ttwid=offline"),
            "test",
            "",
            &config,
        )
        .await
        .unwrap();
        for id in 1..=200 {
            let message = tokio::time::timeout(Duration::from_secs(2), connection.next_message())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(message.log_id, id);
        }
        connection.close().await;
        tokio::time::timeout(Duration::from_secs(2), peer)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn player_reference_fixture_matches_ack_wire_bytes() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/research/player-ack-protocol-v1.json"
        ))
        .unwrap();
        for case in fixture["synthetic_cases"].as_array().unwrap() {
            let extension = case["extension"].as_str().unwrap();
            let mut batch = ttl_sign_core::proto::Writer::new();
            batch
                .bool_field(9, case["need_ack"].as_bool().unwrap())
                .str_field(5, extension);
            let (actual_extension, requested) = batch_ack_metadata(&batch.finish())
                .map(|(e, n)| (e.to_owned(), n))
                .unwrap();
            assert_eq!(requested, case["send_ack"].as_bool().unwrap());
            if requested {
                let incoming = PushFrame {
                    log_id: case["log_id"].as_u64().unwrap(),
                    ..Default::default()
                };
                let hex = incoming
                    .ack_for_batch(&actual_extension)
                    .encode()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                assert_eq!(hex, case["ack_hex"].as_str().unwrap());
            }
        }
    }

    #[test]
    fn player_ack_metadata_is_per_batch_last_wins_and_defaults_false() {
        use ttl_sign_core::proto::Writer;
        assert_eq!(batch_ack_metadata(&[]).unwrap(), ("", false));
        let mut batch = Writer::new();
        batch
            .bool_field(9, true)
            .str_field(5, "current")
            .bool_field(9, false);
        let mut payload = batch.finish();
        payload.extend_from_slice(&[0x48, 0]); // Explicit false; Writer omits default scalar values.
        assert_eq!(batch_ack_metadata(&payload).unwrap(), ("current", false));
        let mut batch = Writer::new();
        batch.bool_field(9, true);
        assert_eq!(batch_ack_metadata(&batch.finish()).unwrap(), ("", true));
        assert!(batch_ack_metadata(&[74, 0]).is_err());
        let frame = PushFrame {
            log_id: 44,
            ..Default::default()
        };
        let ack = frame.ack_for_batch("");
        assert_eq!(ack.log_id, 44);
        assert!(ack.payload.is_empty());
        assert_eq!(frame.ack("").payload, b"-");
    }

    #[tokio::test]
    async fn ack_false_is_skipped_and_empty_extension_does_not_reuse_state() {
        use ttl_sign_core::proto::Writer;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            for (id, requested, extension) in [
                (42, false, "ignored"),
                (43, true, "current"),
                (44, true, ""),
            ] {
                let mut batch = Writer::new();
                batch.bool_field(9, requested).str_field(5, extension);
                let frame = PushFrame {
                    log_id: id,
                    payload_type: "msg".into(),
                    payload: batch.finish(),
                    ..Default::default()
                };
                ws.send(WsMessage::Binary(frame.encode().into()))
                    .await
                    .unwrap();
                if requested {
                    loop {
                        let message = ws.next().await.unwrap().unwrap();
                        if let WsMessage::Binary(bytes) = message {
                            let ack = PushFrame::decode(&bytes).unwrap();
                            if ack.payload_type == "ack" {
                                assert_eq!(ack.log_id, id, "false batch must not produce ACK");
                                assert_eq!(ack.payload, extension.as_bytes());
                                break;
                            }
                        }
                    }
                }
            }
            while let Some(Ok(message)) = ws.next().await {
                if message.is_close() {
                    break;
                }
            }
        });
        let mut connection = LiveConnection::open_uri(
            &format!("ws://{address}/?room_id=1"),
            &CookieJar::parse("ttwid=offline"),
            "test",
            "stale",
            &ConnectConfig::default(),
        )
        .await
        .unwrap();
        for (id, ack) in [(42, false), (43, true), (44, true)] {
            let message = tokio::time::timeout(Duration::from_secs(2), connection.next_message())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(message.log_id, id);
            assert_eq!(connection.last_ack_was_sent(), ack);
        }
        connection.close().await;
        tokio::time::timeout(Duration::from_secs(2), peer)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn ack_extension_matches_sanitized_capture_and_changes_per_batch() {
        fn batch_internal_ext(bytes: &[u8]) -> Result<&str, WsError> {
            batch_ack_metadata(bytes).map(|m| m.0)
        }
        let capture = include_bytes!("../../../fixtures/events/batch.pb");
        assert_eq!(
            batch_internal_ext(capture).unwrap(),
            "redacted-internal-ext"
        );
        let metadata = FetchResult::decode(capture).unwrap();
        assert_eq!(batch_internal_ext(capture).unwrap(), metadata.internal_ext);
        let mut batch = ttl_sign_core::proto::Writer::new();
        batch.str_field(5, "old").str_field(5, "new");
        assert_eq!(batch_internal_ext(&batch.finish()).unwrap(), "new");
        assert_eq!(batch_internal_ext(&[]).unwrap(), "");
        assert!(batch_internal_ext(&[0xff]).is_err());
        assert!(batch_internal_ext(&[0x28, 1]).is_err());
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
