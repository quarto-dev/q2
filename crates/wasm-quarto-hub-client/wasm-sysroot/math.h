#pragma once

#define HUGE_VAL  __builtin_huge_val()
#define HUGE_VALF __builtin_huge_valf()
#define HUGE_VALL __builtin_huge_vall()
#define INFINITY  __builtin_inf()
#define NAN       __builtin_nan("")

double sin(double x);
double cos(double x);
double tan(double x);
double asin(double x);
double acos(double x);
double atan(double x);
double atan2(double y, double x);
double sinh(double x);
double cosh(double x);
double tanh(double x);
double exp(double x);
double log(double x);
double log2(double x);
double log10(double x);
double sqrt(double x);
double pow(double base, double exp);
double fabs(double x);
double floor(double x);
double ceil(double x);
double fmod(double x, double y);
double frexp(double x, int *exp);
double ldexp(double x, int exp);
double modf(double x, double *iptr);
double round(double x);
double trunc(double x);

float sinf(float x);
float cosf(float x);
float tanf(float x);
float sqrtf(float x);
float powf(float base, float exp);
float fabsf(float x);
float floorf(float x);
float ceilf(float x);
float fmodf(float x, float y);
float roundf(float x);
float truncf(float x);
