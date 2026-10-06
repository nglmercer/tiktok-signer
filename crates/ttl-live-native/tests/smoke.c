#include <ttl/ttl.h>
#include <assert.h>
#include <string.h>
#include <stdio.h>
int main(void) {
    const uint8_t payload[] = {8,42,18,3,0,1,2};
    const char *method = "WebcastFutureMessage";
    ttl_event_t *event = ttl_event_decode(method,strlen(method),UINT64_MAX,true,payload,sizeof(payload));
    assert(ttl_live_abi_version() == TTL_LIVE_ABI_VERSION);
    assert(event != NULL);
    assert(ttl_event_support(event) == TTL_EVENT_SUPPORT_RAW);
    assert(ttl_event_message_id(event) == UINT64_MAX && ttl_event_is_history(event));
    ttl_event_info_t info;
    assert(ttl_event_get_info(event,&info));
    assert(info.abi_version == 1 && info.message_id == UINT64_MAX && info.is_history);
    assert(info.method_id == ttl_event_type(event) && info.raw_payload_size == sizeof(payload));
    assert(info.schema_name == NULL && !info.has_timestamp);
    const ttl_object_t *root = ttl_event_root(event);
    assert(ttl_object_field_count(root) == 2);
    const ttl_field_t *field = ttl_object_field_at(root,0);
    uint64_t number = 0;
    assert(ttl_field_get_u64(field,&number) && number == 42);
    bool flag = true;
    assert(!ttl_field_get_bool(field,&flag) && flag);
    assert(ttl_field_number(field) == 1 && ttl_field_name(field) == NULL);
    ttl_event_t *clone = ttl_event_clone(event);
    ttl_event_free(event);
    assert(memcmp(ttl_event_raw_data(clone),payload,sizeof(payload)) == 0);
    char *json = ttl_event_to_json(clone,TTL_JSON_FULL);
    assert(json != NULL && strstr(json,"18446744073709551615") != NULL);
    ttl_string_free(json);
    ttl_event_free(clone);
    assert(ttl_schema_event_count() > 6);
    for (size_t i = 0; i < ttl_schema_event_count(); ++i) {
        const ttl_event_schema_t *schema = ttl_schema_event_at(i);
        const char *name = ttl_event_schema_method(schema);
        assert(name != NULL && ttl_event_schema_full_name(schema) != NULL);
        assert(ttl_schema_event_find(name,strlen(name)) != NULL);
    }
    assert(strlen(ttl_schema_fingerprint()) == 64);
    assert(ttl_live_client_connect(NULL) == TTL_ERROR_INVALID_ARGUMENT);
    ttl_live_client_destroy(NULL);
    ttl_event_free(NULL);ttl_batch_free(NULL);ttl_string_free(NULL);
    puts("native C/C++ ABI smoke passed");
    return 0;
}
