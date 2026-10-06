/**
 * get_outline (CAP-11): structural reads of qmd files. The agent gets the
 * heading tree with line ranges instead of having to read and eyeball the
 * whole file; the line ranges feed read_file's `offset`/`limit` and the
 * `section` selectors.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
} from './in-memory-fixture.js';
import { PARSER_UNAVAILABLE } from './test-setup.js';

const QMD = [
  '---',
  'title: Sections',
  '---',
  '',
  '# Introduction',
  '',
  'Intro paragraph.',
  '',
  '## Background',
  '',
  'Background text.',
  '',
  '# Methods',
  '',
  'Method text.',
  '',
  '### Details',
  '',
  'Detail text.',
  '',
  '# Results',
  '',
  'Result text.',
  '',
].join('\n');

interface OutlineResult {
  path: string;
  hash: string;
  outline: Array<{
    level: number;
    title: string;
    id?: string;
    line: number;
    end_line: number;
  }>;
}

function structuredOf(result: { structuredContent?: unknown; content?: unknown }): OutlineResult {
  const sc = (result as { structuredContent?: unknown }).structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as OutlineResult;
}

describe.skipIf(PARSER_UNAVAILABLE)('get_outline (CAP-11)', () => {
  it('returns the heading tree of a qmd file with line ranges', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'paper.qmd', content: QMD }]);
      const result = await callTool(f, 'get_outline', {
        project: seed.indexDocId,
        path: 'paper.qmd',
      });
      expect(result.isError).not.toBe(true);
      const outline = structuredOf(result);
      expect(outline.path).toBe('paper.qmd');
      expect(outline.hash).toMatch(/^sha256:/);
      expect(outline.outline).toEqual([
        { level: 1, title: 'Introduction', id: 'introduction', line: 5, end_line: 12 },
        { level: 2, title: 'Background', id: 'background', line: 9, end_line: 12 },
        { level: 1, title: 'Methods', id: 'methods', line: 13, end_line: 20 },
        { level: 3, title: 'Details', id: 'details', line: 17, end_line: 20 },
        { level: 1, title: 'Results', id: 'results', line: 21, end_line: 23 },
      ]);
    } finally {
      await f.close();
    }
  });

  it('returns an empty outline (not an error) for a heading-less document', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'plain.qmd', content: 'Just prose.\n' }]);
      const result = await callTool(f, 'get_outline', {
        project: seed.indexDocId,
        path: 'plain.qmd',
      });
      expect(result.isError).not.toBe(true);
      expect(structuredOf(result).outline).toEqual([]);
    } finally {
      await f.close();
    }
  });

  it('errors on a non-qmd file, naming the .qmd requirement', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'data.csv', content: 'a,b\n1,2\n' }]);
      const result = await callTool(f, 'get_outline', {
        project: seed.indexDocId,
        path: 'data.csv',
      });
      expect(result.isError).toBe(true);
      const block = (result.content as Array<{ type: string; text?: string }>)[0];
      expect(block?.text).toMatch(/\.qmd/);
    } finally {
      await f.close();
    }
  });

  it('is on the read-only surface', async () => {
    const f = await startInMemoryMcp({ readOnly: true });
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.map((t) => t.name)).toContain('get_outline');
    } finally {
      await f.close();
    }
  });
});
