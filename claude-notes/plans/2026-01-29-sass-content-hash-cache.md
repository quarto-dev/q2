# SASS Content Hash Cache Keys

**Issue:** kyoto-bpp
**Status:** Planning

## Overview

The current SASS cache uses theme filenames/specifications as cache keys (e.g., `"theme:- editorial_marks.scss:minified=true"`). This causes stale CSS when custom SCSS files are edited because the filename hasn't changed.

This plan implements a merkle-tree-inspired content hash that incorporates actual file contents into the cache key, ensuring cache invalidation when any source file changes.

## Problem Statement

Current flow:
1. Extract theme config from frontmatter → `"- editorial_marks.scss"`
2. Cache key = `SHA256("theme:- editorial_marks.scss:minified=true")`
3. Edit `editorial_marks.scss` → cache key unchanged → stale CSS returned

Desired flow:
1. Extract theme config from frontmatter
2. Resolve each theme component to actual content
3. Hash each content, sort hashes, hash the concatenation
4. Cache key = content-based merkle hash
5. Edit `editorial_marks.scss` → different content → different hash → cache miss → recompile

## Design

### Two-Stage API

Split the existing `compile_document_css` into two concerns:

1. **`compute_theme_content_hash(content: &str, document_path: &str) -> String`**
   - Parses YAML frontmatter to extract theme config
   - Resolves each theme component:
     - Built-in theme name → load from embedded resources
     - Custom SCSS path → read from VFS (using document_path for relative resolution)
   - Computes SHA-256 hash of each file's content
   - Sorts hashes lexicographically
   - Returns SHA-256 hash of concatenated sorted hashes

2. **`compile_document_css(content: &str, document_path: &str) -> String`** (existing)
   - No changes needed - already does compilation
   - TypeScript calls this only on cache miss

### Hash Computation Algorithm

```
function compute_theme_content_hash(theme_config):
    hashes = []
    for component in theme_config.components:
        if is_builtin_theme(component):
            content = load_embedded_theme(component)
        else:
            resolved_path = resolve_relative_to_document(component, document_path)
            content = vfs.read(resolved_path)
        hashes.push(SHA256(content))

    hashes.sort()  // lexicographic sort for determinism
    return SHA256(hashes.join(""))
```

### TypeScript Integration

```typescript
async function compileDocumentCss(content: string, options: Options): Promise<string> {
  const documentPath = options.documentPath ?? 'input.qmd';
  const minified = options.minified ?? true;

  // Compute content-based hash (cheap, runs every time)
  const contentHash = await wasm.compute_theme_content_hash(content, documentPath);
  const cacheKey = `theme-v2:${contentHash}:minified=${minified}`;

  // Check cache
  const cache = getSassCache();
  if (!options.skipCache) {
    const cached = await cache.get(cacheKey);
    if (cached) {
      console.log('[compileDocumentCss] Cache hit for hash:', contentHash.slice(0, 8));
      return cached;
    }
    console.log('[compileDocumentCss] Cache miss for hash:', contentHash.slice(0, 8));
  }

  // Compile (expensive, only on cache miss)
  const result = await wasm.compile_document_css(content, documentPath);

  // Cache result
  if (!options.skipCache) {
    await cache.set(cacheKey, result.css, contentHash, minified);
  }

  return result.css;
}
```

## Assumptions

1. **Built-in themes' imports are stable**: Built-in themes may have `@import`/`@use` statements, but they only reference embedded resources that never change at runtime.

2. **Custom SCSS files are self-contained**: For now, we assume custom SCSS files don't have `@import` statements (or if they do, we don't track those dependencies). This simplifies the implementation.

3. **Hash computation is cheap enough**: Computing SHA-256 hashes of SCSS files is much faster than SCSS compilation, so running it on every render is acceptable.

## Implementation Plan

### Phase 1: WASM Function

- [x] Add `compute_theme_content_hash` function to `wasm-quarto-hub-client/src/lib.rs`
- [x] Implement theme component resolution (reuse existing ThemeConfig parsing)
- [x] Implement content loading for built-in themes (from embedded resources)
- [x] Implement content loading for custom themes (from VFS via runtime)
- [x] Implement merkle hash computation (SHA-256)
- [ ] Add unit tests for hash computation

### Phase 2: TypeScript Integration

- [x] Update `compileDocumentCss` in `wasmRenderer.ts` to use new hash function
- [x] Update cache key format to use content hash (prefix changed to `theme-v2:`)
- [x] Update type definitions for new WASM function
- [x] Add logging for debugging cache behavior
- [x] Update mockWasm.ts for testing

### Phase 3: Testing

- [ ] Test built-in theme hash stability (same theme = same hash across calls)
- [ ] Test custom theme hash changes when file content changes
- [ ] Test mixed theme (built-in + custom) hash computation
- [ ] Test cache invalidation when editing custom SCSS
- [x] Verify existing tests still pass (168 tests pass)

### Phase 4: Future Considerations (out of scope)

- [ ] Handle `@import`/`@use` in custom SCSS files (recursive dependency tracking)
- [ ] Invalidate cache proactively when VFS files change (subscription model)
- [ ] Consider moving cache to Rust side for tighter integration

## Files to Modify

1. **`crates/wasm-quarto-hub-client/src/lib.rs`**
   - Add `compute_theme_content_hash` function
   - May need helper functions for content resolution

2. **`hub-client/src/services/wasmRenderer.ts`**
   - Update `compileDocumentCss` to use content hash
   - Add type for new WASM function

3. **`hub-client/src/types/wasm-quarto-hub-client.d.ts`**
   - Add type declaration for `compute_theme_content_hash`

## Error Handling

- If a custom SCSS file doesn't exist: return error (let compilation fail naturally)
- If VFS read fails: return error with path information
- If built-in theme not found: return error (shouldn't happen)

## Notes

- The cache key prefix changes from `theme:` to `theme-v2:` to avoid conflicts with old cache entries
- Old cache entries will naturally be evicted by LRU
- The hash is truncated in logs for readability but full hash used as key
