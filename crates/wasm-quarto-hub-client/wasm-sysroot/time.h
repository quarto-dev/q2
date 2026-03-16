#pragma once

#include <stdint.h>

typedef unsigned long clock_t;
typedef long time_t;

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
};

#define CLOCKS_PER_SEC ((clock_t)1000000)

clock_t clock(void);
time_t time(time_t *tloc);
