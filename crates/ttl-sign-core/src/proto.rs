//! Minimal protobuf reader.
//!
//! Event parsing is **out of scope** (`README.md`): this module decodes only what is needed
//! to open the WebSocket and answer `ack` frames, without `prost` or generated code.
//!
//! Decoding operates directly on the wire format, so unknown fields are ignored instead of
//! breaking parsing when TikTok adds fields without notice.
//!
//! Field numbers from the schema used by reference clients:
//!
//! ```text
//! ProtoMessageFetchResult          WebcastPushFrame
//!   1  messages (repeated)           1  seq_id
//!   2  cursor                        2  log_id
//!   5  internal_ext                  3  service
//!   7  route_params (map)            4  method
//!   8  heartbeat_duration            5  headers (map)
//!   9  need_ack                      6  payload_encoding
//!  10  push_server                   7  payload_type
//!                                    8  payload
//! ```
//!
//! > **F1 pending:** confirm these numbers against `fixtures/f0/im_fetch.pb`.
//! > `FetchResult::decode` does not fail when they differ: it returns empty fields, and
//! > validation in `docs/05` turns this into a typed error.

use std::fmt;

// --- Field numbers ----------------------------------------------------------------

mod fetch_field {
    pub const CURSOR: u32 = 2;
    pub const INTERNAL_EXT: u32 = 5;
    pub const ROUTE_PARAMS: u32 = 7;
    pub const HEARTBEAT_DURATION: u32 = 8;
    pub const NEED_ACK: u32 = 9;
    pub const PUSH_SERVER: u32 = 10;
}

mod frame_field {
    pub const SEQ_ID: u32 = 1;
    pub const LOG_ID: u32 = 2;
    pub const SERVICE: u32 = 3;
    pub const METHOD: u32 = 4;
    pub const HEADERS: u32 = 5;
    pub const PAYLOAD_ENCODING: u32 = 6;
    pub const PAYLOAD_TYPE: u32 = 7;
    pub const PAYLOAD: u32 = 8;
}

/// Fields of a `map<string, string>` entry.
mod map_entry_field {
    pub const KEY: u32 = 1;
    pub const VALUE: u32 = 2;
}

const ENTER_ROOM_FIELD_ROOM_ID: u32 = 1;
const ENTER_ROOM_FIELD_ROOM_TYPE: u32 = 4;
const ENTER_ROOM_ROOM_TYPE_AUDIENCE: u64 = 12;
const ENTER_ROOM_FIELD_USER_TYPE: u32 = 5;
const ENTER_ROOM_FIELD_FILTER_WELCOME: u32 = 9;
const ENTER_ROOM_FILTER_WELCOME_DISABLED: &str = "0";
const HEARTBEAT_FIELD_ROOM_ID: u32 = 1;
const HEARTBEAT_FIELD_SEQUENCE_ID: u32 = 2;

// --- Errores ----------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum ProtoError {
    /// The buffer ended halfway through a field.
    Truncated,
    /// Varint longer than 10 bytes.
    VarintOverflow,
    /// Wire type unknown to this decoder (3 and 4 are obsolete groups).
    UnsupportedWireType(u8),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "truncated buffer"),
            Self::VarintOverflow => write!(f, "varint is too long"),
            Self::UnsupportedWireType(w) => write!(f, "unsupported wire type: {w}"),
        }
    }
}

impl std::error::Error for ProtoError {}

// --- Decodificador de bajo nivel --------------------------------------------------

/// Field value as represented on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireValue<'a> {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32),
    Bytes(&'a [u8]),
}

impl<'a> WireValue<'a> {
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&'a str> {
        self.as_bytes().and_then(|b| std::str::from_utf8(b).ok())
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Varint(v) => Some(*v),
            Self::Fixed64(v) => Some(*v),
            Self::Fixed32(v) => Some(u64::from(*v)),
            Self::Bytes(_) => None,
        }
    }
}

/// Iterate over protobuf message fields without knowing the schema.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn varint(&mut self) -> Result<u64, ProtoError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self.buf.get(self.pos).ok_or(ProtoError::Truncated)?;
            self.pos += 1;
            if shift >= 64 {
                return Err(ProtoError::VarintOverflow);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        let end = self.pos.checked_add(n).ok_or(ProtoError::Truncated)?;
        let slice = self.buf.get(self.pos..end).ok_or(ProtoError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    /// Next field, or `None` at the end of the message.
    pub fn next_field(&mut self) -> Option<Result<(u32, WireValue<'a>), ProtoError>> {
        if self.pos >= self.buf.len() {
            return None;
        }
        Some(self.read_field())
    }

    fn read_field(&mut self) -> Result<(u32, WireValue<'a>), ProtoError> {
        let tag = self.varint()?;
        let field = (tag >> 3) as u32;
        let wire_type = (tag & 0x7) as u8;
        let value = match wire_type {
            0 => WireValue::Varint(self.varint()?),
            1 => {
                let b = self.take(8)?;
                WireValue::Fixed64(u64::from_le_bytes(b.try_into().expect("8 bytes")))
            }
            2 => {
                let len = self.varint()? as usize;
                WireValue::Bytes(self.take(len)?)
            }
            5 => {
                let b = self.take(4)?;
                WireValue::Fixed32(u32::from_le_bytes(b.try_into().expect("4 bytes")))
            }
            other => return Err(ProtoError::UnsupportedWireType(other)),
        };
        Ok((field, value))
    }
}

// --- Codificador de bajo nivel ----------------------------------------------------

/// Write protobuf messages. Only the parts needed for `ack`, heartbeats, and the
/// initial room entry are implemented.
#[derive(Debug, Default, Clone)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    fn varint(&mut self, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    fn tag(&mut self, field: u32, wire_type: u8) {
        self.varint((u64::from(field) << 3) | u64::from(wire_type));
    }

    /// Varint field. Omitted when zero, as proto3 does.
    pub fn u64_field(&mut self, field: u32, value: u64) -> &mut Self {
        if value != 0 {
            self.tag(field, 0);
            self.varint(value);
        }
        self
    }

    pub fn bool_field(&mut self, field: u32, value: bool) -> &mut Self {
        self.u64_field(field, u64::from(value))
    }

    /// `bytes`/`string`/submessage field. Omitted when empty.
    pub fn bytes_field(&mut self, field: u32, value: &[u8]) -> &mut Self {
        if !value.is_empty() {
            self.tag(field, 2);
            self.varint(value.len() as u64);
            self.buf.extend_from_slice(value);
        }
        self
    }

    pub fn str_field(&mut self, field: u32, value: &str) -> &mut Self {
        self.bytes_field(field, value.as_bytes())
    }

    /// `map<string, string>` entry.
    pub fn map_entry(&mut self, field: u32, key: &str, value: &str) -> &mut Self {
        let mut entry = Writer::new();
        entry.str_field(map_entry_field::KEY, key);
        entry.str_field(map_entry_field::VALUE, value);
        self.bytes_field(field, &entry.finish())
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Decode a `map<string, string>` entry.
fn decode_map_entry(buf: &[u8]) -> Option<(String, String)> {
    let mut key = None;
    let mut value = String::new();
    let mut reader = Reader::new(buf);
    while let Some(field) = reader.next_field() {
        let (number, wire) = field.ok()?;
        match number {
            map_entry_field::KEY => key = Some(wire.as_str()?.to_owned()),
            map_entry_field::VALUE => value = wire.as_str()?.to_owned(),
            _ => {}
        }
    }
    key.map(|k| (k, value))
}

/// Structure of an uninterpreted message: `(field, type, size)`.
///
/// Use this to confirm field numbers against a real response before relying on this module.
pub fn describe(buf: &[u8]) -> Result<Vec<(u32, &'static str, usize)>, ProtoError> {
    let mut out = Vec::new();
    let mut reader = Reader::new(buf);
    while let Some(field) = reader.next_field() {
        let (number, wire) = field?;
        let (kind, size) = match wire {
            WireValue::Varint(v) => ("varint", v.to_string().len()),
            WireValue::Fixed64(_) => ("fixed64", 8),
            WireValue::Fixed32(_) => ("fixed32", 4),
            WireValue::Bytes(b) => ("bytes", b.len()),
        };
        out.push((number, kind, size));
    }
    Ok(out)
}

// --- ProtoMessageFetchResult ------------------------------------------------------

/// The subset of `ProtoMessageFetchResult` required to open the WebSocket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchResult {
    /// WebSocket URI base. Empty in a 200 response means silent rejection.
    pub push_server: String,
    /// Parameters **already signed by TikTok**. Used as-is.
    pub route_params: Vec<(String, String)>,
    /// Required; without it the response is invalid.
    pub cursor: String,
    /// Payload for `ack` frames.
    pub internal_ext: String,
    pub heartbeat_duration: u64,
    pub need_ack: bool,
}

impl FetchResult {
    /// Decode known fields and ignore the rest.
    ///
    /// No validation: an empty `push_server` is returned as-is and is
    /// [`FetchResult::rejection_reason`] quien lo interpreta.
    pub fn decode(buf: &[u8]) -> Result<Self, ProtoError> {
        let mut out = Self::default();
        let mut reader = Reader::new(buf);
        while let Some(field) = reader.next_field() {
            let (number, wire) = field?;
            match number {
                fetch_field::CURSOR => out.cursor = wire.as_str().unwrap_or_default().to_owned(),
                fetch_field::INTERNAL_EXT => {
                    out.internal_ext = wire.as_str().unwrap_or_default().to_owned()
                }
                fetch_field::PUSH_SERVER => {
                    out.push_server = wire.as_str().unwrap_or_default().to_owned()
                }
                fetch_field::HEARTBEAT_DURATION => {
                    out.heartbeat_duration = wire.as_u64().unwrap_or_default()
                }
                fetch_field::NEED_ACK => out.need_ack = wire.as_u64().unwrap_or_default() != 0,
                fetch_field::ROUTE_PARAMS => {
                    if let Some(entry) = wire.as_bytes().and_then(decode_map_entry) {
                        out.route_params.push(entry);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Reason this response is a rejection, if applicable.
    ///
    /// A 200 with empty `push_server` means TikTok rejected silently
    /// (`docs/00-research.md` §1).
    pub fn rejection_reason(&self) -> Option<crate::RejectReason> {
        if self.push_server.is_empty() || self.route_params.is_empty() {
            Some(crate::RejectReason::EmptyPushServer)
        } else {
            None
        }
    }
}

// --- WebcastPushFrame -------------------------------------------------------------

/// WebSocket transport envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushFrame {
    pub seq_id: u64,
    pub log_id: u64,
    pub service: u64,
    pub method: u64,
    pub headers: Vec<(String, String)>,
    pub payload_encoding: String,
    /// `msg` lleva eventos; `hb`, `ack`, `im_enter_room_resp` son transporte.
    pub payload_type: String,
    pub payload: Vec<u8>,
}

impl PushFrame {
    pub fn decode(buf: &[u8]) -> Result<Self, ProtoError> {
        let mut out = Self::default();
        let mut reader = Reader::new(buf);
        while let Some(field) = reader.next_field() {
            let (number, wire) = field?;
            match number {
                frame_field::SEQ_ID => out.seq_id = wire.as_u64().unwrap_or_default(),
                frame_field::LOG_ID => out.log_id = wire.as_u64().unwrap_or_default(),
                frame_field::SERVICE => out.service = wire.as_u64().unwrap_or_default(),
                frame_field::METHOD => out.method = wire.as_u64().unwrap_or_default(),
                frame_field::HEADERS => {
                    if let Some(entry) = wire.as_bytes().and_then(decode_map_entry) {
                        out.headers.push(entry);
                    }
                }
                frame_field::PAYLOAD_ENCODING => {
                    out.payload_encoding = wire.as_str().unwrap_or_default().to_owned()
                }
                frame_field::PAYLOAD_TYPE => {
                    out.payload_type = wire.as_str().unwrap_or_default().to_owned()
                }
                frame_field::PAYLOAD => out.payload = wire.as_bytes().unwrap_or_default().to_vec(),
                _ => {}
            }
        }
        Ok(out)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64_field(frame_field::SEQ_ID, self.seq_id)
            .u64_field(frame_field::LOG_ID, self.log_id)
            .u64_field(frame_field::SERVICE, self.service)
            .u64_field(frame_field::METHOD, self.method);
        for (k, v) in &self.headers {
            w.map_entry(frame_field::HEADERS, k, v);
        }
        w.str_field(frame_field::PAYLOAD_ENCODING, &self.payload_encoding)
            .str_field(frame_field::PAYLOAD_TYPE, &self.payload_type)
            .bytes_field(frame_field::PAYLOAD, &self.payload);
        w.finish()
    }

    /// Does it contain events? Only `msg`; all others are transport and discarded.
    pub fn is_message(&self) -> bool {
        self.payload_type == "msg"
    }

    /// Value of `compress_type` in frame headers.
    pub fn compress_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == "compress_type")
            .map(|(_, v)| v.as_str())
    }

    /// The `ack` corresponding to this frame (`docs/05` §Ack).
    ///
    /// Payload is `internal_ext`, or `-` when empty.
    pub fn ack(&self, internal_ext: &str) -> Self {
        let payload = if internal_ext.is_empty() {
            "-"
        } else {
            internal_ext
        };
        Self {
            log_id: self.log_id,
            payload_type: "ack".into(),
            payload_encoding: "pb".into(),
            payload: payload.as_bytes().to_vec(),
            ..Default::default()
        }
    }

    /// Frame sent immediately after the handshake to enter the room.
    ///
    /// `room_id` is not in the envelope: it is inside `WebcastImEnterRoomMessage`, the
    /// the payload of `im_enter_room`. Without this frame TikTok may accept 101 and leave
    /// the socket completely silent.
    pub fn enter_room(room_id: u64) -> Self {
        let mut writer = Writer::new();
        writer
            .u64_field(ENTER_ROOM_FIELD_ROOM_ID, room_id)
            .u64_field(ENTER_ROOM_FIELD_ROOM_TYPE, ENTER_ROOM_ROOM_TYPE_AUDIENCE)
            .str_field(ENTER_ROOM_FIELD_USER_TYPE, "audience")
            .str_field(
                ENTER_ROOM_FIELD_FILTER_WELCOME,
                ENTER_ROOM_FILTER_WELCOME_DISABLED,
            );

        Self {
            payload_encoding: "pb".into(),
            payload_type: "im_enter_room".into(),
            payload: writer.finish(),
            ..Self::default()
        }
    }

    /// Application heartbeat. The payload is `HeartbeatMessage`:
    /// `room_id=1` and `send_packet_seq_id=2`.
    pub fn heartbeat(room_id: u64, sequence_id: u64) -> Self {
        let mut writer = Writer::new();
        writer
            .u64_field(HEARTBEAT_FIELD_ROOM_ID, room_id)
            .u64_field(HEARTBEAT_FIELD_SEQUENCE_ID, sequence_id);

        Self {
            payload_encoding: "pb".into(),
            payload_type: "hb".into(),
            payload: writer.finish(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic `ProtoMessageFetchResult` using the documented schema.
    fn sample_fetch_result() -> Vec<u8> {
        let mut w = Writer::new();
        w.bytes_field(1, b"\x0a\x03msg") // messages[0], ignored
            .str_field(fetch_field::CURSOR, "1720000000000_abc")
            .str_field(fetch_field::INTERNAL_EXT, "internal_src:dim|...")
            .map_entry(fetch_field::ROUTE_PARAMS, "wss_push_room_id", "7300")
            .map_entry(fetch_field::ROUTE_PARAMS, "wss_push_did", "7299")
            .u64_field(fetch_field::HEARTBEAT_DURATION, 10000)
            .bool_field(fetch_field::NEED_ACK, true)
            .str_field(
                fetch_field::PUSH_SERVER,
                "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/",
            );
        w.finish()
    }

    #[test]
    fn decodes_the_fields_the_websocket_needs() {
        let r = FetchResult::decode(&sample_fetch_result()).unwrap();
        assert_eq!(
            r.push_server,
            "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/"
        );
        assert_eq!(r.cursor, "1720000000000_abc");
        assert_eq!(r.internal_ext, "internal_src:dim|...");
        assert_eq!(r.heartbeat_duration, 10000);
        assert!(r.need_ack);
        assert_eq!(
            r.route_params,
            vec![
                ("wss_push_room_id".to_string(), "7300".to_string()),
                ("wss_push_did".to_string(), "7299".to_string()),
            ]
        );
        assert_eq!(r.rejection_reason(), None);
    }

    #[test]
    fn unknown_fields_are_skipped_not_fatal() {
        let mut w = Writer::new();
        w.u64_field(999, 7)
            .bytes_field(998, b"anything")
            .str_field(fetch_field::CURSOR, "c");
        let r = FetchResult::decode(&w.finish()).unwrap();
        assert_eq!(r.cursor, "c");
    }

    #[test]
    fn empty_push_server_is_a_rejection() {
        let r = FetchResult::decode(
            &Writer::new()
                .str_field(fetch_field::CURSOR, "c")
                .clone()
                .finish(),
        )
        .unwrap();
        assert_eq!(
            r.rejection_reason(),
            Some(crate::RejectReason::EmptyPushServer)
        );
    }

    #[test]
    fn truncated_input_errors_instead_of_panicking() {
        let full = sample_fetch_result();
        assert_eq!(
            FetchResult::decode(&full[..full.len() - 3]),
            Err(ProtoError::Truncated)
        );
    }

    #[test]
    fn push_frame_roundtrips() {
        let frame = PushFrame {
            seq_id: 3,
            log_id: 42,
            service: 1,
            method: 2,
            headers: vec![("compress_type".into(), "gzip".into())],
            payload_encoding: "pb".into(),
            payload_type: "msg".into(),
            payload: vec![1, 2, 3],
        };
        assert_eq!(PushFrame::decode(&frame.encode()).unwrap(), frame);
        assert!(frame.is_message());
        assert_eq!(frame.compress_type(), Some("gzip"));
    }

    #[test]
    fn ack_carries_log_id_and_internal_ext() {
        let received = PushFrame {
            log_id: 99,
            payload_type: "msg".into(),
            ..Default::default()
        };
        let ack = received.ack("internal_src:dim");
        assert_eq!(ack.log_id, 99);
        assert_eq!(ack.payload_type, "ack");
        assert_eq!(ack.payload_encoding, "pb");
        assert_eq!(ack.payload, b"internal_src:dim");

        // Without internal_ext, the payload is "-", not empty.
        assert_eq!(received.ack("").payload, b"-");
    }

    #[test]
    fn enter_room_frame_has_the_expected_payload() {
        let frame = PushFrame::enter_room(7_672_379_773_977_037_584);
        assert_eq!(frame.payload_encoding, "pb");
        assert_eq!(frame.payload_type, "im_enter_room");
        assert_eq!(PushFrame::decode(&frame.encode()).unwrap(), frame);

        let mut reader = Reader::new(&frame.payload);
        let fields: Vec<_> = std::iter::from_fn(|| reader.next_field()).collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].as_ref().unwrap().0, 1);
        assert_eq!(
            fields[0].as_ref().unwrap().1.as_u64(),
            Some(7_672_379_773_977_037_584)
        );
        assert_eq!(fields[1].as_ref().unwrap().0, 4);
        assert_eq!(fields[1].as_ref().unwrap().1.as_u64(), Some(12));
        assert_eq!(fields[2].as_ref().unwrap().0, 5);
        assert_eq!(fields[2].as_ref().unwrap().1.as_str(), Some("audience"));
        assert_eq!(fields[3].as_ref().unwrap().0, 9);
        assert_eq!(fields[3].as_ref().unwrap().1.as_str(), Some("0"));
    }
}
