/**
 * WASM End-to-End Tests for compute_theme_content_hash
 *
 * These tests exercise the actual WASM module to verify content-based
 * cache key computation for SASS themes.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

// Type for the WASM module
interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  compute_theme_content_hash: (content: string, documentPath: string) => string;
  vfs_add_file: (path: string, content: string) => string;
  vfs_remove_file: (path: string) => string;
  vfs_clear: () => string;
}

interface ThemeHashResponse {
  success: boolean;
  hash?: string;
  error?: string;
}

interface VfsResponse {
  success: boolean;
  error?: string;
}

let wasm: WasmModule;

beforeAll(async () => {
  // Get the directory of this test file
  const __dirname = dirname(fileURLToPath(import.meta.url));

  // Load WASM bytes from the package directory
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);

  // Import the WASM module
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;

  // Initialize with the bytes (not a URL/fetch)
  await wasm.default(wasmBytes);
});

/**
 * Helper to parse the JSON response from compute_theme_content_hash
 */
function computeHash(content: string, documentPath: string = 'input.qmd'): ThemeHashResponse {
  const result = wasm.compute_theme_content_hash(content, documentPath);
  return JSON.parse(result) as ThemeHashResponse;
}

/**
 * Helper to add a file to VFS
 */
function vfsAdd(path: string, content: string): VfsResponse {
  const result = wasm.vfs_add_file(path, content);
  return JSON.parse(result) as VfsResponse;
}

/**
 * Helper to clear VFS
 */
function vfsClear(): VfsResponse {
  const result = wasm.vfs_clear();
  return JSON.parse(result) as VfsResponse;
}

describe('compute_theme_content_hash', () => {
  describe('built-in theme hash stability', () => {
    it('same built-in theme produces identical hash across multiple calls', () => {
      const doc = `---
format:
  html:
    theme: cosmo
---

# Hello
`;
      const result1 = computeHash(doc);
      const result2 = computeHash(doc);
      const result3 = computeHash(doc);

      expect(result1.success).toBe(true);
      expect(result2.success).toBe(true);
      expect(result3.success).toBe(true);
      expect(result1.hash).toBe(result2.hash);
      expect(result2.hash).toBe(result3.hash);
    });

    it('different built-in themes produce different hashes', () => {
      const cosmoDoc = `---
format:
  html:
    theme: cosmo
---
# Hello
`;
      const darklyDoc = `---
format:
  html:
    theme: darkly
---
# Hello
`;
      const flatlyDoc = `---
format:
  html:
    theme: flatly
---
# Hello
`;

      const cosmoResult = computeHash(cosmoDoc);
      const darklyResult = computeHash(darklyDoc);
      const flatlyResult = computeHash(flatlyDoc);

      expect(cosmoResult.success).toBe(true);
      expect(darklyResult.success).toBe(true);
      expect(flatlyResult.success).toBe(true);

      // All should be different
      expect(cosmoResult.hash).not.toBe(darklyResult.hash);
      expect(cosmoResult.hash).not.toBe(flatlyResult.hash);
      expect(darklyResult.hash).not.toBe(flatlyResult.hash);
    });
  });

  describe('custom theme from VFS', () => {
    beforeAll(() => {
      vfsClear();
    });

    it('custom theme hash is computed from VFS content', () => {
      // Add a custom SCSS file to VFS
      const scssContent = `
// Custom theme styles
$primary: #ff6600;
.custom-class {
  color: $primary;
}
`;
      const addResult = vfsAdd('/custom.scss', scssContent);
      expect(addResult.success).toBe(true);

      const doc = `---
format:
  html:
    theme: custom.scss
---
# Hello
`;
      const result = computeHash(doc, '/input.qmd');

      expect(result.success).toBe(true);
      expect(result.hash).toBeDefined();
      expect(result.hash!.length).toBe(64); // SHA-256 hex = 64 chars
    });

    it('custom theme hash changes when content changes', () => {
      // First version
      vfsAdd('/changeable.scss', '$color: red;');
      const doc = `---
format:
  html:
    theme: changeable.scss
---
# Hello
`;
      const result1 = computeHash(doc, '/input.qmd');
      expect(result1.success).toBe(true);

      // Modify the file
      vfsAdd('/changeable.scss', '$color: blue;');
      const result2 = computeHash(doc, '/input.qmd');
      expect(result2.success).toBe(true);

      // Hashes should be different
      expect(result1.hash).not.toBe(result2.hash);
    });
  });

  describe('mixed theme (built-in + custom)', () => {
    beforeAll(() => {
      vfsClear();
    });

    it('mixed theme array produces valid hash', () => {
      vfsAdd('/custom-additions.scss', '.my-class { padding: 10px; }');

      const doc = `---
format:
  html:
    theme:
      - cosmo
      - custom-additions.scss
---
# Hello
`;
      const result = computeHash(doc, '/input.qmd');

      expect(result.success).toBe(true);
      expect(result.hash).toBeDefined();
      expect(result.hash!.length).toBe(64);
    });

    it('order of themes affects hash (not sorted by name, sorted by content hash)', () => {
      vfsAdd('/a.scss', '/* A */');
      vfsAdd('/b.scss', '/* B */');

      // Note: The implementation sorts by content hash, not by position,
      // so these should produce the SAME hash
      const doc1 = `---
format:
  html:
    theme:
      - a.scss
      - b.scss
---
# Hello
`;
      const doc2 = `---
format:
  html:
    theme:
      - b.scss
      - a.scss
---
# Hello
`;
      const result1 = computeHash(doc1, '/input.qmd');
      const result2 = computeHash(doc2, '/input.qmd');

      expect(result1.success).toBe(true);
      expect(result2.success).toBe(true);
      // Same components = same hash (order independent due to sorting)
      expect(result1.hash).toBe(result2.hash);
    });
  });

  describe('document path affects resolution', () => {
    beforeAll(() => {
      vfsClear();
    });

    it('relative path resolved from document directory', () => {
      // Add file in a subdirectory
      vfsAdd('/docs/styles/theme.scss', '/* doc theme */');

      const doc = `---
format:
  html:
    theme: styles/theme.scss
---
# Hello
`;
      // Document is in /docs/, so styles/theme.scss resolves to /docs/styles/theme.scss
      const result = computeHash(doc, '/docs/index.qmd');

      expect(result.success).toBe(true);
      expect(result.hash).toBeDefined();
    });

    it('different document paths with same relative theme resolve differently', () => {
      vfsAdd('/project-a/custom.scss', '/* Project A */');
      vfsAdd('/project-b/custom.scss', '/* Project B */');

      const doc = `---
format:
  html:
    theme: custom.scss
---
# Hello
`;
      const resultA = computeHash(doc, '/project-a/index.qmd');
      const resultB = computeHash(doc, '/project-b/index.qmd');

      expect(resultA.success).toBe(true);
      expect(resultB.success).toBe(true);
      // Different content = different hash
      expect(resultA.hash).not.toBe(resultB.hash);
    });
  });

  describe('no theme config', () => {
    it('returns consistent default hash for documents without theme', () => {
      const doc1 = `---
title: No Theme Doc
---
# Hello
`;
      const doc2 = `---
title: Another No Theme Doc
author: Test
---
# World
`;
      const result1 = computeHash(doc1);
      const result2 = computeHash(doc2);

      expect(result1.success).toBe(true);
      expect(result2.success).toBe(true);
      // Both should get the same "default bootstrap" hash
      expect(result1.hash).toBe(result2.hash);
    });

    it('document without frontmatter returns default hash', () => {
      const doc = '# Just a heading\n\nSome content.';
      const result = computeHash(doc);

      expect(result.success).toBe(true);
      expect(result.hash).toBeDefined();
    });
  });

  describe('error handling', () => {
    beforeAll(() => {
      vfsClear();
    });

    it('missing custom SCSS file returns error', () => {
      const doc = `---
format:
  html:
    theme: nonexistent.scss
---
# Hello
`;
      const result = computeHash(doc, '/input.qmd');

      expect(result.success).toBe(false);
      expect(result.error).toBeDefined();
      expect(result.error).toContain('nonexistent.scss');
    });
  });
});
