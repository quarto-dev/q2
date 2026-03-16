#pragma once

#include <stdint.h>
#include <stdarg.h>

#ifndef NULL
#define NULL ((void*)0)
#endif

#define EOF    (-1)
#define BUFSIZ 1024
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

// FILE is an opaque type — use void* since we have no real file system
#define FILE void

#define stdin  ((FILE*)0)
#define stdout ((FILE*)1)
#define stderr ((FILE*)2)

int fprintf(FILE *__restrict__, const char *__restrict__, ...);
int fputs(const char *__restrict, FILE *__restrict);
int fputc(int, FILE *);
FILE *fdopen(int, const char *);
int fclose(FILE *);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);
size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream);
int feof(FILE *stream);
int ferror(FILE *stream);
int fflush(FILE *stream);
int getc(FILE *stream);
FILE *fopen(const char *pathname, const char *mode);
FILE *freopen(const char *pathname, const char *mode, FILE *stream);
long ftell(FILE *stream);
int fseek(FILE *stream, long offset, int whence);
int ungetc(int c, FILE *stream);
char *tmpnam(char *s);
FILE *tmpfile(void);
char *fgets(char *s, int size, FILE *stream);

int vsnprintf(char *s, size_t n, const char *format, va_list ap);
int vsprintf(char *s, const char *format, va_list ap);
int sprintf(char *str, const char *format, ...);
int snprintf(char *str, size_t n, const char *format, ...);
int printf(const char *format, ...);
