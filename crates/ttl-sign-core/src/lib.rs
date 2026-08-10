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
//! - [`proto`] — minimal `ProtoMessageFetchResult` and `WebcastPushFrame` encoding.

pub mod cookie;
pub mod outcome;
pub mod params;
pub mod preset;
pub mod proto;
pub mod room;

pub use cookie::CookieJar;
pub use outcome::{RejectReason, SignError, SignOutcome, SignedFetch};
pub use params::{FetchParams, Query, WsParams};
pub use preset::{DevicePreset, LocationPreset, Preset, ScreenPreset};
pub use proto::FetchResult;
pub use room::{extract_live_channels, room_lookup_url, LiveChannel, RoomLookup};
