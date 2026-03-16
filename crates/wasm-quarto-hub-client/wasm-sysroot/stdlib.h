#pragma once

#include <stdint.h>

#ifndef NULL
#define NULL ((void*)0)
#endif

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1

void* malloc(size_t size);
void* calloc(size_t nmemb, size_t size);
void free(void* ptr);
void* realloc(void* ptr, size_t size);

void abort(void);
void exit(int status);

int abs(int j);
long labs(long j);

double strtod(const char *nptr, char **endptr);
long strtol(const char *nptr, char **endptr, int base);
unsigned long strtoul(const char *nptr, char **endptr, int base);
long long strtoll(const char *nptr, char **endptr, int base);
unsigned long long strtoull(const char *nptr, char **endptr, int base);
int atoi(const char *nptr);

void qsort(void *base, size_t nmemb, size_t size,
           int (*compar)(const void *, const void *));

int rand(void);
void srand(unsigned int seed);
#define RAND_MAX 2147483647
