//! Cliente WebSocket de TikTok LIVE.
//!
//! Consume un `SignedFetch` y abre la conexión. **No firma nada**: los `route_params`
//! vienen ya firmados por TikTok dentro del protobuf (`docs/05-spec-websocket-client.md`).
//!
//! Dos cosas que este crate deliberadamente **no** hace:
//!
//! - **No reconecta.** Los parámetros caducan en ~30 s, así que no existe "reconectar":
//!   existe rehacer el flujo desde `/webcast/im/fetch/`. El cierre se expone con su causa
//!   y decide el orquestador.
//! - **No parsea eventos.** Devuelve el payload de los frames `msg` ya descomprimido; el
//!   esquema de eventos es del consumidor.

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

/// Fallos de la conexión. `Blocked200` está separado del resto a propósito: no es
/// transitorio y reintentarlo es contraproducente.
#[derive(Debug, thiserror::Error)]
pub enum WsError {
    /// El protobuf no traía `cursor`: la respuesta no es válida.
    #[error("la respuesta no trae cursor inicial")]
    InitialCursorMissing,

    /// `push_server` o `route_params` vacíos: TikTok rechazó sin decirlo.
    #[error("la respuesta no trae URL de WebSocket (rechazo silencioso)")]
    WebsocketUrlMissing,

    /// El jar no trae las cookies que el WS necesita.
    #[error("faltan cookies de sesión para abrir el WebSocket")]
    EmptyCookies,

    /// Handshake con status 200: **detección**. No reintentar.
    #[error("TikTok rechazó el handshake (HTTP 200){}", .0.as_ref().map(|m| format!(": {m}")).unwrap_or_default())]
    Blocked200(Option<String>),

    #[error("el protobuf no se pudo decodificar: {0}")]
    Decode(String),

    #[error("error de transporte: {0}")]
    Transport(String),

    /// La conexión se cerró. Puede ser caducidad (>30 s desde la firma).
    #[error("la conexión se cerró: {0}")]
    Closed(String),
}

/// Un frame `msg` ya listo para parsear.
#[derive(Debug, Clone)]
pub struct LiveMessage {
    pub log_id: u64,
    /// Payload descomprimido. Contiene un `WebcastResponse`; parsearlo es del consumidor.
    pub payload: Vec<u8>,
}

/// Opciones de la conexión.
#[derive(Debug, Clone)]
pub struct ConnectConfig {
    /// Coherente con `heartbeat_duration=10000` de la query.
    pub heartbeat: Duration,
    /// Pedir los frames comprimidos. Vacío = sin compresión.
    pub compress: String,
}

impl Default for ConnectConfig {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(10),
            compress: "gzip".into(),
        }
    }
}

/// Conexión abierta. Se consume con [`LiveConnection::next_message`].
pub struct LiveConnection {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    internal_ext: String,
    heartbeat: tokio::time::Interval,
    /// Opciones que devolvió el servidor en `Handshake-Options`, útiles en diagnóstico.
    handshake_options: Vec<(String, String)>,
    uri: String,
}

impl LiveConnection {
    /// Abre la conexión a partir de una firma válida.
    ///
    /// Valida **antes** de tocar la red: un `push_server` vacío en un 200 significa que
    /// TikTok rechazó la petición, y conectar no va a arreglarlo.
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

    /// Abre una URI ya construida **y firmada** por el llamante.
    ///
    /// Es el camino real: la URI del WebSocket lleva firma propia
    /// (`byted_acrawler.frontierSign`), y quien sabe firmar es el motor de webview, no
    /// este crate.
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
        Self::connect(uri.to_string(), cookies, user_agent, internal_ext, config).await
    }

    /// Igual que [`LiveConnection::open`] pero desde un [`FetchResult`] ya decodificado.
    /// Es lo que usa el ejemplo `replay` de F1, que parte de un fixture y no de un firmante.
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

        Self::connect(uri, cookies, user_agent, &result.internal_ext, config).await
    }

    /// El grueso de la conexión, común a las dos entradas.
    async fn connect(
        uri: String,
        cookies: &CookieJar,
        user_agent: &str,
        internal_ext: &str,
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
                    .map_err(|e| WsError::Transport(format!("cookie inválida: {e}")))?,
            );
            headers.insert(
                "User-Agent",
                HeaderValue::from_str(user_agent)
                    .map_err(|e| WsError::Transport(format!("user-agent inválido: {e}")))?,
            );
        }

        // Sin keepalive de protocolo: TikTok no responde a los `ping`, así que un
        // ping_interval con su timeout cerraría la conexión sola
        // (`docs/05-spec-websocket-client.md` §Opciones de conexión). `tungstenite` no
        // envía pings por su cuenta; el keepalive real es el heartbeat de aplicación.
        let ws_config = WebSocketConfig::default();

        let (stream, response) =
            tokio_tungstenite::connect_async_with_config(request, Some(ws_config), false)
                .await
                .map_err(map_handshake_error)?;

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
            heartbeat,
            handshake_options,
            uri,
        })
    }

    /// Opciones que anunció el servidor en el handshake.
    pub fn handshake_options(&self) -> &[(String, String)] {
        &self.handshake_options
    }

    /// URI con la que se abrió la conexión. Contiene parámetros de sesión: no loguear en claro.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Siguiente frame `msg`.
    ///
    /// Los frames de transporte (`hb`, `ack`, `im_enter_room_resp`, …) se consumen aquí y
    /// no se devuelven. El `ack` y el heartbeat se envían solos.
    ///
    /// Devuelve `None` cuando la conexión se cierra limpiamente.
    pub async fn next_message(&mut self) -> Option<Result<LiveMessage, WsError>> {
        loop {
            tokio::select! {
                _ = self.heartbeat.tick() => {
                    if let Err(e) = self.send_heartbeat().await {
                        // Si el heartbeat no sale, la conexión está muerta.
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
                                .unwrap_or_else(|| "sin motivo".into());
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
                            debug!(kind = ?std::mem::discriminant(&other), "frame no binario, se descarta");
                            continue;
                        }
                    }
                }
            }
        }
    }

    /// Procesa un frame. Devuelve `Some` solo si lleva eventos.
    async fn handle_frame(&mut self, bytes: &[u8]) -> Result<Option<LiveMessage>, WsError> {
        let frame = PushFrame::decode(bytes).map_err(|e| WsError::Decode(e.to_string()))?;

        if !frame.is_message() {
            debug!(payload_type = %frame.payload_type, "frame de transporte, se descarta");
            return Ok(None);
        }

        let payload = match frame.compress_type() {
            None | Some("") | Some("none") => frame.payload.clone(),
            Some("gzip") => gunzip(&frame.payload)?,
            Some(other) => {
                // Un valor nuevo aquí es un cambio de TikTok: se avisa alto y se intenta
                // parsear en crudo, que es lo único que puede funcionar.
                error!(compress_type = %other, "compresión desconocida; se intenta en crudo");
                frame.payload.clone()
            }
        };

        // El ack va después de procesar, con el log_id del frame recibido.
        let ack = frame.ack(&self.internal_ext);
        if let Err(e) = self.stream.send(WsMessage::Binary(ack.encode())).await {
            warn!(error = %e, "no se pudo enviar el ack");
        }

        Ok(Some(LiveMessage {
            log_id: frame.log_id,
            payload,
        }))
    }

    async fn send_heartbeat(&mut self) -> Result<(), WsError> {
        let frame = PushFrame {
            payload_type: "hb".into(),
            ..Default::default()
        };
        self.stream
            .send(WsMessage::Binary(frame.encode()))
            .await
            .map_err(|e| WsError::Closed(format!("heartbeat fallido: {e}")))
    }

    /// Cierre ordenado.
    pub async fn close(mut self) {
        let _ = self.stream.close(None).await;
    }
}

/// Distingue el rechazo por detección (status 200) de un fallo de transporte.
fn map_handshake_error(err: tokio_tungstenite::tungstenite::Error) -> WsError {
    use tokio_tungstenite::tungstenite::Error;
    match err {
        Error::Http(response) if response.status().as_u16() == 200 => {
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
            heartbeat_duration: 10000,
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
        .expect("debería fallar antes de conectar")
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
        assert!(gunzip(b"no es gzip").is_err());
    }

    #[test]
    fn heartbeat_frame_is_the_documented_four_bytes() {
        let frame = PushFrame {
            payload_type: "hb".into(),
            ..Default::default()
        };
        // field 7 (payload_type), wire type 2, longitud 2, "hb"
        assert_eq!(frame.encode(), vec![0x3a, 0x02, b'h', b'b']);
    }
}
