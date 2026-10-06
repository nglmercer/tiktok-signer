use super::schema::{cardinality, proto_type, static_text};
use super::*;
use ttl_live_events::{EventEnvelope, JsonMode, LiveEvent, SchemaField, SchemaValue as V};

#[derive(Clone)]
pub struct NativeEvent {
    pub envelope: EventEnvelope,
    method: Vec<u8>,
    root: NativeObject,
}
#[derive(Clone)]
pub struct NativeObject {
    fields: Vec<NativeField>,
    truncated: bool,
}
#[derive(Clone)]
pub struct NativeField {
    field: SchemaField,
    object: Option<Box<NativeObject>>,
}
impl NativeObject {
    fn new(fields: &[SchemaField], truncated: bool) -> Self {
        Self {
            fields: fields
                .iter()
                .map(|f| NativeField {
                    field: f.clone(),
                    object: match &f.value {
                        V::Message(o) => Some(Box::new(Self::new(&o.fields, o.truncated))),
                        _ => None,
                    },
                })
                .collect(),
            truncated,
        }
    }
}
impl NativeEvent {
    pub(super) fn new(envelope: EventEnvelope) -> Self {
        Self {
            method: {
                let mut b = envelope.method().as_bytes().to_vec();
                b.push(0);
                b
            },
            root: NativeObject::new(&envelope.schema.fields, envelope.schema.truncated),
            envelope,
        }
    }
}
pub struct NativeBatch {
    pub(super) events: Vec<NativeEvent>,
    cursor: CString,
    internal_ext: CString,
    push_server: CString,
    need_ack: bool,
    heartbeat: i64,
}
impl NativeBatch {
    pub(super) fn new(b: ttl_live_events::EventBatch) -> Self {
        Self {
            events: b.events.into_iter().map(NativeEvent::new).collect(),
            cursor: cstring(&b.cursor),
            internal_ext: cstring(&b.internal_ext),
            push_server: cstring(&b.push_server),
            need_ack: b.need_ack,
            heartbeat: b.heartbeat_duration,
        }
    }
}
#[repr(C)]
pub struct EventInfo {
    pub abi_version: u32,
    pub method: *const c_char,
    pub method_len: usize,
    pub message_id: u64,
    pub support: i32,
    pub is_history: bool,
    pub schema_name: *const c_char,
    pub schema_name_len: usize,
    pub raw_payload_size: usize,
    pub method_id: u32,
    pub timestamp: u64,
    pub has_timestamp: bool,
}
#[no_mangle]
pub unsafe extern "C" fn ttl_event_decode(
    method: *const c_char,
    method_len: usize,
    message_id: u64,
    history: bool,
    payload: *const u8,
    payload_len: usize,
) -> *mut NativeEvent {
    super::guard(ptr::null_mut(), || {
        let Some(m) = text(method, method_len, 4096) else {
            return ptr::null_mut();
        };
        if m.contains('\0') || m.is_empty() {
            fail(
                ResultCode::InvalidArgument,
                "method must be nonempty and contain no NUL",
            );
            return ptr::null_mut();
        }
        let Some(p) = bytes(
            payload,
            payload_len.min(ttl_live_events::dynamic::MAX_EVENT_BYTES),
            ttl_live_events::dynamic::MAX_EVENT_BYTES,
        ) else {
            return ptr::null_mut();
        };
        let mut e = ttl_live_events::decode_event_envelope(m, message_id, history, p);
        if payload_len > p.len() {
            e.schema.truncated = true;
            e.event = LiveEvent::Unknown {
                method: m.to_owned(),
                payload: p.to_vec(),
            };
        }
        Box::into_raw(Box::new(NativeEvent::new(e)))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_free(e: *mut NativeEvent) {
    super::guard((), || {
        if !e.is_null() {
            drop(Box::from_raw(e));
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_clone(e: *const NativeEvent) -> *mut NativeEvent {
    super::guard(ptr::null_mut(), || {
        e.as_ref()
            .map_or(ptr::null_mut(), |e| Box::into_raw(Box::new(e.clone())))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_method(e: *const NativeEvent) -> *const c_char {
    super::guard(ptr::null(), || {
        e.as_ref().map_or(ptr::null(), |e| e.method.as_ptr().cast())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_method_len(e: *const NativeEvent) -> usize {
    super::guard(0, || e.as_ref().map_or(0, |e| e.envelope.method().len()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_message_id(e: *const NativeEvent) -> u64 {
    super::guard(0, || e.as_ref().map_or(0, |e| e.envelope.message_id()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_type(e: *const NativeEvent) -> u32 {
    super::guard(0, || e.as_ref().map_or(0, |e| e.envelope.method_id()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_is_history(e: *const NativeEvent) -> bool {
    super::guard(false, || {
        e.as_ref().is_some_and(|e| e.envelope.raw.is_history)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_support(e: *const NativeEvent) -> i32 {
    super::guard(0, || e.as_ref().map_or(0, |e| e.envelope.support() as i32))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_schema_name(e: *const NativeEvent) -> *const c_char {
    super::guard(ptr::null(), || {
        e.as_ref()
            .and_then(|e| e.envelope.schema.schema)
            .map_or(ptr::null(), |s| static_text(s.name))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_raw_size(e: *const NativeEvent) -> usize {
    super::guard(0, || {
        e.as_ref().map_or(0, |e| e.envelope.raw_payload().len())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_raw_data(e: *const NativeEvent) -> *const u8 {
    super::guard(ptr::null(), || {
        e.as_ref()
            .map_or(ptr::null(), |e| e.envelope.raw_payload().as_ptr())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_root(e: *const NativeEvent) -> *const NativeObject {
    super::guard(ptr::null(), || e.as_ref().map_or(ptr::null(), |e| &e.root))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_is_truncated(e: *const NativeEvent) -> bool {
    super::guard(false, || {
        e.as_ref().is_some_and(|e| e.envelope.schema.truncated)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_flags(e: *const NativeEvent) -> u32 {
    super::guard(0, || e.as_ref().map_or(0, |e| e.envelope.flags()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_timestamp(e: *const NativeEvent, out: *mut u64) -> bool {
    super::guard(false, || {
        let Some(v) = e.as_ref().and_then(|e| e.envelope.timestamp()) else {
            return false;
        };
        if out.is_null() {
            return false;
        }
        *out = v;
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_get_info(e: *const NativeEvent, out: *mut EventInfo) -> bool {
    super::guard(false, || {
        let Some(e) = e.as_ref() else {
            return false;
        };
        if out.is_null() {
            return false;
        }
        let schema_name = e.envelope.schema.schema.map(|s| s.name);
        *out = EventInfo {
            abi_version: 1,
            method: e.method.as_ptr().cast(),
            method_len: e.envelope.method().len(),
            message_id: e.envelope.message_id(),
            support: e.envelope.support() as i32,
            is_history: e.envelope.raw.is_history,
            schema_name: schema_name.map_or(ptr::null(), static_text),
            schema_name_len: schema_name.map_or(0, str::len),
            raw_payload_size: e.envelope.raw_payload().len(),
            method_id: e.envelope.method_id(),
            timestamp: e.envelope.timestamp().unwrap_or(0),
            has_timestamp: e.envelope.timestamp().is_some(),
        };
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_normalized_type(e: *const NativeEvent) -> i32 {
    super::guard(0, || {
        match e.as_ref().and_then(|e| e.envelope.normalized()) {
            Some(LiveEvent::Chat(_)) => 1,
            Some(LiveEvent::Gift(_)) => 2,
            Some(LiveEvent::Like(_)) => 3,
            Some(LiveEvent::Member(_)) => 4,
            Some(LiveEvent::Social(_)) => 5,
            Some(LiveEvent::RoomUser(_)) => 6,
            _ => 0,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_to_json(e: *const NativeEvent, mode: i32) -> *mut c_char {
    super::guard(ptr::null_mut(), || {
        let Some(e) = e.as_ref() else {
            fail(ResultCode::InvalidArgument, "NULL event");
            return ptr::null_mut();
        };
        let mode = match mode {
            0 => JsonMode::Normalized,
            1 => JsonMode::Schema,
            2 => JsonMode::Full,
            3 => JsonMode::Compact,
            4 => JsonMode::Raw,
            _ => {
                fail(ResultCode::InvalidArgument, "invalid JSON mode");
                return ptr::null_mut();
            }
        };
        e.envelope.to_json(mode).map_or_else(
            || {
                fail(
                    ResultCode::InvalidState,
                    "event has no normalized representation",
                );
                ptr::null_mut()
            },
            |s| cstring(&s).into_raw(),
        )
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_decode(
    payload: *const u8,
    payload_len: usize,
) -> *mut NativeBatch {
    super::guard(ptr::null_mut(), || {
        let Some(p) = bytes(payload, payload_len, 16 * 1024 * 1024) else {
            return ptr::null_mut();
        };
        match ttl_live_events::decode_batch(p) {
            Ok(b) => Box::into_raw(Box::new(NativeBatch::new(b))),
            Err(_) => {
                fail(ResultCode::Decode, "invalid batch envelope");
                ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_free(b: *mut NativeBatch) {
    super::guard((), || {
        if !b.is_null() {
            drop(Box::from_raw(b));
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_event_count(b: *const NativeBatch) -> usize {
    super::guard(0, || b.as_ref().map_or(0, |b| b.events.len()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_event_at(b: *const NativeBatch, i: usize) -> *const NativeEvent {
    super::guard(ptr::null(), || {
        b.as_ref()
            .and_then(|b| b.events.get(i))
            .map_or(ptr::null(), |e| e)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_cursor(b: *const NativeBatch) -> *const c_char {
    super::guard(ptr::null(), || {
        b.as_ref().map_or(ptr::null(), |b| b.cursor.as_ptr())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_internal_ext(b: *const NativeBatch) -> *const c_char {
    super::guard(ptr::null(), || {
        b.as_ref().map_or(ptr::null(), |b| b.internal_ext.as_ptr())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_push_server(b: *const NativeBatch) -> *const c_char {
    super::guard(ptr::null(), || {
        b.as_ref().map_or(ptr::null(), |b| b.push_server.as_ptr())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_need_ack(b: *const NativeBatch) -> bool {
    super::guard(false, || b.as_ref().is_some_and(|b| b.need_ack))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_batch_heartbeat_duration(b: *const NativeBatch) -> i64 {
    super::guard(0, || b.as_ref().map_or(0, |b| b.heartbeat))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_field_count(o: *const NativeObject) -> usize {
    super::guard(0, || o.as_ref().map_or(0, |o| o.fields.len()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_field_at(
    o: *const NativeObject,
    i: usize,
) -> *const NativeField {
    super::guard(ptr::null(), || {
        o.as_ref()
            .and_then(|o| o.fields.get(i))
            .map_or(ptr::null(), |f| f)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_find(
    o: *const NativeObject,
    name: *const c_char,
    len: usize,
) -> *const NativeField {
    super::guard(ptr::null(), || {
        let Some(n) = text(name, len, 4096) else {
            return ptr::null();
        };
        o.as_ref()
            .and_then(|o| o.fields.iter().find(|f| named(f, n)))
            .map_or(ptr::null(), |f| f)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_find_all(
    o: *const NativeObject,
    name: *const c_char,
    len: usize,
    out: *mut *const NativeField,
    capacity: usize,
) -> usize {
    super::guard(0, || {
        let Some(n) = text(name, len, 4096) else {
            return 0;
        };
        let Some(o) = o.as_ref() else {
            return 0;
        };
        let mut count = 0;
        for f in o.fields.iter().filter(|f| named(f, n)) {
            if count < capacity && !out.is_null() {
                *out.add(count) = f;
            }
            count += 1;
        }
        count
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_is_truncated(o: *const NativeObject) -> bool {
    super::guard(false, || o.as_ref().is_some_and(|o| o.truncated))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_number(f: *const NativeField) -> u32 {
    super::guard(0, || f.as_ref().map_or(0, |f| f.field.number))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_name(f: *const NativeField) -> *const c_char {
    super::guard(ptr::null(), || {
        f.as_ref()
            .and_then(|f| f.field.name)
            .map_or(ptr::null(), static_text)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_json_name(f: *const NativeField) -> *const c_char {
    super::guard(ptr::null(), || {
        f.as_ref()
            .and_then(|f| f.field.descriptor)
            .map_or(ptr::null(), |d| static_text(d.json_name))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_is_named(f: *const NativeField) -> bool {
    super::guard(false, || {
        f.as_ref().is_some_and(|f| f.field.descriptor.is_some())
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_wire_type(f: *const NativeField) -> u8 {
    super::guard(255, || f.as_ref().map_or(255, |f| f.field.wire_type))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_proto_type(f: *const NativeField) -> i32 {
    super::guard(17, || {
        proto_type(
            f.as_ref()
                .and_then(|f| f.field.descriptor)
                .map(|d| d.value_kind),
        )
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_cardinality(f: *const NativeField) -> i32 {
    super::guard(0, || {
        cardinality(f.as_ref().and_then(|f| f.field.descriptor))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_is_map(f: *const NativeField) -> bool {
    super::guard(false, || {
        f.as_ref()
            .and_then(|f| f.field.descriptor)
            .is_some_and(|d| d.is_map)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_oneof_name(f: *const NativeField) -> *const c_char {
    super::guard(ptr::null(), || {
        f.as_ref()
            .and_then(|f| f.field.descriptor)
            .and_then(|d| d.oneof_name)
            .map_or(ptr::null(), static_text)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_oneof_index(f: *const NativeField) -> i32 {
    super::guard(-1, || {
        f.as_ref()
            .and_then(|f| f.field.descriptor)
            .and_then(|d| d.oneof_index)
            .unwrap_or(-1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_enum_type(f: *const NativeField) -> *const c_char {
    super::guard(ptr::null(), || match f.as_ref().map(|f| &f.field.value) {
        Some(V::Enum { enum_type, .. }) => static_text(enum_type),
        _ => ptr::null(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_enum_name(f: *const NativeField) -> *const c_char {
    super::guard(ptr::null(), || match f.as_ref().map(|f| &f.field.value) {
        Some(V::Enum { name: Some(n), .. }) => static_text(n),
        _ => ptr::null(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_get_object(f: *const NativeField) -> *const NativeObject {
    super::guard(ptr::null(), || {
        f.as_ref()
            .and_then(|f| f.object.as_deref())
            .map_or(ptr::null(), |o| o)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_type(f: *const NativeField) -> i32 {
    super::guard(-1, || match f.as_ref().map(|f| &f.field.value) {
        Some(
            V::Varint(_)
            | V::Int64(_)
            | V::Uint64(_)
            | V::Sint64(_)
            | V::Int32(_)
            | V::Uint32(_)
            | V::Sint32(_)
            | V::Bool(_)
            | V::Enum { .. },
        ) => 0,
        Some(V::Fixed32(_) | V::Sfixed32(_) | V::Float(_)) => 1,
        Some(V::Fixed64(_) | V::Sfixed64(_) | V::Double(_)) => 2,
        Some(V::Text(_)) => 3,
        Some(V::Bytes(_)) => 4,
        Some(V::Message(_)) => 5,
        Some(V::Truncated(_)) => 6,
        None => -1,
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_get_string(
    f: *const NativeField,
    out: *mut *const c_char,
    len: *mut usize,
) -> bool {
    super::guard(false, || {
        if out.is_null() || len.is_null() {
            return false;
        }
        match f.as_ref().map(|f| &f.field.value) {
            Some(V::Text(s)) => {
                *out = s.as_ptr().cast();
                *len = s.len();
                true
            }
            _ => false,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_field_get_bytes(
    f: *const NativeField,
    out: *mut *const u8,
    len: *mut usize,
) -> bool {
    super::guard(false, || {
        if out.is_null() || len.is_null() {
            return false;
        }
        match f.as_ref().map(|f| &f.field.value) {
            Some(V::Bytes(s) | V::Truncated(s)) => {
                *out = s.as_ptr();
                *len = s.len();
                true
            }
            _ => false,
        }
    })
}

fn named(f: &NativeField, n: &str) -> bool {
    f.field
        .descriptor
        .is_some_and(|d| d.name == n || d.json_name == n)
}
macro_rules! getter {
    ($name:ident,$t:ty,$($p:pat => $v:expr),+ $(,)?) => {
        #[no_mangle]
pub unsafe extern "C" fn $name(f: *const NativeField, out: *mut $t) -> bool { super::guard(false, || {
            if out.is_null() || f.as_ref().is_some_and(|f| f.field.warning) { return false; }
            match f.as_ref().map(|f| &f.field.value) { $(Some($p) => { *out = $v; true },)+ _ => false }
        }) }

    }
}
getter!(ttl_field_get_u64,u64,V::Uint64(v) | V::Fixed64(v) | V::Varint(v) => *v);
getter!(ttl_field_get_i64,i64,V::Int64(v) | V::Sint64(v) | V::Sfixed64(v) => *v);
getter!(ttl_field_get_u32,u32,V::Uint32(v) | V::Fixed32(v) => *v);
getter!(ttl_field_get_i32,i32,V::Int32(v) | V::Sint32(v) | V::Sfixed32(v) => *v);
getter!(ttl_field_get_f32,f32,V::Float(v) => *v);
getter!(ttl_field_get_f64,f64,V::Double(v) => *v);
getter!(ttl_field_get_bool,bool,V::Bool(v) => *v);
getter!(ttl_field_get_enum_number,i32,V::Enum { number, .. } => *number);

/// Normalized convenience views use length-delimited borrowed strings.
#[repr(C)]
pub struct UserView {
    pub id: u64,
    pub nickname: *const c_char,
    pub nickname_len: usize,
    pub unique_id: *const c_char,
    pub unique_id_len: usize,
    pub sec_uid: *const c_char,
    pub sec_uid_len: usize,
    pub avatar_url: *const c_char,
    pub avatar_url_len: usize,
}
impl UserView {
    fn from(u: &ttl_live_events::EventUser) -> Self {
        Self {
            id: u.id,
            nickname: u.nickname.as_ptr().cast(),
            nickname_len: u.nickname.len(),
            unique_id: u.unique_id.as_ptr().cast(),
            unique_id_len: u.unique_id.len(),
            sec_uid: u.sec_uid.as_ptr().cast(),
            sec_uid_len: u.sec_uid.len(),
            avatar_url: u
                .avatar_url
                .as_ref()
                .map_or(ptr::null(), |s| s.as_ptr().cast()),
            avatar_url_len: u.avatar_url.as_ref().map_or(0, String::len),
        }
    }
}
#[repr(C)]
pub struct ChatView {
    pub user: UserView,
    pub comment: *const c_char,
    pub comment_len: usize,
}
#[repr(C)]
pub struct GiftView {
    pub user: UserView,
    pub gift_id: u64,
    pub gift_name: *const c_char,
    pub gift_name_len: usize,
    pub diamond_count: u64,
    pub repeat_count: u64,
    pub combo_count: u64,
    pub group_id: u64,
    pub repeat_end: bool,
    pub gift_image_url: *const c_char,
    pub gift_image_url_len: usize,
}
#[no_mangle]
pub unsafe extern "C" fn ttl_event_get_user(e: *const NativeEvent, out: *mut UserView) -> bool {
    super::guard(false, || {
        if out.is_null() {
            return false;
        }
        let Some(u) = e
            .as_ref()
            .and_then(|e| e.envelope.normalized())
            .and_then(LiveEvent::user)
        else {
            return false;
        };
        *out = UserView::from(u);
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_chat(e: *const NativeEvent, out: *mut ChatView) -> bool {
    super::guard(false, || {
        if out.is_null() {
            return false;
        }
        let Some(LiveEvent::Chat(c)) = e.as_ref().and_then(|e| e.envelope.normalized()) else {
            return false;
        };
        *out = ChatView {
            user: UserView::from(&c.user),
            comment: c.comment.as_ptr().cast(),
            comment_len: c.comment.len(),
        };
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_gift(e: *const NativeEvent, out: *mut GiftView) -> bool {
    super::guard(false, || {
        if out.is_null() {
            return false;
        }
        let Some(LiveEvent::Gift(g)) = e.as_ref().and_then(|e| e.envelope.normalized()) else {
            return false;
        };
        *out = GiftView {
            user: UserView::from(&g.user),
            gift_id: g.gift_id,
            gift_name: g.gift_name.as_ptr().cast(),
            gift_name_len: g.gift_name.len(),
            diamond_count: g.diamond_count,
            repeat_count: g.repeat_count,
            combo_count: g.combo_count,
            group_id: g.group_id,
            repeat_end: g.repeat_end,
            gift_image_url: g
                .gift_image_url
                .as_ref()
                .map_or(ptr::null(), |s| s.as_ptr().cast()),
            gift_image_url_len: g.gift_image_url.as_ref().map_or(0, String::len),
        };
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_map_size(
    o: *const NativeObject,
    name: *const c_char,
    len: usize,
) -> usize {
    super::guard(0, || {
        let Some(n) = text(name, len, 4096) else {
            return 0;
        };
        o.as_ref().map_or(0, |o| {
            o.fields
                .iter()
                .filter(|f| named(f, n) && f.field.descriptor.is_some_and(|d| d.is_map))
                .count()
        })
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_map_key_at(
    o: *const NativeObject,
    name: *const c_char,
    len: usize,
    index: usize,
) -> *const NativeField {
    super::guard(ptr::null(), || {
        let Some(n) = text(name, len, 4096) else {
            return ptr::null();
        };
        map_part(o.as_ref(), n, index, 1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_map_value_at(
    o: *const NativeObject,
    name: *const c_char,
    len: usize,
    index: usize,
) -> *const NativeField {
    super::guard(ptr::null(), || {
        let Some(n) = text(name, len, 4096) else {
            return ptr::null();
        };
        map_part(o.as_ref(), n, index, 2)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_object_oneof_active(
    o: *const NativeObject,
    name: *const c_char,
    len: usize,
) -> *const NativeField {
    super::guard(ptr::null(), || {
        let Some(n) = text(name, len, 4096) else {
            return ptr::null();
        };
        o.as_ref()
            .and_then(|o| {
                o.fields
                    .iter()
                    .rev()
                    .find(|f| f.field.descriptor.and_then(|d| d.oneof_name) == Some(n))
            })
            .map_or(ptr::null(), |f| f)
    })
}

fn map_part(o: Option<&NativeObject>, name: &str, index: usize, number: u32) -> *const NativeField {
    o.and_then(|o| {
        o.fields
            .iter()
            .filter(|f| named(f, name) && f.field.descriptor.is_some_and(|d| d.is_map))
            .nth(index)
    })
    .and_then(|f| f.object.as_deref())
    .and_then(|o| o.fields.iter().rev().find(|f| f.field.number == number))
    .map_or(ptr::null(), |f| f)
}
