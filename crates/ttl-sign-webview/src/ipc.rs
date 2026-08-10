//! Mensajes del puente JS↔Rust (`docs/04-spec-webview-bridge.md` §Mensajes IPC).

use serde::{Deserialize, Serialize};

/// JS → Rust. `request_id` correlaciona con el `oneshot::Sender` que espera.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromPage {
    /// El SDK está cargado y el puente instalado. Sin esto no se acepta ninguna firma.
    Ready {
        #[serde(default)]
        sdk_version: Option<String>,
    },
    /// Petición completada (con cualquier status HTTP).
    Result {
        request_id: u64,
        status: u16,
        #[serde(default)]
        url: String,
        body_b64: String,
        #[serde(default)]
        cookie: String,
    },
    /// Fallo dentro de la página. `request_id == 0` significa que no corresponde a
    /// ninguna petición concreta (p. ej. `sdk_not_ready`).
    Error { request_id: u64, message: String },
}

/// Rust → JS. La query la construye Rust; el JS no compone parámetros.
#[derive(Debug, Clone, Serialize)]
pub struct ToPage {
    pub request_id: u64,
    pub url: String,
}

impl ToPage {
    /// La llamada que se pasa a `evaluate_script`.
    pub fn to_script(&self) -> String {
        let json = serde_json::to_string(self).expect("ToPage siempre serializa");
        format!("window.__ttlSign({json})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready() {
        let msg: FromPage =
            serde_json::from_str(r#"{"type":"ready","sdk_version":"1.0.0.368"}"#).unwrap();
        assert!(matches!(msg, FromPage::Ready { sdk_version: Some(v) } if v == "1.0.0.368"));
    }

    #[test]
    fn parses_ready_without_version() {
        let msg: FromPage = serde_json::from_str(r#"{"type":"ready","sdk_version":null}"#).unwrap();
        assert!(matches!(msg, FromPage::Ready { sdk_version: None }));
    }

    #[test]
    fn parses_result() {
        let raw = r#"{"type":"result","request_id":42,"status":200,
                      "url":"https://webcast.tiktok.com/webcast/im/fetch/?X-Gnarly=K",
                      "body_b64":"CgoK","cookie":"msToken=abc"}"#;
        match serde_json::from_str(raw).unwrap() {
            FromPage::Result {
                request_id,
                status,
                body_b64,
                cookie,
                url,
            } => {
                assert_eq!(request_id, 42);
                assert_eq!(status, 200);
                assert_eq!(body_b64, "CgoK");
                assert_eq!(cookie, "msToken=abc");
                assert!(url.contains("X-Gnarly"));
            }
            other => panic!("mensaje inesperado: {other:?}"),
        }
    }

    #[test]
    fn parses_error() {
        let msg: FromPage = serde_json::from_str(
            r#"{"type":"error","request_id":7,"message":"TypeError: Failed to fetch"}"#,
        )
        .unwrap();
        assert!(matches!(msg, FromPage::Error { request_id: 7, .. }));
    }

    #[test]
    fn script_is_a_single_call() {
        let script = ToPage {
            request_id: 1,
            url: "https://example.com/?a=1".into(),
        }
        .to_script();
        assert!(script.starts_with("window.__ttlSign({"), "{script}");
        assert!(script.ends_with("})"), "{script}");
    }
}
