#ifndef TTL_ERROR_H
#define TTL_ERROR_H
#include "ttl_version.h"
TTL_BEGIN
typedef enum ttl_result {
    TTL_OK = 0, TTL_ERROR_INVALID_ARGUMENT = 1, TTL_ERROR_INVALID_UTF8 = 2,
    TTL_ERROR_INVALID_STATE = 3, TTL_ERROR_DISCOVERY = 4, TTL_ERROR_OFFLINE = 5,
    TTL_ERROR_SIGNER = 6, TTL_ERROR_AUTH = 7, TTL_ERROR_SOCKET = 8,
    TTL_ERROR_PROTOCOL = 9, TTL_ERROR_DECODE = 10, TTL_ERROR_TIMEOUT = 11,
    TTL_ERROR_SHUTDOWN = 12, TTL_ERROR_INTERNAL = 13
} ttl_result_t;
typedef struct ttl_error_info {
    ttl_result_t code;
    const char *message;
    size_t message_len;
    bool retryable;
    int64_t retry_after_ms; /* -1 if unspecified */
    int32_t http_status; /* 0 if unavailable */
} ttl_error_info_t;
/* Last failure on this calling thread, valid until its next failure. */
TTL_API const char *ttl_last_error(void);
TTL_API ttl_result_t ttl_last_error_code(void);
/* Only release strings returned by ttl_event_to_json with this function. */
TTL_API void ttl_string_free(char *value);
TTL_END
#endif
