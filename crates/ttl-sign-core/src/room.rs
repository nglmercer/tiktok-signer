//! Flow step 1: `unique_id` → `room_id`. **Unsigned**.
//!
//! Two paths are required because TikTok serves these differently:
//!
//! | What | How | Signed |
//! |---|---|---|
//! | `unique_id` → `room_id` + status | `GET /api-live/user/room/?uniqueId=…`, JSON | no |
//! | Who is live now | **Rendered** DOM from `https://www.tiktok.com/live` | no |
//!
//! `/live` does not include the data in HTML; the client renders it. Therefore
//! [`extract_live_channels`] operates on the WebView DOM, not a raw `GET` that returns only
//! the shell.
//!
//! There is no I/O here: all functions are pure operations over caller-provided text.

use std::collections::BTreeMap;

const ROOM_STATUS_ENDED: i64 = 4;
const EMPTY_ROOM_ID: &str = "0";

/// Live exploration page. JavaScript is required to populate it.
pub const LIVE_EXPLORE_URL: &str = "https://www.tiktok.com/live";
const ROOM_LOOKUP_APP_ID: &str = "1988";
const ROOM_LOOKUP_SOURCE_TYPE: &str = "54";

/// URL for a user's live page.
pub fn live_page_url(unique_id: &str) -> String {
    format!(
        "https://www.tiktok.com/@{}/live",
        unique_id.trim_start_matches('@')
    )
}

/// Endpoint resolving `unique_id` → `room_id`. It requires no signature or cookies.
pub fn room_lookup_url(unique_id: &str) -> String {
    format!(
        "https://www.tiktok.com/api-live/user/room/?aid={ROOM_LOOKUP_APP_ID}&sourceType={ROOM_LOOKUP_SOURCE_TYPE}&uniqueId={}",
        unique_id.trim_start_matches('@')
    )
}

/// A user's room status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomLookup {
    pub unique_id: String,
    pub room_id: String,
    pub nickname: String,
    /// Raw `status` field. `4` means the live session ended; `2` means live.
    pub status: i64,
    pub title: String,
}

impl RoomLookup {
    /// Is the user live now?
    ///
    /// TikTok marks ended rooms with `4`. The `room_id` remains present, so checking only
    /// that it exists is insufficient: signing an offline room returns a protobuf without
    /// `push_server`, indistinguishable from a rejection.
    pub fn is_live(&self) -> bool {
        self.status != ROOM_STATUS_ENDED
            && !self.room_id.is_empty()
            && self.room_id != EMPTY_ROOM_ID
    }

    /// Parse the response from [`room_lookup_url`].
    pub fn from_json(raw: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        let user = value.get("data")?.get("user")?;
        let live_room = value.get("data").and_then(|d| d.get("liveRoom"));

        Some(Self {
            unique_id: string_at(user, "uniqueId"),
            room_id: string_at(user, "roomId"),
            nickname: string_at(user, "nickname"),
            // User and room status match; use the other field when one is missing.
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

/// A channel found on the exploration page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveChannel {
    pub unique_id: String,
    /// Empty when the page contained only a link; resolve it with [`room_lookup_url`].
    pub room_id: String,
    pub nickname: String,
}

/// Extract channels from the rendered DOM of `https://www.tiktok.com/live`.
///
/// Combine two sources because neither is reliable alone:
///
/// 1. DOM links `/@user/live` survive JSON schema changes but do not contain `room_id`.
/// 2. Any embedded JSON object containing both `uniqueId` **and** `roomId`, found recursively
///    without assuming its location. TikTok moves these keys often, but their names persist.
pub fn extract_live_channels(dom: &str) -> Vec<LiveChannel> {
    // BTreeMap provides stable ordering and unique_id deduplication.
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

/// `href="/@user/live"` → `user`.
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

/// JSON-declaring `<script>` contents, plus the full document when the provided DOM is
/// already JSON.
fn embedded_json_blobs(dom: &str) -> Vec<String> {
    let trimmed = dom.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return vec![dom.to_string()];
    }

    let mut out = Vec::new();
    for chunk in dom.split("<script").skip(1) {
        let Some(open) = chunk.find('>') else {
            continue;
        };
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

/// Walk the JSON tree looking for channel objects.
fn collect_channels(value: &serde_json::Value, out: &mut Vec<LiveChannel>) {
    match value {
        serde_json::Value::Object(map) => {
            let unique_id = map.get("uniqueId").and_then(serde_json::Value::as_str);
            let room_id = map.get("roomId").and_then(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });

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

    /// Truncated real response shape for `/api-live/user/room/`.
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

    /// `room_id` remains after a live session ends, so checking only its presence is not enough.
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
            <a href="/@carol">not a live stream</a>
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

    /// An unrendered `/live` page contains no channels. Returning an empty list is correct,
    /// not an error: it describes exactly what happened.
    #[test]
    fn an_unrendered_page_yields_nothing() {
        let shell = r#"<html><head><script id="__UNIVERSAL_DATA_FOR_REHYDRATION__"
            type="application/json">{"__DEFAULT_SCOPE__":{"webapp.app-context":{}}}</script>
            </head><body><div id="app"></div></body></html>"#;
        assert!(extract_live_channels(shell).is_empty());
    }
}
