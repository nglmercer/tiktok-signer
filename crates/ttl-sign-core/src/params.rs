//! Construcción de query strings.
//!
//! Dos consumidores:
//!
//! - [`FetchParams`] → la query de `https://webcast.tiktok.com/webcast/im/fetch/`
//!   (`docs/00-research.md` §4). Es lo único que se firma.
//! - [`WsParams`] → la query del WebSocket (`docs/05-spec-websocket-client.md`), que
//!   **no** se firma: los `route_params` vienen ya firmados dentro del protobuf.
//!
//! Se usa un `Vec<(String, String)>` y no un `HashMap` a propósito: el orden es estable
//! (tests reproducibles) y la query del WS necesita repetir `version_code` con dos
//! valores distintos, cosa que un mapa no puede representar.

use rand::Rng;

use crate::preset::Preset;

/// Query string en construcción: pares ordenados, con duplicados permitidos.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query(Vec<(String, String)>);

impl Query {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Añade un par. Si la clave ya existía, **sobrescribe** el valor en su posición
    /// original (así los `ws_client_params` pisan a los `route_params` sin reordenar).
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let key = key.into();
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
        self
    }

    /// Añade un par **sin** deduplicar. Necesario para el `version_code` duplicado.
    pub fn push_raw(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.0.push((key.into(), value.into()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Serializa a `k=v&k=v`, percent-encoding los valores.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.0 {
            if !out.is_empty() {
                out.push('&');
            }
            out.push_str(&percent_encode(k));
            out.push('=');
            out.push_str(&percent_encode(v));
        }
        out
    }

    /// Parsea `k=v&k=v`. No decodifica: se usa sobre queries ya normalizadas en tests.
    pub fn parse(raw: &str) -> Self {
        let mut q = Query::new();
        for pair in raw.trim_start_matches('?').split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            q.push_raw(k, v);
        }
        q
    }
}

/// Percent-encoding para query strings: se conserva el conjunto no reservado más
/// `.`, `-`, `_`, `~`, y se codifica todo lo demás. Es lo que hace `encodeURIComponent`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Genera un `device_id` de 19 dígitos, como el que emite el navegador real.
pub fn random_device_id() -> String {
    let mut rng = rand::thread_rng();
    let mut s = String::with_capacity(19);
    s.push(char::from_digit(rng.gen_range(1..=9), 10).expect("dígito válido"));
    for _ in 0..18 {
        s.push(char::from_digit(rng.gen_range(0..=9), 10).expect("dígito válido"));
    }
    s
}

/// `last_rtt` plausible, en el rango que usa el cliente de referencia.
pub fn random_last_rtt() -> u32 {
    rand::thread_rng().gen_range(100..=200)
}

/// Query de `/webcast/im/fetch/`.
///
/// El `X-Bogus` / `X-Gnarly` / `msToken` **no** se añaden aquí: los pone `webmssdk.js`
/// dentro del webview al interceptar el `fetch` (`docs/01-architecture.md` §D2).
#[derive(Debug, Clone)]
pub struct FetchParams {
    pub room_id: String,
    pub device_id: String,
    pub cursor: String,
    pub internal_ext: String,
    /// Email de contacto que pide la spec de Euler. Vacío = no se emite.
    pub contact_us: String,
    /// `sup_ws_ds_opt`: le dice a TikTok qué tipo de WebSocket queremos.
    ///
    /// Con `1` la respuesta trae un `push_server` de tipo
    /// `ws_proxy/ws_reuse_supplement/`; el patrón que documenta
    /// `docs/00-research.md` §1 es `webcast<N>-ws-web-<idc>`. Configurable para poder
    /// comparar los dos contra una sala real.
    pub sup_ws_ds_opt: u8,
}

impl FetchParams {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            device_id: random_device_id(),
            cursor: String::new(),
            internal_ext: String::new(),
            contact_us: String::new(),
            sup_ws_ds_opt: 1,
        }
    }

    /// Construye la query completa a partir del preset.
    pub fn build(&self, preset: &Preset) -> Query {
        let d = &preset.device;
        let l = &preset.location;
        let s = &preset.screen;

        let mut q = Query::new();
        q.set("aid", "1988")
            .set("app_language", &l.language)
            .set("app_name", "tiktok_web")
            .set("browser_language", &l.browser_language)
            .set("browser_name", &d.browser_name)
            .set("browser_online", "true")
            .set("browser_platform", &d.browser_platform)
            .set("browser_version", &d.browser_version)
            .set("cookie_enabled", "true")
            .set("cursor", &self.cursor)
            .set("debug", "false")
            .set("device_id", &self.device_id)
            .set("device_platform", "web")
            .set("did_rule", "3")
            .set("fetch_rule", "1")
            .set("history_comment_count", "6")
            .set("history_comment_cursor", "")
            .set("identity", "audience")
            .set("internal_ext", &self.internal_ext)
            .set("last_rtt", "0")
            .set("live_id", "12")
            .set("os", &d.os)
            .set("priority_region", &l.region)
            .set("region", &l.region)
            .set("resp_content_type", "protobuf")
            .set("room_id", &self.room_id)
            .set("screen_height", s.height.to_string())
            .set("screen_width", s.width.to_string())
            .set("sup_ws_ds_opt", self.sup_ws_ds_opt.to_string())
            .set("tz_name", &l.tz_name)
            .set("version_code", "270000")
            .set("webcast_language", &l.language)
            .set("notice", "CUSTOM_SIGN_SERVER");

        if !self.contact_us.is_empty() {
            q.set("contact_us", &self.contact_us);
        }
        q
    }

    /// URL absoluta lista para pasar al `fetch` parcheado del webview.
    pub fn url(&self, preset: &Preset) -> String {
        format!("{}?{}", FETCH_ENDPOINT, self.build(preset).encode())
    }
}

/// Endpoint que hay que firmar. El único del camino crítico.
pub const FETCH_ENDPOINT: &str = "https://webcast.tiktok.com/webcast/im/fetch/";

/// Query del WebSocket: `route_params` firmados + nuestros params + el `version_code`
/// duplicado del final (`docs/05-spec-websocket-client.md`).
#[derive(Debug, Clone)]
pub struct WsParams {
    pub room_id: String,
    /// `gzip`, o vacío para pedir sin compresión.
    pub compress: String,
    pub last_rtt: u32,
    /// Cursor de la respuesta de `/webcast/im/fetch/`.
    ///
    /// Va en la query del WebSocket, no en los `route_params`: comprobado contra una
    /// respuesta real, donde `route_params` solo trae `wrss` e `imprp`. Sin cursor el
    /// handshake se acepta igual y luego **no llega ni un frame**, que es el fallo más
    /// desconcertante de todo el flujo.
    pub cursor: String,
    /// `internal_ext` de la misma respuesta. Mismo razonamiento.
    pub internal_ext: String,
}

impl WsParams {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            compress: "gzip".into(),
            last_rtt: random_last_rtt(),
            cursor: String::new(),
            internal_ext: String::new(),
        }
    }

    /// Los params propios del cliente, sin los `route_params`.
    pub fn client_params(&self, preset: &Preset) -> Query {
        let d = &preset.device;
        let l = &preset.location;
        let s = &preset.screen;

        let mut q = Query::new();
        q.set("aid", "1988")
            .set("app_language", &l.language)
            .set("app_name", "tiktok_web")
            .set("browser_language", &l.browser_language)
            .set("browser_name", &d.browser_name)
            .set("browser_online", "true")
            .set("browser_platform", &d.browser_platform)
            .set("browser_version", &d.browser_version)
            .set("client_enter", "1")
            .set("compress", &self.compress)
            .set("cookie_enabled", "true")
            .set("cursor", &self.cursor)
            .set("device_platform", "web")
            .set("did_rule", "3")
            .set("heartbeat_duration", "10000")
            .set("internal_ext", &self.internal_ext)
            .set("history_comment_count", "6")
            .set("identity", "audience")
            .set("last_rtt", self.last_rtt.to_string())
            .set("live_id", "12")
            .set("resp_content_type", "protobuf")
            .set("room_id", &self.room_id)
            .set("screen_height", s.height.to_string())
            .set("screen_width", s.width.to_string())
            .set("sup_ws_ds_opt", "1")
            .set("tz_name", &l.tz_name)
            .set("update_version_code", "2.0.0")
            .set("version_code", "180800")
            .set("webcast_language", &l.language)
            .set("ws_direct", "1");
        q
    }

    /// URI completa del WebSocket.
    ///
    /// Tres reglas que hay que respetar tal cual (`docs/05` §Construcción de la URI):
    ///
    /// 1. Los `route_params` con valor vacío se descartan.
    /// 2. Los params del cliente se aplican después: ganan ante colisión.
    /// 3. `&version_code=270000` se concatena al final **aunque** ya haya un
    ///    `version_code=180800`. La query lo lleva dos veces, y es intencionado.
    pub fn build_uri(
        &self,
        push_server: &str,
        route_params: &[(String, String)],
        preset: &Preset,
    ) -> String {
        let mut q = Query::new();
        for (k, v) in route_params {
            if v.is_empty() {
                continue;
            }
            q.set(k, v);
        }
        for (k, v) in self.client_params(preset).iter() {
            q.set(k, v);
        }
        q.push_raw("version_code", "270000");

        let sep = if push_server.contains('?') { '&' } else { '?' };
        format!("{push_server}{sep}{}", q.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{DevicePreset, LocationPreset, Preset, ScreenPreset};

    fn preset() -> Preset {
        Preset::new(
            DevicePreset::chrome_windows(),
            LocationPreset::us_east(),
            ScreenPreset::FHD,
        )
    }

    #[test]
    fn fetch_query_carries_the_euler_documented_params() {
        let p = preset();
        let q = FetchParams::new("7300000000000000000").build(&p);

        for key in [
            "aid",
            "app_name",
            "browser_name",
            "browser_version",
            "device_id",
            "device_platform",
            "identity",
            "resp_content_type",
            "room_id",
            "sup_ws_ds_opt",
            "version_code",
        ] {
            assert!(q.get(key).is_some(), "falta el param {key}");
        }
        assert_eq!(q.get("aid"), Some("1988"));
        assert_eq!(q.get("resp_content_type"), Some("protobuf"));
        assert_eq!(q.get("room_id"), Some("7300000000000000000"));
    }

    /// Coherencia UA ↔ query: la causa nº1 de rechazo.
    #[test]
    fn fetch_query_browser_params_match_the_user_agent() {
        for p in Preset::all() {
            let q = FetchParams::new("1").build(&p);
            let ua = p.user_agent();
            assert_eq!(
                ua,
                format!(
                    "{}/{}",
                    q.get("browser_name").unwrap(),
                    q.get("browser_version").unwrap()
                )
            );
            assert_eq!(
                q.get("browser_platform"),
                Some(p.device.browser_platform.as_str())
            );
            assert_eq!(q.get("os"), Some(p.device.os.as_str()));
        }
    }

    #[test]
    fn device_id_is_19_digits() {
        for _ in 0..100 {
            let id = random_device_id();
            assert_eq!(id.len(), 19, "device_id de longitud incorrecta: {id}");
            assert!(id.chars().all(|c| c.is_ascii_digit()));
            assert_ne!(id.as_bytes()[0], b'0', "no debe empezar por cero: {id}");
        }
    }

    /// `cursor` e `internal_ext` viajan en la query del WS, no en los `route_params`.
    #[test]
    fn ws_uri_carries_cursor_and_internal_ext() {
        let mut params = WsParams::new("42");
        params.cursor = "1786_7672_1_1".into();
        params.internal_ext = "internal_src:dim|wss_info:0-1".into();
        let uri = params.build_uri("wss://x/ws/", &[("wrss".into(), "abc".into())], &preset());
        assert!(uri.contains("cursor=1786_7672_1_1"), "{uri}");
        assert!(uri.contains("internal_ext=internal_src"), "{uri}");
        assert!(uri.contains("wrss=abc"), "{uri}");
    }

    #[test]
    fn ws_uri_keeps_version_code_twice() {
        let uri = WsParams::new("42").build_uri(
            "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/",
            &[("wss_push_room_id".into(), "42".into())],
            &preset(),
        );
        // `&` delante: si no, `update_version_code=` cuenta también.
        let occurrences = uri.matches("&version_code=").count();
        assert_eq!(
            occurrences, 2,
            "version_code debe aparecer dos veces: {uri}"
        );
        assert!(uri.ends_with("&version_code=270000"), "{uri}");
        assert!(uri.contains("version_code=180800"), "{uri}");
    }

    #[test]
    fn ws_uri_drops_empty_route_params() {
        let uri = WsParams::new("42").build_uri(
            "wss://example.tiktok.com/webcast/im/ws/",
            &[
                ("keep".into(), "yes".into()),
                ("drop".into(), String::new()),
            ],
            &preset(),
        );
        assert!(uri.contains("keep=yes"), "{uri}");
        assert!(!uri.contains("drop="), "{uri}");
    }

    #[test]
    fn client_params_win_over_route_params() {
        let uri = WsParams::new("42").build_uri(
            "wss://example.tiktok.com/webcast/im/ws/",
            &[("identity".into(), "anchor".into())],
            &preset(),
        );
        assert!(uri.contains("identity=audience"), "{uri}");
        assert!(!uri.contains("identity=anchor"), "{uri}");
    }

    #[test]
    fn query_roundtrips_through_parse() {
        let q = Query::parse("a=1&b=2&a=3");
        assert_eq!(q.len(), 3, "parse no debe deduplicar");
        assert_eq!(q.encode(), "a=1&b=2&a=3");
    }

    #[test]
    fn encoding_escapes_reserved_characters() {
        let mut q = Query::new();
        q.set("tz_name", "America/New_York");
        assert_eq!(q.encode(), "tz_name=America%2FNew_York");
    }
}
