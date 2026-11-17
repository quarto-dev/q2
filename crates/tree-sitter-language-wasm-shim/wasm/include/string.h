#ifndef TREE_SITTER_WASM_STRING_H_
#define TREE_SITTER_WASM_STRING_H_

#include <stdint.h>

int memcmp(const void *lhs, const void *rhs, size_t count);

void *memcpy(void *restrict dst, const void *restrict src, size_t size);

void *memmove(void *dst, const void *src, size_t count);

void *memset(void *dst, int value, size_t count);

// Added from upstream tree-sitter-language 0.1.9 (tree-sitter-containerfile
// calls it). The symbol is provided at link time by wasm-c-shim.
void *memchr(const void *src, int c, size_t n);

int strncmp(const char *left, const char *right, size_t n);

size_t strlen(const char *str);

#endif // TREE_SITTER_WASM_STRING_H_
