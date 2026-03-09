/**
 * Tests for wasmRenderer utility functions.
 *
 * Note: These tests focus on pure functions that don't require WASM initialization.
 * Full WASM integration tests are in themeContentHash.wasm.test.ts.
 */

import { describe, it, expect } from 'vitest';
import { toSimpleYaml } from './wasmRenderer';

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
