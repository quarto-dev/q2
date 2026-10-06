/**
 * `search_files` (CAP-7): substring/regex content search across a
 * project's text files with context snippets and a result cap.
 * Client-side scan — right-sized at project scale, same approach as the
 * web client's in-memory provider. Binary files are skipped (they have
 * no searchable text); dangling entries are skipped (they have no
 * content at all). Read-only tool: it joins the `--read-only` surface.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

interface SearchMatch {
  path: string;
  line: number;
  snippet: string;
}

interface SearchResult {
  matches: SearchMatch[];
  total_matches: number;
  files_searched: number;
  truncated: boolean;
}

async function search(
  f: InMemoryMcpFixture,
  project: string,
  args: Record<string, unknown>,
): Promise<{ result: Awaited<ReturnType<typeof callTool>>; parsed?: SearchResult }> {
  const result = await callTool(f, 'search_files', { project, ...args });
  if (result.isError === true) return { result };
  return { result, parsed: result.structuredContent as unknown as SearchResult };
}

const FILES = [
  { path: 'index.qmd', content: '---\ntitle: Home\n---\n\nSee the turtles guide.\n' },
  { path: 'docs/guide.qmd', content: 'The guide covers turtles.\nTurtles appear twice here.\n' },
  { path: 'docs/deep/nested.qmd', content: 'nothing relevant\n' },
  { path: 'README.md', content: 'Turtle soup recipe\n' },
];

describe('search_files (CAP-7)', () => {
  it('finds substring matches with line numbers and snippets, ranked by per-file count', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, FILES);
      const { result, parsed } = await search(f, seed.indexDocId, { query: 'turtles' });
      expect(result.isError).not.toBe(true);
      expect(parsed).toBeDefined();
      // Case-insensitive by default: guide×2 ("turtles", "Turtles") + index×1.
      // ("Turtle soup" in README is not a match — "turtle" ≠ "turtles".)
      expect(parsed!.total_matches).toBe(3);
      // Ranked by per-file count: guide (2) before index (1).
      expect(parsed!.matches.map((m) => m.path)).toEqual([
        'docs/guide.qmd',
        'docs/guide.qmd',
        'index.qmd',
      ]);
      expect(parsed!.matches[0]).toMatchObject({ line: 1, snippet: 'The guide covers turtles.' });
      expect(parsed!.matches[1]).toMatchObject({ line: 2 });
      expect(parsed!.matches[2]).toMatchObject({ line: 5 });
      expect(parsed!.truncated).toBe(false);
      expect(parsed!.files_searched).toBe(4);
    } finally {
      await f.close();
    }
  });

  it('case_sensitive: true narrows to exact-case matches', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, FILES);
      const { parsed } = await search(f, seed.indexDocId, {
        query: 'Turtle',
        case_sensitive: true,
      });
      // "Turtle" matches inside "Turtles" (guide line 2) and "Turtle soup"
      // (README line 1); the lowercase hits are excluded. Tie on count →
      // path order: README.md before docs/guide.qmd.
      expect(parsed!.total_matches).toBe(2);
      expect(parsed!.matches.map((m) => m.path)).toEqual(['README.md', 'docs/guide.qmd']);
      expect(parsed!.matches[0]).toMatchObject({ line: 1 });
      expect(parsed!.matches[1]).toMatchObject({ line: 2 });
    } finally {
      await f.close();
    }
  });

  it('regex mode matches patterns; an invalid pattern is an actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'paper.qmd', content: 'See @fig-12 and @fig-3 but not figures.\n' },
      ]);
      const { parsed } = await search(f, seed.indexDocId, { query: '@fig-\\d+', regex: true });
      expect(parsed!.total_matches).toBe(1);
      expect(parsed!.matches[0].snippet).toContain('@fig-12');

      const bad = await search(f, seed.indexDocId, { query: '[unclosed', regex: true });
      expect(bad.result.isError).toBe(true);
      const block = bad.result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/regular expression/);
        expect(block.text).toMatch(/regex: false/);
      }
    } finally {
      await f.close();
    }
  });

  it('max_results caps the returned matches and reports truncation with the full count', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'a.qmd', content: 'hit\nhit\nhit\nhit\nhit\n' },
      ]);
      const { parsed } = await search(f, seed.indexDocId, { query: 'hit', max_results: 2 });
      expect(parsed!.matches.length).toBe(2);
      expect(parsed!.total_matches).toBe(5);
      expect(parsed!.truncated).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('skips binary files (files_searched counts text only)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'plain text\n' }]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'payload.bin',
        // base64 of bytes whose ASCII decoding contains the query string
        content: Buffer.from('secret query payload bytes').toString('base64'),
        encoding: 'base64',
        mime_type: 'application/octet-stream',
      });
      const { parsed } = await search(f, seed.indexDocId, { query: 'secret query payload' });
      expect(parsed!.total_matches).toBe(0);
      expect(parsed!.files_searched).toBe(1);
    } finally {
      await f.close();
    }
  });

  it('an empty query is refused', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, FILES);
      const { result } = await search(f, seed.indexDocId, { query: '' });
      expect(result.isError).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('is part of the read-only surface', async () => {
    const ro = await startInMemoryMcp({ readOnly: true });
    try {
      const tools = await ro.client.listTools();
      expect(tools.tools.map((t) => t.name)).toContain('search_files');
    } finally {
      await ro.close();
    }
  });
});
