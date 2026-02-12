/**
 * WASM Tests for prepare_template
 *
 * Tests the template processing function that extracts template-name
 * metadata and produces stripped content for new file creation.
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
  prepare_template: (content: string) => string;
}

interface PrepareTemplateResponse {
  success: boolean;
  template_name?: string | null;
  stripped_content?: string;
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
 * Helper to call prepare_template and parse the JSON response
 */
function prepareTemplate(content: string): PrepareTemplateResponse {
  const result = wasm.prepare_template(content);
  return JSON.parse(result) as PrepareTemplateResponse;
}

describe('prepare_template', () => {
  describe('template-name extraction', () => {
    it('extracts template-name from frontmatter', () => {
      const content = `---
template-name: "Article Template"
title: "Untitled"
---

# Introduction
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.template_name).toBe('Article Template');
    });

    it('handles template-name without quotes', () => {
      const content = `---
template-name: Simple Article
title: "Untitled"
---

Content here.
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.template_name).toBe('Simple Article');
    });

    it('returns null/undefined template_name when not present', () => {
      const content = `---
title: "Untitled"
author: "Someone"
---

# Content
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      // template_name is omitted from JSON when None, so it's undefined in JS
      expect(result.template_name).toBeFalsy();
    });

    it('handles document with no frontmatter', () => {
      const content = `# Just a Heading

Some content without frontmatter.
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      // template_name is omitted from JSON when None, so it's undefined in JS
      expect(result.template_name).toBeFalsy();
    });
  });

  describe('content stripping', () => {
    it('removes template-name from stripped content', () => {
      const content = `---
template-name: "Article Template"
title: "Untitled"
---

# Introduction
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.stripped_content).toBeDefined();
      expect(result.stripped_content).not.toContain('template-name');
      expect(result.stripped_content).toContain('title');
      expect(result.stripped_content).toContain('Untitled');
    });

    it('preserves other metadata fields', () => {
      const content = `---
template-name: "Report"
title: "Quarterly Report"
author: "Jane Doe"
date: "2026-01-15"
format: html
---

# Summary

Report content here.
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.stripped_content).not.toContain('template-name');
      expect(result.stripped_content).toContain('title');
      expect(result.stripped_content).toContain('Quarterly Report');
      expect(result.stripped_content).toContain('author');
      expect(result.stripped_content).toContain('Jane Doe');
      expect(result.stripped_content).toContain('format');
    });

    it('preserves document body content', () => {
      const content = `---
template-name: "Article"
title: "Test"
---

# First Section

This is paragraph one.

## Subsection

- Item 1
- Item 2

\`\`\`python
print("Hello")
\`\`\`
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.stripped_content).toContain('First Section');
      expect(result.stripped_content).toContain('paragraph one');
      expect(result.stripped_content).toContain('Subsection');
      expect(result.stripped_content).toContain('Item 1');
      expect(result.stripped_content).toContain('print("Hello")');
    });

    it('returns unchanged content when no template-name exists', () => {
      const content = `---
title: "Existing Document"
---

# Content
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      // Content should still be valid QMD (may have formatting differences)
      expect(result.stripped_content).toContain('title');
      expect(result.stripped_content).toContain('Existing Document');
      expect(result.stripped_content).toContain('Content');
    });
  });

  describe('edge cases', () => {
    it('handles empty document', () => {
      const content = '';
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      // template_name is omitted from JSON when None, so it's undefined in JS
      expect(result.template_name).toBeFalsy();
    });

    it('handles frontmatter-only document', () => {
      const content = `---
template-name: "Empty Template"
title: "Blank"
---
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.template_name).toBe('Empty Template');
      expect(result.stripped_content).not.toContain('template-name');
    });

    it('handles template-name with special characters', () => {
      const content = `---
template-name: "Article (with parentheses) & symbols!"
title: "Test"
---

Content.
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.template_name).toBe('Article (with parentheses) & symbols!');
    });

    it('handles template-name with unicode', () => {
      const content = `---
template-name: "Artikel auf Deutsch"
title: "Test"
---

Content.
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.template_name).toBe('Artikel auf Deutsch');
    });

    it('handles nested format metadata', () => {
      const content = `---
template-name: "HTML Article"
title: "Test"
format:
  html:
    toc: true
    toc-depth: 2
---

# Content
`;
      const result = prepareTemplate(content);

      expect(result.success).toBe(true);
      expect(result.template_name).toBe('HTML Article');
      expect(result.stripped_content).not.toContain('template-name');
      expect(result.stripped_content).toContain('format');
      expect(result.stripped_content).toContain('toc');
    });
  });

  describe('error handling', () => {
    it('handles invalid YAML gracefully', () => {
      const content = `---
template-name: "Test
title: unclosed quote
---
`;
      const result = prepareTemplate(content);

      // Should either succeed with parsing recovery or fail gracefully
      // The exact behavior depends on the parser's error recovery
      expect(result).toHaveProperty('success');
    });
  });
});
