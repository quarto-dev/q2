/**
 * Unit Tests for templateService
 *
 * Tests template discovery and processing functionality.
 * Uses mocked WASM and VFS functions.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { discoverTemplates, hasTemplates } from './templateService';

// Mock the wasmRenderer module
vi.mock('./wasmRenderer', () => ({
  vfsListFiles: vi.fn(),
  vfsReadFile: vi.fn(),
}));

// Mock the WASM module
vi.mock('wasm-quarto-hub-client', () => ({
  prepare_template: vi.fn(),
}));

import { vfsListFiles, vfsReadFile } from './wasmRenderer';
import { prepare_template } from 'wasm-quarto-hub-client';

const mockVfsListFiles = vi.mocked(vfsListFiles);
const mockVfsReadFile = vi.mocked(vfsReadFile);
const mockPrepareTemplate = vi.mocked(prepare_template);

describe('templateService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('discoverTemplates', () => {
    it('returns empty array when no templates directory exists', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: ['/project/index.qmd', '/project/chapter1.qmd'],
      });

      const templates = await discoverTemplates();

      expect(templates).toEqual([]);
    });

    it('discovers templates in _quarto-hub-templates directory', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/index.qmd',
          '/project/_quarto-hub-templates/article.qmd',
          '/project/_quarto-hub-templates/report.qmd',
        ],
      });

      mockVfsReadFile.mockImplementation((path: string) => {
        if (path === '/project/_quarto-hub-templates/article.qmd') {
          return { success: true, content: 'article content' };
        }
        if (path === '/project/_quarto-hub-templates/report.qmd') {
          return { success: true, content: 'report content' };
        }
        return { success: false, error: 'Not found' };
      });

      mockPrepareTemplate.mockImplementation((content: string) => {
        if (content === 'article content') {
          return JSON.stringify({
            success: true,
            template_name: 'Article Template',
            stripped_content: 'stripped article',
          });
        }
        if (content === 'report content') {
          return JSON.stringify({
            success: true,
            template_name: 'Report Template',
            stripped_content: 'stripped report',
          });
        }
        return JSON.stringify({ success: false, error: 'Unknown' });
      });

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(2);
      expect(templates[0].displayName).toBe('Article Template');
      expect(templates[0].strippedContent).toBe('stripped article');
      expect(templates[1].displayName).toBe('Report Template');
    });

    it('uses filename as fallback when template-name is missing', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: ['/project/_quarto-hub-templates/my-template.qmd'],
      });

      mockVfsReadFile.mockReturnValue({
        success: true,
        content: 'template without name',
      });

      mockPrepareTemplate.mockReturnValue(
        JSON.stringify({
          success: true,
          template_name: null,
          stripped_content: 'content',
        })
      );

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(1);
      expect(templates[0].displayName).toBe('my-template'); // filename without .qmd
    });

    it('ignores nested directories in templates folder', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/_quarto-hub-templates/article.qmd',
          '/project/_quarto-hub-templates/drafts/draft.qmd', // nested - should be ignored
        ],
      });

      mockVfsReadFile.mockReturnValue({
        success: true,
        content: 'content',
      });

      mockPrepareTemplate.mockReturnValue(
        JSON.stringify({
          success: true,
          template_name: 'Article',
          stripped_content: 'stripped',
        })
      );

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(1);
      expect(templates[0].path).toBe('/project/_quarto-hub-templates/article.qmd');
    });

    it('ignores non-qmd files in templates folder', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/_quarto-hub-templates/article.qmd',
          '/project/_quarto-hub-templates/readme.md',
          '/project/_quarto-hub-templates/config.yml',
        ],
      });

      mockVfsReadFile.mockReturnValue({
        success: true,
        content: 'content',
      });

      mockPrepareTemplate.mockReturnValue(
        JSON.stringify({
          success: true,
          template_name: 'Article',
          stripped_content: 'stripped',
        })
      );

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(1);
    });

    it('sorts templates alphabetically by display name', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/_quarto-hub-templates/zebra.qmd',
          '/project/_quarto-hub-templates/alpha.qmd',
          '/project/_quarto-hub-templates/middle.qmd',
        ],
      });

      mockVfsReadFile.mockReturnValue({
        success: true,
        content: 'content',
      });

      mockPrepareTemplate.mockImplementation((content: string) => {
        // Return template names that match the expected sorting
        return JSON.stringify({
          success: true,
          template_name: null, // Use filename fallback
          stripped_content: 'stripped',
        });
      });

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(3);
      expect(templates[0].displayName).toBe('alpha');
      expect(templates[1].displayName).toBe('middle');
      expect(templates[2].displayName).toBe('zebra');
    });

    it('handles VFS list failure gracefully', async () => {
      mockVfsListFiles.mockReturnValue({
        success: false,
        error: 'VFS error',
      });

      const templates = await discoverTemplates();

      expect(templates).toEqual([]);
    });

    it('skips templates that fail to read', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/_quarto-hub-templates/good.qmd',
          '/project/_quarto-hub-templates/bad.qmd',
        ],
      });

      mockVfsReadFile.mockImplementation((path: string) => {
        if (path.includes('good')) {
          return { success: true, content: 'good content' };
        }
        return { success: false, error: 'Read error' };
      });

      mockPrepareTemplate.mockReturnValue(
        JSON.stringify({
          success: true,
          template_name: 'Good Template',
          stripped_content: 'stripped',
        })
      );

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(1);
      expect(templates[0].displayName).toBe('Good Template');
    });

    it('skips templates that fail to process', async () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/_quarto-hub-templates/good.qmd',
          '/project/_quarto-hub-templates/bad.qmd',
        ],
      });

      mockVfsReadFile.mockReturnValue({
        success: true,
        content: 'content',
      });

      mockPrepareTemplate.mockImplementation((content: string) => {
        // Alternate between success and failure
        if (mockPrepareTemplate.mock.calls.length % 2 === 1) {
          return JSON.stringify({
            success: true,
            template_name: 'Good Template',
            stripped_content: 'stripped',
          });
        }
        return JSON.stringify({
          success: false,
          error: 'Parse error',
        });
      });

      const templates = await discoverTemplates();

      expect(templates).toHaveLength(1);
    });
  });

  describe('hasTemplates', () => {
    it('returns true when templates exist', () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: [
          '/project/index.qmd',
          '/project/_quarto-hub-templates/article.qmd',
        ],
      });

      expect(hasTemplates()).toBe(true);
    });

    it('returns false when no templates directory', () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: ['/project/index.qmd', '/project/chapter1.qmd'],
      });

      expect(hasTemplates()).toBe(false);
    });

    it('returns false when templates directory is empty', () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: ['/project/index.qmd'],
      });

      expect(hasTemplates()).toBe(false);
    });

    it('returns false on VFS failure', () => {
      mockVfsListFiles.mockReturnValue({
        success: false,
        error: 'VFS error',
      });

      expect(hasTemplates()).toBe(false);
    });

    it('ignores nested files in templates directory', () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: ['/project/_quarto-hub-templates/nested/file.qmd'],
      });

      expect(hasTemplates()).toBe(false);
    });

    it('ignores non-qmd files', () => {
      mockVfsListFiles.mockReturnValue({
        success: true,
        files: ['/project/_quarto-hub-templates/readme.md'],
      });

      expect(hasTemplates()).toBe(false);
    });
  });
});
