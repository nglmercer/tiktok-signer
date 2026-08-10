//! Núcleo del signer: tipos y construcción de requests.
//!
//! Sin red y sin GUI, para que sea testeable en CI sin display
//! (ver `docs/01-architecture.md`).
//!
//! Piezas:
//!
//! - [`preset`] — `DevicePreset` / `LocationPreset` / `ScreenPreset`: única fuente de
//!   verdad para User-Agent **y** parámetros de navegador.
//! - [`params`] — construcción de la query de `/webcast/im/fetch/` y de la del WebSocket.
//! - [`cookie`] — `CookieJar` mínimo, formato cookie-string (`X-Set-TT-Cookie`).
//! - [`outcome`] — `SignOutcome`: rechazo y error de transporte **nunca** comparten variante.
//! - [`room`] — paso 1 sin firma: `unique_id` → `room_id`, y descubrimiento de
//!   canales en directo.
//! - [`proto`] — lectura mínima de `ProtoMessageFetchResult` y (de)serialización de
//!   `WebcastPushFrame`, lo justo para abrir el WebSocket y responder los `ack`.

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
