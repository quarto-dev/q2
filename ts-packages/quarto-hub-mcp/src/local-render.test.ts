/**
 * Tests for the local WASM renderer backing `get_errors` (v2).
 *
 * These run the REAL wasm-quarto-hub-client module — the same artifact
 * the browser preview loads — in Node, via the vitest aliases in
 * vitest.config.ts. No mocks: the point of this layer is that the MCP
 * generates diagnostics exactly the way QuartoHub does.
 */

import { createHash } from 'node:crypto';
import { describe, it, expect } from 'vitest';
import type { FilePayload } from '@quarto/quarto-sync-client';
import { renderDiagnostics } from './local-render.js';

const BROKEN_YAML = '---\ntitle: "broken\n---\n\n# Hello\n';
const BROKEN_STRONG = '---\ntitle: ok\n---\n\nHello **unclosed strong\n';
const CLEAN = '---\ntitle: ok\n---\n\nAll fine here.\n';
const QUARTO_YML = 'project:\n  type: default\n';

function project(files: Record<string, string | Uint8Array>): Map<string, FilePayload> {
  const m = new Map<string, FilePayload>();
  for (const [path, content] of Object.entries(files)) {
    m.set(
      path,
      typeof content === 'string'
        ? { type: 'text', text: content }
        : { type: 'binary', data: content, mimeType: 'image/png' },
    );
  }
  return m;
}

describe('renderDiagnostics — real WASM', () => {
  it('reports a structured error with line/column for an unclosed strong emphasis', async () => {
    const result = await renderDiagnostics(
      project({ '_quarto.yml': QUARTO_YML, 'index.qmd': BROKEN_STRONG }),
      'index.qmd',
    );

    expect(result.errors.length).toBeGreaterThan(0);
    const err = result.errors[0]!;
    expect(err.title).toBe('Unclosed Strong Star Emphasis');
    expect(err.start_line).toBe(5);
    expect(typeof err.start_column).toBe('number');
    // The ANSI `rendered` snippet is stripped: agents get structured
    // fields + the file content; escape codes are token noise.
    expect(Object.keys(err)).not.toContain('rendered');
  }, 60000);

  it('reports the unclosed front-matter quote the way QuartoHub does (a warning)', async () => {
    // Pinned against the real pipeline: the qmd YAML parser recovers
    // from `title: "broken` and emits a warning, not an error — the
    // agent must see exactly what the browser preview reports.
    const result = await renderDiagnostics(
      project({ '_quarto.yml': QUARTO_YML, 'index.qmd': BROKEN_YAML }),
      'index.qmd',
    );
    expect(result.errors).toEqual([]);
    expect(result.warnings.length).toBeGreaterThan(0);
  }, 60000);

  it('returns no errors for a clean document', async () => {
    const result = await renderDiagnostics(
      project({ '_quarto.yml': QUARTO_YML, 'index.qmd': CLEAN }),
      'index.qmd',
    );
    expect(result.errors).toEqual([]);
  }, 60000);

  it('attributes a sibling pass-1 failure to the sibling path', async () => {
    const result = await renderDiagnostics(
      project({
        '_quarto.yml': QUARTO_YML,
        'index.qmd': CLEAN,
        'about.qmd': BROKEN_STRONG,
      }),
      'index.qmd',
    );
    // The active page renders clean; the broken sibling surfaces as a
    // pass-1 failure keyed by its own (VFS-prefix-stripped) path.
    expect(result.errors).toEqual([]);
    const sibling = result.pass1Failures.find((f) => f.path === 'about.qmd');
    expect(sibling).toBeDefined();
    expect(sibling!.errors.length).toBeGreaterThan(0);
  }, 60000);

  it('tolerates binary files in the project', async () => {
    const result = await renderDiagnostics(
      project({
        '_quarto.yml': QUARTO_YML,
        'index.qmd': CLEAN,
        'logo.png': new Uint8Array([137, 80, 78, 71]),
      }),
      'index.qmd',
    );
    expect(result.errors).toEqual([]);
  }, 60000);

  it('reports the sha256 of exactly the content it rendered', async () => {
    const result = await renderDiagnostics(
      project({ '_quarto.yml': QUARTO_YML, 'index.qmd': CLEAN }),
      'index.qmd',
    );
    const expected = `sha256:${createHash('sha256').update(CLEAN, 'utf8').digest('hex')}`;
    expect(result.checkedContentSha256).toBe(expected);
  }, 60000);

  it('serializes concurrent renders (VFS is per-instance global state)', async () => {
    const a = renderDiagnostics(
      project({ '_quarto.yml': QUARTO_YML, 'index.qmd': BROKEN_STRONG }),
      'index.qmd',
    );
    const b = renderDiagnostics(
      project({ '_quarto.yml': QUARTO_YML, 'index.qmd': CLEAN }),
      'index.qmd',
    );
    const [ra, rb] = await Promise.all([a, b]);
    expect(ra.errors.length).toBeGreaterThan(0);
    expect(rb.errors).toEqual([]);
  }, 60000);
});
