//! Mensajes del puente JS↔Rust (`docs/04-spec-webview-bridge.md` §Mensajes IPC).

use serde::{Deserialize, Serialize};

/// JS → Rust. `request_id` correlaciona con el `oneshot::Sender` que espera.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromPage {
    /// The SDK is loaded and the bridge is installed. No signature is accepted before this.
    Ready {
        #[serde(default)]
        sdk_version: Option<String>,
        /// Environment visible from inside the page. Query parameters are
        /// alinean con esto en vez de adivinarlo desde Rust.
        #[serde(default)]
        env: Option<PageEnv>,
    },
    /// The bridge installed session cookies in **this** document. The engine waits for this
    /// before navigating to the real page.
    Session {
        #[serde(default)]
        installed: usize,
        #[serde(default)]
        host: String,
        #[serde(default)]
        cookie: String,
    },
    /// The SDK sent a signed request. **It has no body**: `/webcast/im/fetch/` does not
    /// return CORS headers, so the page cannot read the response. Rust
    /// repite esta URL con su propio cliente HTTP (Plan B).
    Signed {
        request_id: u64,
        /// URL ya firmada: incluye X-Bogus, X-Gnarly, X-Dynosaur y msToken.
        url: String,
        #[serde(default)]
        cookie: String,
        /// Page that produced the signature: this is the `Referer` TikTok expects.
        #[serde(default)]
        page: String,
    },
    /// Respuesta de texto: el lookup `uniqueId` → `room_id`, o el DOM renderizado.
    /// No lleva cookies porque no interviene en ninguna firma.
    Text {
        request_id: u64,
        status: u16,
        body: String,
    },
    /// Page failure. `request_id == 0` means it is not tied to a specific request.
    Error { request_id: u64, message: String },
}

/// What the page reports about itself: language, time zone, screen, and region of the
/// cuenta. Sustituye a adivinarlo con un preset fijo.
#[derive(Debug, Clone, Deserialize)]
pub struct PageEnv {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub browser_language: String,
    #[serde(default)]
    pub tz_name: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub screen_width: u32,
    #[serde(default)]
    pub screen_height: u32,
}

/// Rust → JS. Rust builds the query; JavaScript does not compose parameters.
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

/// Rust → JS para el paso sin firma: un GET de texto, o el DOM si `url` es `None`.
#[derive(Debug, Clone, Serialize)]
pub struct ToPageText {
    pub request_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl ToPageText {
    pub fn to_script(&self) -> String {
        let json = serde_json::to_string(self).expect("ToPageText siempre serializa");
        format!("window.__ttlText({json})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready() {
        let msg: FromPage =
            serde_json::from_str(r#"{"type":"ready","sdk_version":"1.0.0.368"}"#).unwrap();
        assert!(matches!(msg, FromPage::Ready { sdk_version: Some(v), .. } if v == "1.0.0.368"));
    }

    #[test]
    fn parses_ready_without_version() {
        let msg: FromPage = serde_json::from_str(r#"{"type":"ready","sdk_version":null}"#).unwrap();
        assert!(matches!(
            msg,
            FromPage::Ready {
                sdk_version: None,
                ..
            }
        ));
    }

    #[test]
    fn parses_signed() {
        let raw = r#"{"type":"signed","request_id":42,
                      "url":"https://webcast.tiktok.com/webcast/im/fetch/?X-Gnarly=K",
                      "cookie":"msToken=abc"}"#;
        match serde_json::from_str(raw).unwrap() {
            FromPage::Signed {
                request_id,
                cookie,
                url,
                ..
            } => {
                assert_eq!(request_id, 42);
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
    fn parses_text() {
        let msg: FromPage =
            serde_json::from_str(r#"{"type":"text","request_id":3,"status":200,"body":"<html>"}"#)
                .unwrap();
        assert!(matches!(msg, FromPage::Text { request_id: 3, .. }));
    }

    /// Sin `url` el puente devuelve el DOM: la clave no debe aparecer siquiera.
    #[test]
    fn dom_request_omits_the_url() {
        let script = ToPageText {
            request_id: 5,
            url: None,
        }
        .to_script();
        assert_eq!(script, r#"window.__ttlText({"request_id":5})"#);
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
