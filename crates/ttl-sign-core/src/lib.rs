//! Signer core: types and request construction.
//!
//! No network or GUI, so it can be tested in CI without a display.
//!
//! Modules:
//!
//! - [`preset`] — `DevicePreset` / `LocationPreset` / `ScreenPreset`: one source of truth
//!   for User-Agent **and** browser parameters.
//! - [`params`] — query construction for `/webcast/im/fetch/` and the WebSocket.
//! - [`cookie`] — minimal `CookieJar` in `X-Set-TT-Cookie` format.
//! - [`outcome`] — `SignOutcome`: rejection and transport errors never share a variant.
//! - [`room`] — unsigned `unique_id` → `room_id` lookup and live-channel discovery.
//! - [`proto`] — transport envelope decoding and a stable subset of common live events.
//! - [`full_schema`] — generated bindings for the bundled TikTok Webcast schema snapshot.

pub mod cookie;
pub mod full_schema;
pub mod outcome;
pub mod params;
pub mod preset;
pub mod proto;
pub mod room;

pub use cookie::CookieJar;
pub use full_schema::{
    decode_webcast_message, schema_by_name, schema_for_method, tik_tok, FieldKind, FieldSchema,
    MessageSchema, SchemaField, SchemaMessage, SchemaValue, GENERATED_SCHEMA_MESSAGE_COUNT,
    GENERATED_WEBCAST_METHOD_COUNT,
};
pub use outcome::{RejectReason, SignError, SignOutcome, SignedFetch};
pub use params::{FetchParams, Query, WsParams};
pub use preset::{DevicePreset, LocationPreset, Preset, ScreenPreset};
pub use proto::FetchResult;
pub use room::{
    extract_live_channels, gift_list_url, is_live_status, parse_gift_list, room_info_url,
    room_lookup_url, Gift, LiveChannel, RoomInfo, RoomLookup, RoomOwner,
};
