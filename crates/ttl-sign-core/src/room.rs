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

/// `status` reported by a room that is currently broadcasting.
///
/// One definition for both [`RoomLookup`] and [`RoomInfo`]. They used to disagree — the
/// lookup accepted anything except the "ended" marker while room info required this exact
/// value — so the same room could be live by one and offline by the other.
const ROOM_STATUS_LIVE: i64 = 2;
const EMPTY_ROOM_ID: &str = "0";

/// Is this the `status` of a room that is broadcasting right now?
///
/// Deliberately an allowlist: an unrecognised status means "not known to be live", which
/// fails by skipping a room rather than by connecting to one that ended.
pub fn is_live_status(status: i64) -> bool {
    status == ROOM_STATUS_LIVE
}

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
    /// TikTok reports `2` while broadcasting and `4` once the session ended. The `room_id`
    /// remains present afterwards, so checking only that it exists is insufficient: signing
    /// an offline room returns a protobuf without `push_server`, indistinguishable from a
    /// rejection.
    pub fn is_live(&self) -> bool {
        is_live_status(self.status) && !self.room_id.is_empty() && self.room_id != EMPTY_ROOM_ID
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

// --- Signed webcast REST endpoints -------------------------------------------------
//
// These are **not** unsigned like the lookup above. They live on `webcast.tiktok.com` and
// require the SDK signature, but the page's patched `fetch` adds it automatically, so the
// engine only has to build the URL and issue the request from inside the page.
//
// Verified against a real room on 2026-08-10: both answer `200` with
// `Response.type == "cors"`, so the body is readable from the page (unlike
// `/webcast/im/fetch/`, which returns no CORS headers).

const WEBCAST_API_BASE: &str = "https://webcast.tiktok.com/webcast";
const WEBCAST_APP_ID: &str = "1988";
const WEBCAST_APP_LANGUAGE: &str = "en";
const WEBCAST_DEVICE_PLATFORM: &str = "web";

fn webcast_url(path: &str, room_id: &str) -> String {
    format!(
        "{WEBCAST_API_BASE}/{path}/?aid={WEBCAST_APP_ID}&app_language={WEBCAST_APP_LANGUAGE}\
         &device_platform={WEBCAST_DEVICE_PLATFORM}&room_id={room_id}"
    )
}

/// Full metadata for one room: title, owner, cover, and counters.
pub fn room_info_url(room_id: &str) -> String {
    webcast_url("room/info", room_id)
}

/// Every gift available in a room, with its diamond cost and icon.
///
/// The response is large (about 2.6 MB for 626 gifts), so callers should request it once
/// per session rather than per event.
pub fn gift_list_url(room_id: &str) -> String {
    webcast_url("gift/list", room_id)
}

/// TikTok refused a webcast request while still answering `200`.
///
/// Every `webcast.tiktok.com` JSON endpoint reports success as `status_code: 0` and failure
/// as a non-zero code with a human-readable message. Deliberately **not** an enum of known
/// codes: the interesting refusals (rate limiting, verification) have not been observed
/// first-hand, and inventing constants for them would produce a classifier that silently
/// mislabels whatever TikTok actually sends. The code and message are passed through so a
/// caller can log and react to the real value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebcastRefusal {
    pub status_code: i64,
    pub message: String,
}

impl std::fmt::Display for WebcastRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() {
            write!(
                f,
                "TikTok refused the request (status_code={})",
                self.status_code
            )
        } else {
            write!(
                f,
                "TikTok refused the request (status_code={}): {}",
                self.status_code, self.message
            )
        }
    }
}

/// Read the refusal out of a webcast JSON envelope, if it is one.
///
/// `None` means the response reported success, or is not a webcast envelope at all.
pub fn webcast_refusal(raw: &str) -> Option<WebcastRefusal> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let status_code = value.get("status_code")?.as_i64()?;
    if status_code == 0 {
        return None;
    }
    let data = value.get("data");
    // TikTok puts the reason in `data.message`, and sometimes only in `data.prompts`.
    let message = data
        .map(|data| string_at(data, "message"))
        .filter(|message| !message.is_empty())
        .or_else(|| data.map(|data| string_at(data, "prompts")))
        .unwrap_or_default();
    Some(WebcastRefusal {
        status_code,
        message,
    })
}

/// The broadcaster of a room.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomOwner {
    pub id: String,
    /// `@handle`, reported by TikTok as `display_id`.
    pub unique_id: String,
    pub nickname: String,
    pub sec_uid: String,
    pub avatar_url: String,
    pub follower_count: u64,
    pub following_count: u64,
}

/// Room metadata from [`room_info_url`].
///
/// This is the equivalent of `tiktok-live-connector`'s `roomInfo`, and it is the state a
/// listener cannot reconstruct from the event stream: the stream reports *changes*, while
/// this reports the room as it already is at connect time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomInfo {
    pub room_id: String,
    pub title: String,
    /// `2` while broadcasting, `4` once the session ended.
    pub status: i64,
    /// Unix seconds at which the broadcast started.
    pub create_time: u64,
    /// Viewers watching right now.
    pub viewer_count: u64,
    /// Distinct viewers since the broadcast started.
    pub total_viewers: u64,
    pub like_count: u64,
    pub comment_count: u64,
    pub share_count: u64,
    /// New follows gained during this broadcast.
    pub follow_count: u64,
    pub cover_url: String,
    pub share_url: String,
    pub owner: RoomOwner,
}

impl RoomInfo {
    pub fn is_live(&self) -> bool {
        is_live_status(self.status)
    }

    /// Parse the response from [`room_info_url`].
    ///
    /// Returns `None` when TikTok reports a non-zero `status_code`, which is how it signals
    /// a rejected or malformed request while still answering `200`.
    pub fn from_json(raw: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        if value.get("status_code").and_then(serde_json::Value::as_i64) != Some(0) {
            return None;
        }
        let data = value.get("data")?;
        let stats = data.get("stats");
        let owner = data.get("owner");
        let follow_info = owner.and_then(|owner| owner.get("follow_info"));

        Some(Self {
            room_id: string_at(data, "id_str"),
            title: string_at(data, "title"),
            status: data
                .get("status")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default(),
            create_time: number_at(data, "create_time"),
            viewer_count: number_at(data, "user_count"),
            total_viewers: stats
                .map(|s| number_at(s, "total_user"))
                .unwrap_or_default(),
            like_count: stats
                .map(|s| number_at(s, "like_count"))
                .unwrap_or_default(),
            comment_count: stats
                .map(|s| number_at(s, "comment_count"))
                .unwrap_or_default(),
            share_count: stats
                .map(|s| number_at(s, "share_count"))
                .unwrap_or_default(),
            follow_count: stats
                .map(|s| number_at(s, "follow_count"))
                .unwrap_or_default(),
            cover_url: data.get("cover").map(first_url).unwrap_or_default(),
            share_url: string_at(data, "share_url"),
            owner: RoomOwner {
                id: owner.map(|o| string_at(o, "id_str")).unwrap_or_default(),
                unique_id: owner
                    .map(|o| string_at(o, "display_id"))
                    .unwrap_or_default(),
                nickname: owner.map(|o| string_at(o, "nickname")).unwrap_or_default(),
                sec_uid: owner.map(|o| string_at(o, "sec_uid")).unwrap_or_default(),
                avatar_url: owner
                    .and_then(|o| o.get("avatar_thumb"))
                    .map(first_url)
                    .unwrap_or_default(),
                follower_count: follow_info
                    .map(|f| number_at(f, "follower_count"))
                    .unwrap_or_default(),
                following_count: follow_info
                    .map(|f| number_at(f, "following_count"))
                    .unwrap_or_default(),
            },
        })
    }
}

/// One gift available in a room, from [`gift_list_url`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gift {
    pub id: u64,
    pub name: String,
    /// Sentence TikTok displays when the gift is sent, for example `sent Rose`.
    pub describe: String,
    /// Cost in diamonds. This is what turns a `WebcastGiftMessage` into a value.
    pub diamond_count: u64,
    /// `true` when repeated sends are combined into one streak.
    pub combo: bool,
    pub icon_url: String,
}

/// Parse the response from [`gift_list_url`].
///
/// Returns `None` only when the response is not a successful gift list; an empty list is
/// returned as an empty `Vec`, because a room genuinely can offer no gifts.
pub fn parse_gift_list(raw: &str) -> Option<Vec<Gift>> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("status_code").and_then(serde_json::Value::as_i64) != Some(0) {
        return None;
    }
    let gifts = value.get("data")?.get("gifts")?.as_array()?;
    Some(
        gifts
            .iter()
            .map(|gift| Gift {
                id: number_at(gift, "id"),
                name: string_at(gift, "name"),
                describe: string_at(gift, "describe"),
                diamond_count: number_at(gift, "diamond_count"),
                combo: gift
                    .get("combo")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or_default(),
                icon_url: gift.get("icon").map(first_url).unwrap_or_default(),
            })
            .collect(),
    )
}

/// First entry of a TikTok image object's `url_list`.
fn first_url(image: &serde_json::Value) -> String {
    image
        .get("url_list")
        .and_then(serde_json::Value::as_array)
        .and_then(|list| list.first())
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Counter that TikTok sends as a number in some fields and as a string in others.
fn number_at(value: &serde_json::Value, key: &str) -> u64 {
    let Some(field) = value.get(key) else {
        return 0;
    };
    field
        .as_u64()
        .or_else(|| field.as_str().and_then(|raw| raw.parse().ok()))
        .unwrap_or_default()
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

    /// The lookup and room info must never disagree about the same status.
    #[test]
    fn both_room_views_share_one_definition_of_live() {
        for status in [0, 1, 2, 3, 4, 5] {
            let lookup = RoomLookup {
                unique_id: "x".into(),
                room_id: "7300".into(),
                nickname: String::new(),
                status,
                title: String::new(),
            };
            let info = RoomInfo {
                status,
                ..RoomInfo::default()
            };
            assert_eq!(lookup.is_live(), info.is_live(), "status={status}");
            assert_eq!(info.is_live(), is_live_status(status));
        }
        // An unrecognised status is not treated as live.
        assert!(!is_live_status(0));
        assert!(is_live_status(2));
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

    /// Truncated real response shape for `/webcast/room/info/`, captured 2026-08-10.
    const ROOM_INFO_JSON: &str = r#"{
      "data": {
        "id_str": "7672427094097333000",
        "title": "late night stream",
        "status": 2,
        "create_time": 1786376160,
        "user_count": 6,
        "like_count": 0,
        "share_url": "https://m.tiktok.com/share/live/7672427094097333000/",
        "cover": { "url_list": ["https://p16.tiktokcdn.com/cover.webp", "https://p19.tiktokcdn.com/cover.webp"] },
        "stats": {
          "total_user": 165, "like_count": 42, "comment_count": 7,
          "share_count": 3, "follow_count": 4, "enter_count": 165
        },
        "owner": {
          "id_str": "7591454537077752850",
          "display_id": "diegogod279",
          "nickname": "paisana jacinta",
          "sec_uid": "MS4wLjABAAAA",
          "avatar_thumb": { "url_list": ["https://p16.tiktokcdn.com/avatar.webp"] },
          "follow_info": { "follower_count": 542, "following_count": 28 }
        }
      },
      "status_code": 0
    }"#;

    #[test]
    fn parses_room_info() {
        let info = RoomInfo::from_json(ROOM_INFO_JSON).unwrap();
        assert_eq!(info.room_id, "7672427094097333000");
        assert_eq!(info.title, "late night stream");
        assert!(info.is_live());
        assert_eq!(info.create_time, 1786376160);
        assert_eq!(info.viewer_count, 6);
        assert_eq!(info.total_viewers, 165);
        // `stats.like_count` is the real counter; the top-level field stays 0.
        assert_eq!(info.like_count, 42);
        assert_eq!(info.comment_count, 7);
        assert_eq!(info.follow_count, 4);
        assert_eq!(info.cover_url, "https://p16.tiktokcdn.com/cover.webp");
        assert_eq!(info.owner.unique_id, "diegogod279");
        assert_eq!(info.owner.nickname, "paisana jacinta");
        assert_eq!(info.owner.follower_count, 542);
        assert_eq!(
            info.owner.avatar_url,
            "https://p16.tiktokcdn.com/avatar.webp"
        );
    }

    /// TikTok answers `200` with a non-zero `status_code` for a rejected request; that is a
    /// failure, not a room with empty fields.
    #[test]
    fn a_rejected_room_info_is_not_parsed_as_an_empty_room() {
        let rejected = r#"{"data":{"message":"Request params error"},"status_code":10011}"#;
        assert!(RoomInfo::from_json(rejected).is_none());
        assert!(RoomInfo::from_json("not json").is_none());
        assert!(parse_gift_list(rejected).is_none());
    }

    /// A refusal must be reported as such, with TikTok's own code and words, instead of
    /// being flattened into "could not parse".
    #[test]
    fn a_refusal_is_read_out_of_the_envelope() {
        // Shape captured from a real `ranklist/online_audience` rejection.
        let raw = r#"{"data":{"message":"Request params error","prompts":"Request params error"},
                      "extra":{"now":1786378962758},"status_code":10011}"#;
        let refusal = webcast_refusal(raw).expect("this is a refusal");
        assert_eq!(refusal.status_code, 10011);
        assert_eq!(refusal.message, "Request params error");
        assert!(refusal.to_string().contains("10011"));
        assert!(refusal.to_string().contains("Request params error"));
    }

    #[test]
    fn a_successful_envelope_is_not_a_refusal() {
        assert!(webcast_refusal(ROOM_INFO_JSON).is_none());
        // Not a webcast envelope at all.
        assert!(webcast_refusal(r#"{"anything":1}"#).is_none());
        assert!(webcast_refusal("<html>").is_none());
    }

    /// Some refusals carry the reason only in `prompts`, and some carry no words at all.
    #[test]
    fn a_refusal_without_a_message_still_reports_its_code() {
        let only_prompts = webcast_refusal(r#"{"data":{"prompts":"slow down"},"status_code":8}"#)
            .expect("this is a refusal");
        assert_eq!(only_prompts.message, "slow down");

        let bare = webcast_refusal(r#"{"status_code":99}"#).expect("this is a refusal");
        assert_eq!(bare.status_code, 99);
        assert!(bare.message.is_empty());
        assert!(bare.to_string().contains("99"));
    }

    #[test]
    fn parses_the_gift_list() {
        let raw = r#"{
          "data": { "gifts": [
            { "id": 231956, "name": "Clap Clap", "describe": "sent Clap Clap",
              "diamond_count": 1, "combo": true,
              "icon": { "url_list": ["https://p16.tiktokcdn.com/clap.webp"] } },
            { "id": 5655, "name": "Rose", "diamond_count": 1 }
          ] },
          "status_code": 0
        }"#;
        let gifts = parse_gift_list(raw).unwrap();
        assert_eq!(gifts.len(), 2);
        assert_eq!(gifts[0].id, 231956);
        assert_eq!(gifts[0].name, "Clap Clap");
        assert_eq!(gifts[0].diamond_count, 1);
        assert!(gifts[0].combo);
        assert_eq!(gifts[0].icon_url, "https://p16.tiktokcdn.com/clap.webp");
        // Missing optional fields are absent, not a parse failure.
        assert_eq!(gifts[1].icon_url, "");
        assert!(!gifts[1].combo);
    }

    #[test]
    fn an_empty_gift_list_is_not_an_error() {
        let gifts = parse_gift_list(r#"{"data":{"gifts":[]},"status_code":0}"#).unwrap();
        assert!(gifts.is_empty());
    }

    #[test]
    fn webcast_urls_carry_the_room_and_app_id() {
        let url = room_info_url("7300");
        assert!(
            url.starts_with("https://webcast.tiktok.com/webcast/room/info/?"),
            "{url}"
        );
        assert!(url.contains("aid=1988"));
        assert!(url.ends_with("room_id=7300"));
        assert!(gift_list_url("7300").contains("/webcast/gift/list/"));
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
