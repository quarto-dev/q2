/**
 * The docs tool (CAP-13): Quarto 2 documentation search/fetch over the
 * embedded corpus, reachable without shelling out. Tests drive the tool
 * against a FAKE q2 stub (QUARTO_Q2_PATH) serving a small llms-full.txt;
 * the real-corpus end-to-end is recorded at the phase gate.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mkdtempSync, rmSync, writeFileSync, chmodSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  startInMemoryMcp,
  callTool,
} from './in-memory-fixture.js';
import { resetDocsCorpusForTests } from './docs-tool.js';

const CORPUS = [
  '---',
  'title: Authoring Quarto Documents',
  'url: guides/authoring/index.md',
  '---',
  '',
  '# Authoring Quarto Documents',
  '',
  'Write qmd files with YAML front matter. Figures get captions with fig-cap.',
  '',
  '---',
  'title: Figures',
  'url: guides/authoring/figures.md',
  '---',
  '',
  '# Figures',
  '',
  'Cross-reference a figure with @fig-plot. Set fig-cap for the caption text.',
  'Use fig-alt for accessibility text on every image.',
  '',
  '---',
  'title: Creating projects with q2 create',
  'url: guides/projects/create.md',
  '---',
  '',
  '# Creating projects',
  '',
  'Run q2 create to scaffold a project with a _quarto.yml.',
  '',
].join('\n');

const STUB = `#!/usr/bin/env node
// Fake q2 for docs tests: serves the canned corpus on --full.
if (process.env.FAKE_Q2_MODE === 'placeholder') {
  process.stderr.write('this q2 binary was built without the embedded documentation (placeholder embed); run cargo xtask build-agents-docs\\n');
  process.exit(1);
}
if (process.argv.includes('--full')) {
  process.stdout.write(${JSON.stringify(CORPUS)});
  process.exit(0);
}
process.stderr.write('unexpected argv: ' + process.argv.slice(2).join(' ') + '\\n');
process.exit(2);
`;

let stubDir: string;
let savedQ2Path: string | undefined;
let savedMode: string | undefined;

beforeEach(() => {
  stubDir = mkdtempSync(join(tmpdir(), 'fake-q2-docs-'));
  const stubPath = join(stubDir, 'fake-q2.mjs');
  writeFileSync(stubPath, STUB);
  chmodSync(stubPath, 0o755);
  savedQ2Path = process.env['QUARTO_Q2_PATH'];
  savedMode = process.env['FAKE_Q2_MODE'];
  process.env['QUARTO_Q2_PATH'] = stubPath;
  resetDocsCorpusForTests();
});

afterEach(() => {
  if (savedQ2Path === undefined) delete process.env['QUARTO_Q2_PATH'];
  else process.env['QUARTO_Q2_PATH'] = savedQ2Path;
  if (savedMode === undefined) delete process.env['FAKE_Q2_MODE'];
  else process.env['FAKE_Q2_MODE'] = savedMode;
  resetDocsCorpusForTests();
  rmSync(stubDir, { recursive: true, force: true });
});

function structured<T>(result: unknown): T {
  const sc = (result as { structuredContent?: unknown }).structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as T;
}

function errorText(result: unknown): string {
  const content = (result as { content?: Array<{ text?: string }> }).content;
  return content?.map((b) => b.text ?? '').join('\n') ?? '';
}

describe('docs (CAP-13)', () => {
  it('query returns ranked pages with snippets', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'docs', { query: 'figure caption' });
      expect(result.isError).not.toBe(true);
      const r = structured<{
        results: Array<{ href: string; title: string; snippet?: string }>;
        total_matches: number;
        truncated: boolean;
      }>(result);
      expect(r.results.length).toBeGreaterThan(0);
      expect(r.results[0]!.href).toBe('guides/authoring/figures.md');
      expect(r.results[0]!.title).toBe('Figures');
      expect(r.results[0]!.snippet).toMatch(/caption/);
    } finally {
      await f.close();
    }
  });

  it('max_results caps the list with truncation metadata', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'docs', { query: 'fig', max_results: 1 });
      expect(result.isError).not.toBe(true);
      const r = structured<{ results: unknown[]; total_matches: number; truncated: boolean }>(result);
      expect(r.results).toHaveLength(1);
      expect(r.total_matches).toBe(2);
      expect(r.truncated).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('page returns one page of markdown', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'docs', { page: 'guides/authoring/figures.md' });
      expect(result.isError).not.toBe(true);
      const r = structured<{ href: string; title: string; markdown: string }>(result);
      expect(r.href).toBe('guides/authoring/figures.md');
      expect(r.title).toBe('Figures');
      expect(r.markdown).toContain('@fig-plot');
    } finally {
      await f.close();
    }
  });

  it('page accepts the extensionless form', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'docs', { page: 'guides/authoring/figures' });
      expect(result.isError).not.toBe(true);
      const r = structured<{ href: string }>(result);
      expect(r.href).toBe('guides/authoring/figures.md');
    } finally {
      await f.close();
    }
  });

  it('a page miss names nearby pages', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'docs', { page: 'guides/authoring/figure.md' });
      expect(result.isError).toBe(true);
      expect(errorText(result)).toMatch(/figures\.md/);
    } finally {
      await f.close();
    }
  });

  it('requires exactly one of query / page', async () => {
    const f = await startInMemoryMcp();
    try {
      const neither = await callTool(f, 'docs', {});
      expect(neither.isError).toBe(true);
      const both = await callTool(f, 'docs', { query: 'x', page: 'y' });
      expect(both.isError).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('a placeholder embed is an actionable error, not a crash', async () => {
    process.env['FAKE_Q2_MODE'] = 'placeholder';
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'docs', { query: 'figures' });
      expect(result.isError).toBe(true);
      expect(errorText(result)).toMatch(/embedded documentation|placeholder/);
    } finally {
      await f.close();
    }
  });

  it('is on the read-only surface', async () => {
    const f = await startInMemoryMcp({ readOnly: true });
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.map((t) => t.name)).toContain('docs');
    } finally {
      await f.close();
    }
  });
});
