# Hub-Client Custom SCSS Theme Support

## Overview

This plan addresses the issue where custom SCSS theme files (e.g., `editorial_marks.scss`) exist in an Automerge project but cannot be found by the SCSS compilation subsystem in hub-client.

## Problem Diagnosis

### Error Message
```
[compileDocumentCss] Cache miss for theme: - editorial_marks.scss
[renderToHtml] Theme CSS compilation failed, using default CSS: Error: SASS compilation failed: Custom theme file not found: /editorial_marks.scss
```

### Root Cause Analysis

There are **two interconnected issues**:

#### Issue 1: Document Path Context Lost in `compile_document_css`

The WASM function `compile_document_css(content: &str)` only receives the document **content**, not its **path**:

```rust
// crates/wasm-quarto-hub-client/src/lib.rs:1574
pub async fn compile_document_css(content: &str) -> String {
    // ...
    // Create theme context (using root as document dir since VFS is flat)
    let context = ThemeContext::new(std::path::PathBuf::from("/"), runtime);
    // ...
}
```

**Consequence**: When a document at `/docs/index.qmd` references `editorial_marks.scss`, the path resolves to `/editorial_marks.scss` instead of `/docs/editorial_marks.scss`.

#### Issue 2: Path Resolution Logic for Custom Themes

The `ThemeContext::resolve_path()` function in `quarto-sass/src/themes.rs` joins relative paths with the document directory:

```rust
pub fn resolve_path(&self, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        self.document_dir.join(path)
    }
}
```

Since the document directory is always `/` in the current WASM implementation, relative theme paths cannot be correctly resolved.

### How It Works in Native Binary

In the native `quarto` binary, the flow is:

1. Document path is known (e.g., `/project/docs/index.qmd`)
2. `ThemeContext` is created with document directory `/project/docs/`
3. Relative theme `editorial_marks.scss` resolves to `/project/docs/editorial_marks.scss`
4. `NativeRuntime` reads the file from the actual filesystem

### How It Should Work in Hub-Client

In hub-client:

1. Files are synced from Automerge to VFS via `vfsAddFile(path, content)`
2. The VFS contains all project files (including `.scss` files)
3. SCSS compilation should be able to find custom theme files using the same relative path resolution

## Solution Design

### Option A: Pass Document Path to `compile_document_css` (Recommended)

Modify the API to include the document path:

```rust
// New signature
pub async fn compile_document_css(content: &str, document_path: &str) -> String {
    // Extract document directory from path
    let doc_path = Path::new(document_path);
    let doc_dir = doc_path.parent().unwrap_or(Path::new("/"));

    // Create theme context with correct document directory
    let context = ThemeContext::new(doc_dir.to_path_buf(), runtime);
    // ...
}
```

**Hub-client changes**:
```typescript
// wasmRenderer.ts
async function compileAndInjectThemeCss(qmdContent: string, documentPath: string): Promise<string> {
    // ...
    const result = JSON.parse(await wasm.compile_document_css(content, documentPath));
    // ...
}
```

**Pros**:
- Clean solution that preserves the native behavior
- Relative imports within SCSS files will also work correctly
- Minimal changes to existing code

**Cons**:
- Requires updating the API signature
- Need to thread document path through several call sites

### Option B: Provide All Project Files Context

Alternative: Pass a list of available files so the SCSS resolver can search for matches.

**Pros**:
- More flexible for complex project structures

**Cons**:
- More complex implementation
- Doesn't solve the fundamental path resolution issue

## Implementation Plan

### Phase 1: API Changes

- [x] Update `compile_document_css` in `wasm-quarto-hub-client` to accept `document_path` parameter
- [x] Update TypeScript type definitions for WASM module
- [x] Update `compileAndInjectThemeCss` in `wasmRenderer.ts` to pass document path
- [x] Update `renderToHtml` to accept and pass document path

### Phase 2: Path Threading in Hub-Client

- [x] Track active document path in hub-client state (via `currentFile.path`)
- [x] Pass document path from Editor component through to render calls
- [x] Ensure document path is available when CSS compilation is triggered

### Phase 3: Testing

- [x] Verified existing wasmRenderer.test.ts tests pass (168 tests total)
- [x] Verified quarto-sass tests pass (135 tests total)
- [x] TypeScript compilation verified
- [x] Full workspace Rust build verified
- [ ] Integration test with real WASM module (requires wasm-pack setup)

### Phase 4: Edge Cases

- [x] Handle case where document path is not available (fall back to `/input.qmd`)
- [x] Handle absolute paths in theme specifications (handled by ThemeContext.resolve_path())
- [ ] Verify embedded resource paths (`/__quarto_resources__/`) still work (verified by existing tests)

## Files to Modify

1. **`crates/wasm-quarto-hub-client/src/lib.rs`**
   - Update `compile_document_css()` signature and implementation

2. **`hub-client/src/services/wasmRenderer.ts`**
   - Update `compileAndInjectThemeCss()` to pass document path
   - Update `renderToHtml()` API if needed

3. **`hub-client/src/components/Editor.tsx`** (or wherever render is triggered)
   - Ensure document path is available and passed to render functions

## Notes

- The Automerge sync client already syncs ALL files to VFS, including `.scss` files
- VFS paths may or may not have leading slashes depending on project creation
- Need to verify path format consistency between Automerge and VFS

## Testing Strategy

### Existing Test Infrastructure

The hub-client has a comprehensive testing infrastructure:

1. **Unit tests** (Vitest, Node environment): `src/**/*.test.ts`
2. **Integration tests** (Vitest, jsdom): `src/**/*.integration.test.ts`
3. **E2E tests** (Playwright): `e2e/**/*.spec.ts`

Key mock implementations available:
- `createMockWasmRenderer()` in `src/test-utils/mockWasm.ts`
- `createMockSyncClient()` in `src/test-utils/mockSyncClient.ts`

### Testing Levels for This Fix

#### Level 1: Rust Unit Test (Most Direct)

Location: `crates/quarto-sass/tests/custom_theme_test.rs`

The existing Rust tests already cover custom theme resolution with `ThemeContext::native()`.
To test the WASM-specific fix, we need to test that `compile_document_css` correctly extracts
document directory from the path parameter.

```rust
// In crates/wasm-quarto-hub-client - but WASM testing is complex
// Better to test the path extraction logic in quarto-sass directly
#[test]
fn test_document_path_to_directory() {
    // Test that /docs/index.qmd -> /docs/
    // Test that /index.qmd -> /
    // Test that index.qmd -> /
}
```

#### Level 2: Hub-Client Integration Test (Recommended for Regression)

Location: `hub-client/src/services/scssCompilation.integration.test.ts`

This requires the real WASM module to be built and available. The test would:

1. Initialize the real WASM module
2. Add files to VFS (document + custom SCSS)
3. Call `compileDocumentCss` with content and path
4. Verify the custom theme CSS is included in output

```typescript
// hub-client/src/services/scssCompilation.integration.test.ts
import { describe, it, expect, beforeAll } from 'vitest';
import { initWasm, vfsAddFile, vfsClear } from './wasmRenderer';

describe('SCSS compilation with custom themes', () => {
  beforeAll(async () => {
    await initWasm();
  });

  beforeEach(() => {
    vfsClear();
  });

  it('should find custom SCSS file in same directory as document', async () => {
    // Add custom SCSS to VFS
    vfsAddFile('/docs/custom-theme.scss', `
      /*-- scss:defaults --*/
      $primary: #ff6600 !default;

      /*-- scss:rules --*/
      .custom-class { color: $primary; }
    `);

    // Add document that references the theme
    const qmdContent = `---
title: Test
format:
  html:
    theme: custom-theme.scss
---

# Hello
`;

    // This is where the fix matters - we pass the document path
    const result = await compileDocumentCss(qmdContent, '/docs/index.qmd');

    expect(result.success).toBe(true);
    expect(result.css).toContain('.custom-class');
  });

  it('should handle document at project root', async () => {
    vfsAddFile('/editorial_marks.scss', `
      /*-- scss:rules --*/
      .editorial { border: 1px solid red; }
    `);

    const qmdContent = `---
theme: editorial_marks.scss
---
# Test
`;

    const result = await compileDocumentCss(qmdContent, '/index.qmd');
    expect(result.success).toBe(true);
    expect(result.css).toContain('.editorial');
  });
});
```

**Note**: This test requires the WASM module to be built. Run with:
```bash
npm run build:wasm && npm run test:integration
```

#### Level 3: E2E Test (Full Pipeline Verification)

Location: `hub-client/e2e/custom-theme.spec.ts`

This tests the complete user flow:

```typescript
// hub-client/e2e/custom-theme.spec.ts
import { test, expect } from '@playwright/test';

test.describe('Custom SCSS Theme Support', () => {
  test('should render document with custom theme', async ({ page }) => {
    // This test requires:
    // 1. A fixture project with custom SCSS theme
    // 2. The sync server running
    // 3. Loading the project in hub-client

    // Navigate to project with custom theme
    await page.goto(`/?project=${process.env.E2E_CUSTOM_THEME_PROJECT_ID}`);

    // Wait for document to load
    await page.waitForSelector('.preview-iframe');

    // Get the iframe content
    const iframe = page.frameLocator('.preview-iframe');

    // Verify custom CSS is applied
    // (This assumes the custom theme adds a specific class or style)
    const element = await iframe.locator('.custom-styled-element');
    await expect(element).toHaveCSS('color', 'rgb(255, 102, 0)'); // #ff6600
  });
});
```

**Note**: E2E tests require fixtures and sync server setup. See `e2e/helpers/`.

### Recommended Test Coverage

| Level | Test File | What it Tests | Dependencies |
|-------|-----------|---------------|--------------|
| Rust | `quarto-sass/tests/custom_theme_test.rs` | Path resolution logic | Native only |
| Integration | `services/scssCompilation.integration.test.ts` | WASM + VFS + path fix | Built WASM |
| E2E | `e2e/custom-theme.spec.ts` | Full user flow | All services |

### Mock Updates (Optional)

The current `mockWasm.ts` doesn't simulate path resolution. For unit tests that don't need
real WASM, we could update the mock to accept document path:

```typescript
// In mockWasm.ts - update compileDocumentCss signature
async compileDocumentCss(
  content: string,
  documentPath?: string,  // Add optional path parameter
  options?: { minified?: boolean },
): Promise<string> {
  if (sassError) throw sassError;
  if (!isSassAvailable) throw new Error('SASS not available');

  // For mock, we can simulate the path resolution behavior
  // by checking if referenced theme files exist in mock VFS
  return compiledCss;
}
```

### Test Fixtures

Create test fixtures for custom SCSS themes:

```
hub-client/
├── src/
│   └── test-utils/
│       └── fixtures/
│           └── custom-theme-project/
│               ├── index.qmd
│               └── custom-theme.scss
└── e2e/
    └── fixtures/
        └── custom-theme-project/
            ├── index.qmd
            └── editorial_marks.scss
```

Fixture content examples are already available in `crates/quarto-sass/test-fixtures/custom/`.
