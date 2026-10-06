#ifndef TTL_EVENT_H
#define TTL_EVENT_H
#include "ttl_version.h"
TTL_BEGIN
/* Opaque immutable handles. All child views borrow their owning event.
 * Callback events/batches last until that callback returns. Clone an event to
 * retain it. Never free a borrowed callback/batch event. NULL handles are safe.
 * Non-NULL handles must be live library handles, not arbitrary addresses.
 * Names/methods are NUL-terminated; get_string returns a length-delimited view,
 * which can contain embedded NULs and is NOT necessarily NUL-terminated.
 * Events/clones may be read concurrently, but must not be freed while in use.
 */
typedef struct ttl_event ttl_event_t;
typedef struct ttl_object ttl_object_t;
typedef struct ttl_field ttl_field_t;
typedef struct ttl_batch ttl_batch_t;
typedef uint32_t ttl_event_type_t;
typedef enum ttl_event_support {
    TTL_EVENT_SUPPORT_NORMALIZED = 1, TTL_EVENT_SUPPORT_SCHEMA = 2, TTL_EVENT_SUPPORT_RAW = 3
} ttl_event_support_t;
typedef enum ttl_value_type {
    TTL_VALUE_VARINT = 0, TTL_VALUE_FIXED32 = 1, TTL_VALUE_FIXED64 = 2,
    TTL_VALUE_STRING = 3, TTL_VALUE_BYTES = 4, TTL_VALUE_MESSAGE = 5, TTL_VALUE_TRUNCATED = 6
} ttl_value_type_t;
typedef enum ttl_proto_type {
    TTL_PROTO_DOUBLE = 0, TTL_PROTO_FLOAT = 1, TTL_PROTO_INT64 = 2, TTL_PROTO_UINT64 = 3,
    TTL_PROTO_SINT64 = 4, TTL_PROTO_FIXED64 = 5, TTL_PROTO_SFIXED64 = 6,
    TTL_PROTO_INT32 = 7, TTL_PROTO_UINT32 = 8, TTL_PROTO_SINT32 = 9,
    TTL_PROTO_FIXED32 = 10, TTL_PROTO_SFIXED32 = 11, TTL_PROTO_BOOL = 12,
    TTL_PROTO_STRING = 13, TTL_PROTO_BYTES = 14, TTL_PROTO_ENUM = 15,
    TTL_PROTO_MESSAGE = 16, TTL_PROTO_UNKNOWN = 17
} ttl_proto_type_t;
typedef enum ttl_field_cardinality {
    TTL_CARDINALITY_OPTIONAL = 0, TTL_CARDINALITY_REPEATED = 1
} ttl_field_cardinality_t;
typedef enum ttl_normalized_type {
    TTL_NORMALIZED_NONE = 0, TTL_NORMALIZED_CHAT = 1, TTL_NORMALIZED_GIFT = 2,
    TTL_NORMALIZED_LIKE = 3, TTL_NORMALIZED_MEMBER = 4, TTL_NORMALIZED_SOCIAL = 5,
    TTL_NORMALIZED_ROOM_USER = 6
} ttl_normalized_type_t;
typedef enum ttl_json_mode {
    TTL_JSON_NORMALIZED = 0, TTL_JSON_SCHEMA = 1, TTL_JSON_FULL = 2,
    TTL_JSON_COMPACT = 3, TTL_JSON_RAW = 4
} ttl_json_mode_t;
#define TTL_EVENT_FLAG_TRUNCATED UINT32_C(1)
#define TTL_EVENT_FLAG_UNKNOWN_SCHEMA UINT32_C(2)
#define TTL_EVENT_FLAG_UNKNOWN_FIELDS UINT32_C(4)
#define TTL_EVENT_FLAG_DECODE_WARNING UINT32_C(8)
typedef struct ttl_event_info {
    uint32_t abi_version;
    const char *method;
    size_t method_len;
    uint64_t message_id;
    ttl_event_support_t support;
    bool is_history;
    const char *schema_name;
    size_t schema_name_len;
    size_t raw_payload_size;
    ttl_event_type_t method_id;
    uint64_t timestamp;
    bool has_timestamp;
} ttl_event_info_t;
/* Pointer+length inputs: NULL is allowed only when length is zero. Decode owns
 * its input copy. Events >4 MiB retain a prefix with TRUNCATED; batch >16 MiB is
 * rejected (also limited to 8192 events). Fields/depth are bounded at 4096/8
 * per event and 65536 fields per batch; later events retain raw bytes and
 * truncation metadata when the batch field budget is exhausted. Invalid event wire data yields
 * a partial event with warnings, not a NULL result. Methods must be UTF-8,
 * nonempty, <=4096 bytes, and contain no NUL.
 */
TTL_API ttl_event_t *ttl_event_decode(const char *method, size_t method_len, uint64_t message_id, bool is_history, const uint8_t *payload, size_t payload_len);
TTL_API ttl_event_t *ttl_event_clone(const ttl_event_t *event);
TTL_API void ttl_event_free(ttl_event_t *event);
TTL_API const char *ttl_event_method(const ttl_event_t *event);
TTL_API size_t ttl_event_method_len(const ttl_event_t *event);
TTL_API ttl_event_type_t ttl_event_type(const ttl_event_t *event); /* FNV-1a; collisions possible */
TTL_API uint64_t ttl_event_message_id(const ttl_event_t *event);
TTL_API bool ttl_event_is_history(const ttl_event_t *event);
TTL_API ttl_event_support_t ttl_event_support(const ttl_event_t *event);
TTL_API ttl_normalized_type_t ttl_event_normalized_type(const ttl_event_t *event);
TTL_API const char *ttl_event_schema_name(const ttl_event_t *event); /* NULL if unknown */
TTL_API size_t ttl_event_raw_size(const ttl_event_t *event);
TTL_API const uint8_t *ttl_event_raw_data(const ttl_event_t *event);
TTL_API const ttl_object_t *ttl_event_root(const ttl_event_t *event);
TTL_API bool ttl_event_is_truncated(const ttl_event_t *event);
TTL_API uint32_t ttl_event_flags(const ttl_event_t *event);
TTL_API bool ttl_event_timestamp(const ttl_event_t *event, uint64_t *out); /* common.create_time, server units */
TTL_API bool ttl_event_get_info(const ttl_event_t *event, ttl_event_info_t *out);
/* Owned NUL-terminated JSON. NORMALIZED returns NULL and INVALID_STATE when
 * absent. 64-bit integers are decimal strings; bytes are {"$bytes":"base64"}.
 * FULL includes ordered wireFields with duplicates/unknowns; RAW also embeds
 * the retained protobuf payload. COMPACT is explicitly lossy for unknowns.
 */
TTL_API char *ttl_event_to_json(const ttl_event_t *event, ttl_json_mode_t mode);
TTL_API ttl_batch_t *ttl_batch_decode(const uint8_t *payload, size_t payload_len);
TTL_API void ttl_batch_free(ttl_batch_t *batch);
TTL_API size_t ttl_batch_event_count(const ttl_batch_t *batch);
TTL_API const ttl_event_t *ttl_batch_event_at(const ttl_batch_t *batch, size_t index);
TTL_API const char *ttl_batch_cursor(const ttl_batch_t *batch);
TTL_API const char *ttl_batch_internal_ext(const ttl_batch_t *batch);
TTL_API const char *ttl_batch_push_server(const ttl_batch_t *batch);
TTL_API bool ttl_batch_need_ack(const ttl_batch_t *batch);
TTL_API int64_t ttl_batch_heartbeat_duration(const ttl_batch_t *batch);
/* Canonical iteration is wire order. Packed repeated values expand to adjacent
 * occurrences with repeated cardinality and wire type 2. Maps retain their
 * repeated entry objects, with is_map metadata. Lookup matches proto/JSON names
 * exactly and returns the first occurrence. find_all returns total matches,
 * writes min(total,capacity) pointers; NULL out is a count-only query.
 */
TTL_API size_t ttl_object_field_count(const ttl_object_t *object);
TTL_API const ttl_field_t *ttl_object_field_at(const ttl_object_t *object, size_t index);
TTL_API const ttl_field_t *ttl_object_find(const ttl_object_t *object, const char *name, size_t name_len);
TTL_API size_t ttl_object_find_all(const ttl_object_t *object, const char *name, size_t name_len, const ttl_field_t **out, size_t capacity);
TTL_API bool ttl_object_is_truncated(const ttl_object_t *object);
TTL_API uint32_t ttl_field_number(const ttl_field_t *field);
TTL_API const char *ttl_field_name(const ttl_field_t *field);
TTL_API const char *ttl_field_json_name(const ttl_field_t *field);
TTL_API ttl_value_type_t ttl_field_type(const ttl_field_t *field);
TTL_API uint8_t ttl_field_wire_type(const ttl_field_t *field);
TTL_API ttl_proto_type_t ttl_field_proto_type(const ttl_field_t *field);
TTL_API bool ttl_field_is_named(const ttl_field_t *field);
TTL_API ttl_field_cardinality_t ttl_field_cardinality(const ttl_field_t *field);
TTL_API bool ttl_field_is_map(const ttl_field_t *field);
TTL_API const char *ttl_field_oneof_name(const ttl_field_t *field);
TTL_API int32_t ttl_field_oneof_index(const ttl_field_t *field); /* -1 if absent */
/* Strict logical getters. No signed/unsigned/float/integer coercion. Unknown
 * varints can be read as u64; unknown fixed32/fixed64 as u32/u64. Wrong-type or
 * NULL output returns false and leaves outputs unchanged. */
TTL_API bool ttl_field_get_u64(const ttl_field_t *field, uint64_t *out);
TTL_API bool ttl_field_get_u32(const ttl_field_t *field, uint32_t *out);
TTL_API bool ttl_field_get_i64(const ttl_field_t *field, int64_t *out);
TTL_API bool ttl_field_get_i32(const ttl_field_t *field, int32_t *out);
TTL_API bool ttl_field_get_f64(const ttl_field_t *field, double *out);
TTL_API bool ttl_field_get_f32(const ttl_field_t *field, float *out);
TTL_API bool ttl_field_get_bool(const ttl_field_t *field, bool *out);
TTL_API bool ttl_field_get_string(const ttl_field_t *field, const char **out, size_t *length);
TTL_API bool ttl_field_get_bytes(const ttl_field_t *field, const uint8_t **out, size_t *length);
TTL_API const ttl_object_t *ttl_field_get_object(const ttl_field_t *field);
TTL_API bool ttl_field_get_enum_number(const ttl_field_t *field, int32_t *out);
TTL_API const char *ttl_field_enum_type(const ttl_field_t *field);
TTL_API const char *ttl_field_enum_name(const ttl_field_t *field); /* NULL for unknown enum value */
/* Normalized convenience views. Every string is length-delimited, borrowed
 * from the event, and can contain NUL; no additional allocation occurs. */
typedef struct ttl_user {
    uint64_t id;
    const char *nickname; size_t nickname_len;
    const char *unique_id; size_t unique_id_len;
    const char *sec_uid; size_t sec_uid_len;
    const char *avatar_url; size_t avatar_url_len;
} ttl_user_t;
typedef struct ttl_chat_event {
    ttl_user_t user;
    const char *comment; size_t comment_len;
} ttl_chat_event_t;
typedef struct ttl_gift_event {
    ttl_user_t user;
    uint64_t gift_id;
    const char *gift_name; size_t gift_name_len;
    uint64_t diamond_count, repeat_count, combo_count, group_id;
    bool repeat_end;
    const char *gift_image_url; size_t gift_image_url_len;
} ttl_gift_event_t;
TTL_API bool ttl_event_get_user(const ttl_event_t *event, ttl_user_t *out);
TTL_API bool ttl_event_chat(const ttl_event_t *event, ttl_chat_event_t *out);
TTL_API bool ttl_event_gift(const ttl_event_t *event, ttl_gift_event_t *out);
/* Map helpers enumerate entries in wire order. Absent default key/value fields
 * return NULL; schema JSON supplies protobuf scalar defaults for those entries.
 * Duplicate keys remain distinct here; schema JSON uses protobuf last-key wins.
 */
TTL_API size_t ttl_object_map_size(const ttl_object_t *object, const char *name, size_t name_len);
TTL_API const ttl_field_t *ttl_object_map_key_at(const ttl_object_t *object, const char *name, size_t name_len, size_t index);
TTL_API const ttl_field_t *ttl_object_map_value_at(const ttl_object_t *object, const char *name, size_t name_len, size_t index);
TTL_API const ttl_field_t *ttl_object_oneof_active(const ttl_object_t *object, const char *name, size_t name_len);
TTL_END
#endif
