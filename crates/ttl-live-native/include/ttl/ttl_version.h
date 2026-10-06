#ifndef TTL_VERSION_H
#define TTL_VERSION_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#if defined(_WIN32) && !defined(TTL_STATIC)
# if defined(TTL_BUILD)
#  define TTL_API __declspec(dllexport)
# else
#  define TTL_API __declspec(dllimport)
# endif
#else
# define TTL_API
#endif
#ifdef __cplusplus
#define TTL_BEGIN extern "C" {
#define TTL_END }
#else
#define TTL_BEGIN
#define TTL_END
#endif
#define TTL_LIVE_ABI_VERSION 1
TTL_BEGIN
TTL_API uint32_t ttl_live_abi_version(void);
TTL_API const char *ttl_live_version(void);
TTL_END
#endif
