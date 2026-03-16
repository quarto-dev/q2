#pragma once

typedef signed char int8_t;
typedef short int16_t;
typedef long int32_t;
typedef long long int64_t;

typedef unsigned char uint8_t;
typedef unsigned short uint16_t;
typedef unsigned long uint32_t;
typedef unsigned long long uint64_t;

typedef unsigned long size_t;
typedef long ssize_t;
typedef long ptrdiff_t;

typedef unsigned int uintptr_t;
typedef int intptr_t;

typedef long long intmax_t;
typedef unsigned long long uintmax_t;

#define UINT8_MAX  0xff
#define UINT16_MAX 0xffff
#define UINT32_MAX 0xffffffff
#define UINT64_MAX 0xffffffffffffffffULL

#define INT8_MIN   (-128)
#define INT8_MAX   127
#define INT16_MIN  (-32768)
#define INT16_MAX  32767
#define INT32_MIN  (-2147483647L - 1L)
#define INT32_MAX  2147483647L
#define INT64_MIN  (-9223372036854775807LL - 1LL)
#define INT64_MAX  9223372036854775807LL

#define SIZE_MAX   UINT32_MAX
#define INTMAX_MIN INT64_MIN
#define INTMAX_MAX INT64_MAX
#define UINTMAX_MAX UINT64_MAX
