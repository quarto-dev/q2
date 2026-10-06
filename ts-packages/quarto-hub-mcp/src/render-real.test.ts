/**
 * render against the REAL q2 binary (CAP-12's plan-spec test): a fixture
 * project with a deliberate error must come back as structured diagnostics
 * with the expected Q- code and source location.
 *
 * Gated on `target/debug/q2` existing — the same cross-language tier as
 * the launcher e2e: in CI this runs in the test-suite job (Rust legs build
 * the binary before the npm steps); a toolchain-less leg skips.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
} from './in-memory-fixture.js';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..', '..');
const q2Bin = join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'q2.exe' : 'q2');
const Q2_AVAILABLE = existsSync(q2Bin);

describe.skipIf(!Q2_AVAILABLE)('render with the real q2 binary (CAP-12)', () => {
  let saved: string | undefined;
  beforeEach(() => {
    saved = process.env['QUARTO_Q2_PATH'];
    process.env['QUARTO_Q2_PATH'] = q2Bin;
  });
  afterEach(() => {
    if (saved === undefined) delete process.env['QUARTO_Q2_PATH'];
    else process.env['QUARTO_Q2_PATH'] = saved;
  });

  it('a project with a broken include returns the expected Q- code and location', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [
        {
          path: 'bad.qmd',
          content: '---\ntitle: Bad\n---\n\n# Hi\n\n{{< include missing-file.qmd >}}\n',
        },
      ]);
      const result = await callTool(f, 'render', { project: seed.indexDocId, timeout_seconds: 120 });
      expect(result.isError).not.toBe(true);
      const r = result.structuredContent as {
        ok: boolean;
        diagnostics: Array<{ code: string | null; file?: string; line?: number; column?: number }>;
      };
      expect(r.ok).toBe(false);
      expect(r.diagnostics.length).toBeGreaterThan(0);
      const d = r.diagnostics[0]!;
      expect(d.code).toBe('Q-17-2');
      expect(d.file).toBe('bad.qmd');
      expect(d.line).toBe(7);
      expect(d.column).toBe(1);
    } finally {
      await f.close();
    }
  }, 180000);

  it('a clean single-file render reports ok', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [
        { path: 'index.qmd', content: '---\ntitle: Fine\n---\n\nHello, render.\n' },
      ]);
      const result = await callTool(f, 'render', {
        project: seed.indexDocId,
        path: 'index.qmd',
        timeout_seconds: 120,
      });
      expect(result.isError).not.toBe(true);
      const r = result.structuredContent as { ok: boolean; diagnostics: unknown[] };
      expect(r.ok).toBe(true);
      expect(r.diagnostics).toEqual([]);
    } finally {
      await f.close();
    }
  }, 180000);
});
