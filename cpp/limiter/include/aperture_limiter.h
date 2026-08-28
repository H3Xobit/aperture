#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ApertureLimiter ApertureLimiter;

/* capacity: burst size. refill_per_sec: tokens added each second. */
ApertureLimiter* aperture_limiter_create(uint64_t capacity, double refill_per_sec);
void aperture_limiter_destroy(ApertureLimiter* limiter);

/* Returns 1 if allowed, 0 if limited, -1 on error. */
int aperture_limiter_allow(ApertureLimiter* limiter, const char* tenant_id, uint32_t cost);

/* Remaining tokens for tenant, or -1 on error. */
double aperture_limiter_remaining(ApertureLimiter* limiter, const char* tenant_id);

#ifdef __cplusplus
}
#endif
