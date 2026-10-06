#ifndef TTL_SCHEMA_H
#define TTL_SCHEMA_H
#include "ttl_event.h"
TTL_BEGIN
/* All catalog handles/names remain valid for the library lifetime. Enumerates
 * EVERY descriptor whose short name starts Webcast, including duplicate short
 * names in different packages. find uses the Rust canonical method resolver.
 * This catalog and its field metadata are generated from the pinned registry.
 */
typedef struct ttl_event_schema ttl_event_schema_t;
typedef struct ttl_schema_field ttl_schema_field_t;
TTL_API size_t ttl_schema_event_count(void);
TTL_API const ttl_event_schema_t *ttl_schema_event_at(size_t index);
TTL_API const ttl_event_schema_t *ttl_schema_event_find(const char *method, size_t method_len);
TTL_API const char *ttl_schema_fingerprint(void); /* SHA-256 of descriptor set */
TTL_API const char *ttl_event_schema_method(const ttl_event_schema_t *schema);
TTL_API const char *ttl_event_schema_full_name(const ttl_event_schema_t *schema);
TTL_API ttl_event_type_t ttl_event_schema_type(const ttl_event_schema_t *schema);
TTL_API size_t ttl_event_schema_field_count(const ttl_event_schema_t *schema);
TTL_API const ttl_schema_field_t *ttl_event_schema_field_at(const ttl_event_schema_t *schema, size_t index);
TTL_API uint32_t ttl_schema_field_number(const ttl_schema_field_t *field);
TTL_API const char *ttl_schema_field_name(const ttl_schema_field_t *field);
TTL_API const char *ttl_schema_field_json_name(const ttl_schema_field_t *field);
TTL_API ttl_proto_type_t ttl_schema_field_proto_type(const ttl_schema_field_t *field);
TTL_API uint8_t ttl_schema_field_wire_type(const ttl_schema_field_t *field);
TTL_API ttl_field_cardinality_t ttl_schema_field_cardinality(const ttl_schema_field_t *field);
TTL_API bool ttl_schema_field_is_map(const ttl_schema_field_t *field);
TTL_API bool ttl_schema_field_is_deprecated(const ttl_schema_field_t *field);
TTL_API const char *ttl_schema_field_oneof_name(const ttl_schema_field_t *field);
TTL_API int32_t ttl_schema_field_oneof_index(const ttl_schema_field_t *field);
TTL_API const char *ttl_schema_field_message_name(const ttl_schema_field_t *field);
TTL_API const char *ttl_schema_field_enum_type(const ttl_schema_field_t *field);
TTL_END
#endif
