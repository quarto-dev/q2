#pragma once
#include <stdint.h>

#define TS_LITTLE_ENDIAN 1

static inline uint16_t le16toh(uint16_t x) { return x; }
static inline uint16_t be16toh(uint16_t x)
{
#if defined(__GNUC__) || defined(__clang__)
    return __builtin_bswap16(x);
#else
    return (x << 8) | (x >> 8);
#endif
}