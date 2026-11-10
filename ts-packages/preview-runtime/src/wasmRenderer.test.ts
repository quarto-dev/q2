/**
 * Tests for wasmRenderer utility functions and raw-call forwarding.
 */

import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  _setWasmModuleForTest,
  renderPageForPreview,
  renderPageInProjectWithAttribution,
  toSimpleYaml,
} from './wasmRenderer';
import { setPerfOverrideForTests } from './perfCounters';

describe('toSimpleYaml', () => {
  it('serializes flat key-value pairs', () => {
    expect(toSimpleYaml({ author: 'Test' })).toBe('author: Test');
  });

  it('serializes nested objects with indentation', () => {
    const obj = { format: { html: { 'source-location': 'full' } } };
    const expected = 'format:\n  html:\n    source-location: full';
    expect(toSimpleYaml(obj)).toBe(expected);
  });

  it('serializes booleans and numbers', () => {
    expect(toSimpleYaml({ toc: true, 'toc-depth': 3 })).toBe('toc: true\ntoc-depth: 3');
  });

  it('returns empty string for empty object', () => {
    expect(toSimpleYaml({})).toBe('');
  });

  it('handles mixed flat and nested keys', () => {
    const obj = { author: 'Me', format: { html: { theme: 'cosmo' } } };
    const result = toSimpleYaml(obj);
    expect(result).toContain('author: Me');
    expect(result).toContain('format:');
    expect(result).toContain('  html:');
    expect(result).toContain('    theme: cosmo');
  });

  it('produces YAML that setScrollSyncEnabled would generate', () => {
    // This mirrors the exact object setScrollSyncEnabled(true) builds
    const settings = { format: { html: { 'source-location': 'full' } } };
    const yaml = toSimpleYaml(settings) + '\n';
    expect(yaml).toBe('format:\n  html:\n    source-location: full\n');
  });
});

describe('capture option forwarding (Phase 3B1)', () => {
  afterEach(() => {
    _setWasmModuleForTest(null);
    setPerfOverrideForTests(null);
  });

  it.each([false, true])('reaches both raw WASM entries with perf=%s', async (perf) => {
    setPerfOverrideForTests(perf);
    const response = JSON.stringify({ success: true, ast_json: '{}' });
    const renderPreview = vi.fn(async () => response);
    const renderProject = vi.fn(async () => response);
    _setWasmModuleForTest({
      render_page_for_preview: renderPreview,
      render_page_in_project_with_attribution: renderProject,
      // `renderPageForPreview` reads the source it renders in its queue slot.
      vfs_read_file: () => JSON.stringify({ success: true, content: '' }),
      vfs_take_read_set: () => 'null',
    });
    const options = { captureUntransformedAst: false };

    await renderPageForPreview('doc.qmd', undefined, undefined, options);
    expect(renderPreview).toHaveBeenCalledWith('doc.qmd', undefined, undefined, options);

    await renderPageInProjectWithAttribution('doc.qmd', undefined, null, undefined, true, options);
    expect(renderProject).toHaveBeenCalledWith(
      'doc.qmd',
      undefined,
      undefined,
      undefined,
      true,
      options,
    );
  });
});
