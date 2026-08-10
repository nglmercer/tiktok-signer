//! Paso 1 del flujo: `unique_id` → `room_id`. **Sin firma**
//! (`docs/00-research.md` §1).
//!
//! Dos caminos, porque TikTok sirve las dos cosas de forma distinta:
//!
//! | Qué | Cómo | Firma |
//! |---|---|---|
//! | `unique_id` → `room_id` + estado | `GET /api-live/user/room/?uniqueId=…`, JSON | no |
//! | Quién está en directo ahora | DOM de `https://www.tiktok.com/live` **ya renderizado** | no |
//!
//! La página `/live` no trae los datos en el HTML: los pinta el cliente. Por eso
//! [`extract_live_channels`] se aplica al DOM que devuelve el webview, no a un `GET`
//! pelado, que solo devolvería el esqueleto.
//!
//! Aquí no hay I/O: todo son funciones puras sobre el texto que traiga quien sea.

use std::collections::BTreeMap;

/// Página de exploración de directos. Necesita JS para poblarse.
pub const LIVE_EXPLORE_URL: &str = "https://www.tiktok.com/live";

/// URL del directo de un usuario.
pub fn live_page_url(unique_id: &str) -> String {
    format!("https://www.tiktok.com/@{}/live", unique_id.trim_start_matches('@'))
}

/// Endpoint que resuelve `unique_id` → `room_id`. No requiere firma ni cookies.
pub fn room_lookup_url(unique_id: &str) -> String {
    format!(
        "https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId={}",
        unique_id.trim_start_matches('@')
    )
}

/// Estado de la sala de un usuario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomLookup {
    pub unique_id: String,
    pub room_id: String,
    pub nickname: String,
    /// Campo `status` tal cual. `4` = el directo ha terminado; `2` = en directo.
    pub status: i64,
    pub title: String,
}

impl RoomLookup {
    /// ¿Está emitiendo ahora?
    ///
    /// TikTok marca con `4` las salas terminadas. El `room_id` sigue ahí, así que
    /// comprobar solo que exista no vale: firmar contra una sala apagada da un protobuf
    /// sin `push_server`, que es indistinguible de un rechazo.
    pub fn is_live(&self) -> bool {
        self.status != 4 && !self.room_id.is_empty() && self.room_id != "0"
    }

    /// Parsea la respuesta de [`room_lookup_url`].
    pub fn from_json(raw: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        let user = value.get("data")?.get("user")?;
        let live_room = value.get("data").and_then(|d| d.get("liveRoom"));

        Some(Self {
            unique_id: string_at(user, "uniqueId"),
            room_id: string_at(user, "roomId"),
            nickname: string_at(user, "nickname"),
            // El estado del usuario y el de la sala coinciden; si falta uno, vale el otro.
            status: user
                .get("status")
                .and_then(serde_json::Value::as_i64)
                .or_else(|| live_room?.get("status")?.as_i64())
                .unwrap_or(0),
            title: live_room.map(|r| string_at(r, "title")).unwrap_or_default(),
        })
    }
}

fn string_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Un canal encontrado en la página de exploración.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveChannel {
    pub unique_id: String,
    /// Vacío si la página solo traía el enlace: hay que resolverlo con [`room_lookup_url`].
    pub room_id: String,
    pub nickname: String,
}

/// Extrae los canales de un DOM ya renderizado de `https://www.tiktok.com/live`.
///
/// Combina dos fuentes porque ninguna es fiable por sí sola:
///
/// 1. Los enlaces `/@usuario/live` del DOM — sobreviven a cualquier cambio de esquema
///    JSON, pero no traen `room_id`.
/// 2. Cualquier objeto JSON embebido que tenga `uniqueId` **y** `roomId`, buscado
///    recursivamente sin asumir dónde vive. TikTok mueve esas claves de sitio a menudo;
///    lo que no cambia es que se llamen así.
pub fn extract_live_channels(dom: &str) -> Vec<LiveChannel> {
    // BTreeMap: orden estable y deduplicado por unique_id.
    let mut found: BTreeMap<String, LiveChannel> = BTreeMap::new();

    for unique_id in extract_live_links(dom) {
        found.entry(unique_id.clone()).or_insert(LiveChannel {
            unique_id,
            room_id: String::new(),
            nickname: String::new(),
        });
    }

    for json in embedded_json_blobs(dom) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            let mut collected = Vec::new();
            collect_channels(&value, &mut collected);
            for channel in collected {
                found
                    .entry(channel.unique_id.clone())
                    .and_modify(|existing| {
                        if existing.room_id.is_empty() {
                            existing.room_id = channel.room_id.clone();
                        }
                        if existing.nickname.is_empty() {
                            existing.nickname = channel.nickname.clone();
                        }
                    })
                    .or_insert(channel);
            }
        }
    }

    found.into_values().collect()
}

/// `href="/@usuario/live"` → `usuario`.
fn extract_live_links(dom: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in dom.split("/@").skip(1) {
        let Some(end) = chunk.find(['"', '\'', '?', '<', ' ', '\\']) else {
            continue;
        };
        let path = &chunk[..end];
        let Some(unique_id) = path.strip_suffix("/live") else {
            continue;
        };
        if !unique_id.is_empty() && unique_id.chars().all(is_username_char) {
            out.push(unique_id.to_string());
        }
    }
    out
}

fn is_username_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'
}

/// Contenido de los `<script>` que declaran JSON, más el documento entero por si el
/// DOM que nos pasan ya *es* JSON.
fn embedded_json_blobs(dom: &str) -> Vec<String> {
    let trimmed = dom.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return vec![dom.to_string()];
    }

    let mut out = Vec::new();
    for chunk in dom.split("<script").skip(1) {
        let Some(open) = chunk.find('>') else { continue };
        let (attrs, rest) = chunk.split_at(open);
        if !attrs.contains("application/json") {
            continue;
        }
        if let Some(end) = rest.find("</script>") {
            out.push(rest[1..end].to_string());
        }
    }
    out
}

/// Recorre el árbol JSON buscando objetos que sean un canal.
fn collect_channels(value: &serde_json::Value, out: &mut Vec<LiveChannel>) {
    match value {
        serde_json::Value::Object(map) => {
            let unique_id = map.get("uniqueId").and_then(serde_json::Value::as_str);
            let room_id = map
                .get("roomId")
                .and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n| n.to_string())));

            if let (Some(unique_id), Some(room_id)) = (unique_id, room_id) {
                if !unique_id.is_empty() && !room_id.is_empty() && room_id != "0" {
                    out.push(LiveChannel {
                        unique_id: unique_id.to_string(),
                        room_id,
                        nickname: map
                            .get("nickname")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    });
                }
            }
            for child in map.values() {
                collect_channels(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_channels(child, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forma real de la respuesta de `/api-live/user/room/`, recortada.
    const ROOM_JSON: &str = r#"{
      "data": {
        "user": {
          "id": "107955",
          "nickname": "TikTok",
          "uniqueId": "tiktok",
          "roomId": "7671098478126271240",
          "status": 4
        },
        "liveRoom": { "title": "Alex Warren LIVE", "status": 4 }
      },
      "statusCode": 0
    }"#;

    #[test]
    fn parses_the_room_lookup() {
        let lookup = RoomLookup::from_json(ROOM_JSON).unwrap();
        assert_eq!(lookup.unique_id, "tiktok");
        assert_eq!(lookup.room_id, "7671098478126271240");
        assert_eq!(lookup.nickname, "TikTok");
        assert_eq!(lookup.title, "Alex Warren LIVE");
    }

    /// El `room_id` sigue presente cuando el directo ha terminado: por eso no basta con
    /// comprobar que exista.
    #[test]
    fn status_4_is_not_live_even_with_a_room_id() {
        let lookup = RoomLookup::from_json(ROOM_JSON).unwrap();
        assert_eq!(lookup.status, 4);
        assert!(!lookup.is_live());

        let live = RoomLookup {
            status: 2,
            ..lookup
        };
        assert!(live.is_live());
    }

    #[test]
    fn a_zero_room_id_is_never_live() {
        let lookup = RoomLookup {
            unique_id: "x".into(),
            room_id: "0".into(),
            nickname: String::new(),
            status: 2,
            title: String::new(),
        };
        assert!(!lookup.is_live());
    }

    #[test]
    fn malformed_json_returns_none_instead_of_panicking() {
        assert!(RoomLookup::from_json("no soy json").is_none());
        assert!(RoomLookup::from_json(r#"{"data":{}}"#).is_none());
    }

    #[test]
    fn lookup_url_tolerates_a_leading_at() {
        assert_eq!(room_lookup_url("@user"), room_lookup_url("user"));
        assert!(room_lookup_url("user").ends_with("uniqueId=user"));
        assert_eq!(live_page_url("@user"), "https://www.tiktok.com/@user/live");
    }

    #[test]
    fn finds_channels_from_dom_links() {
        let dom = r#"<div>
            <a href="/@alice/live"><span>Alice</span></a>
            <a href="/@bob.b_1/live?lang=en">Bob</a>
            <a href="/@carol">no es un directo</a>
            <a href="/@dave/video/123">tampoco</a>
        </div>"#;
        let ids: Vec<_> = extract_live_channels(dom)
            .into_iter()
            .map(|c| c.unique_id)
            .collect();
        assert_eq!(ids, vec!["alice", "bob.b_1"]);
    }

    #[test]
    fn merges_room_ids_from_embedded_json() {
        let dom = r#"<a href="/@alice/live">A</a>
          <script id="x" type="application/json">
            {"any":{"where":{"deep":[{"uniqueId":"alice","roomId":"7300","nickname":"Alice"}]}}}
          </script>"#;
        let channels = extract_live_channels(dom);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].room_id, "7300");
        assert_eq!(channels[0].nickname, "Alice");
    }

    #[test]
    fn accepts_raw_json_as_input() {
        let channels = extract_live_channels(r#"[{"uniqueId":"eve","roomId":7400}]"#);
        assert_eq!(channels[0].unique_id, "eve");
        assert_eq!(channels[0].room_id, "7400");
    }

    #[test]
    fn ignores_channels_without_a_room() {
        let channels = extract_live_channels(r#"{"uniqueId":"ghost","roomId":"0"}"#);
        assert!(channels.is_empty());
    }

    /// La página `/live` sin renderizar no trae canales. Devolver una lista vacía es la
    /// respuesta correcta, no un error: dice exactamente lo que pasa.
    #[test]
    fn an_unrendered_page_yields_nothing() {
        let shell = r#"<html><head><script id="__UNIVERSAL_DATA_FOR_REHYDRATION__"
            type="application/json">{"__DEFAULT_SCOPE__":{"webapp.app-context":{}}}</script>
            </head><body><div id="app"></div></body></html>"#;
        assert!(extract_live_channels(shell).is_empty());
    }
}
