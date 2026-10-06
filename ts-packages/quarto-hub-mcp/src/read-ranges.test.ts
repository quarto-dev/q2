/**
 * Read ranges, truncation, and listing metadata (ERG-3): an agent must
 * be able to page a large file instead of swallowing it whole.
 *
 * - `read_file` gains `offset` (1-based line), `limit` (lines), and
 *   `max_bytes` (default 65536); a read that doesn't reach EOF reports
 *   `truncated: true` with a `next_offset` continuation and a hint.
 * - Window boundaries are line-aligned and byte-exact: walking
 *   `next_offset` reassembles the original file.
 * - `list_files` entries carry `size`, `mimeType`, and `lines` (text)
 *   so an agent can decide what to read before spending context.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

interface ReadResult {
  path: string;
  hash: string;
  type: 'text' | 'binary';
  content?: string;
  truncated?: boolean;
  total_lines?: number;
  next_offset?: number | null;
  hint?: string;
}

function structuredOf(result: { structuredContent?: unknown }): ReadResult {
  const sc = result.structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as ReadResult;
}

const LINES = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`);
const TEXT = LINES.join('\n') + '\n';

async function seedText(f: InMemoryMcpFixture, text = TEXT): Promise<string> {
  const seed = await seedProject(f, [{ path: 'big.qmd', content: text }]);
  return seed.indexDocId;
}

describe('read_file ranges (ERG-3)', () => {
  it('returns the requested line window with a continuation', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seedText(f);
      const read = structuredOf(
        await callTool(f, 'read_file', { project, path: 'big.qmd', offset: 3, limit: 2 }),
      );
      expect(read.content).toBe('line 3\nline 4\n');
      expect(read.total_lines).toBe(20);
      expect(read.truncated).toBe(true);
      expect(read.next_offset).toBe(5);
      expect(read.hint).toMatch(/offset=5/);
    } finally {
      await f.close();
    }
  });

  it('a window reaching EOF is not truncated and has no continuation', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seedText(f);
      const read = structuredOf(
        await callTool(f, 'read_file', { project, path: 'big.qmd', offset: 19, limit: 50 }),
      );
      expect(read.content).toBe('line 19\nline 20\n');
      expect(read.truncated).toBe(false);
      expect(read.next_offset).toBeNull();
    } finally {
      await f.close();
    }
  });

  it('a small whole-file read is byte-identical and untruncated', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seedText(f, 'hello\n');
      const read = structuredOf(await callTool(f, 'read_file', { project, path: 'big.qmd' }));
      expect(read.content).toBe('hello\n');
      expect(read.truncated).toBe(false);
      expect(read.total_lines).toBe(1);
    } finally {
      await f.close();
    }
  });

  it('max_bytes truncates at a line boundary and next_offset pages to a byte-exact reassembly', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seedText(f);
      let offset = 1;
      let reassembled = '';
      let pages = 0;
      for (;;) {
        const read = structuredOf(
          await callTool(f, 'read_file', {
            project,
            path: 'big.qmd',
            offset,
            max_bytes: 32, // ~4 lines of 7-8 bytes each
          }),
        );
        reassembled += read.content ?? '';
        pages++;
        // Every page except possibly the last respects the byte cap…
        if (read.truncated) {
          expect(Buffer.byteLength(read.content ?? '', 'utf8')).toBeLessThanOrEqual(32);
          expect(read.next_offset).toBeGreaterThan(offset);
          offset = read.next_offset as number;
        } else {
          expect(read.next_offset).toBeNull();
          break;
        }
        expect(pages).toBeLessThan(50); // termination guard
      }
      expect(reassembled).toBe(TEXT);
      expect(pages).toBeGreaterThan(2); // the cap really did page
    } finally {
      await f.close();
    }
  });

  it('offset beyond EOF is an actionable error naming the file length', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seedText(f);
      const result = await callTool(f, 'read_file', {
        project,
        path: 'big.qmd',
        offset: 100,
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/offset/);
        expect(block.text).toMatch(/20/);
      }
    } finally {
      await f.close();
    }
  });

  it('a file larger than the default byte cap truncates with a hint', async () => {
    const f = await startInMemoryMcp();
    try {
      const big = 'x'.repeat(70000) + '\n' + 'tail line\n';
      const project = await seedText(f, big);
      const read = structuredOf(await callTool(f, 'read_file', { project, path: 'big.qmd' }));
      expect(read.truncated).toBe(true);
      expect(read.total_lines).toBe(2);
      expect(read.hint).toBeDefined();
      // The first line alone exceeds the cap: the cut is byte-level, and
      // there is no line-aligned continuation to offer.
      expect(read.next_offset).toBeNull();
      expect(Buffer.byteLength(read.content ?? '', 'utf8')).toBeLessThanOrEqual(65536);
    } finally {
      await f.close();
    }
  });
});

describe('list_files metadata (ERG-3)', () => {
  it('entries carry size, mimeType, and lines (text) / size and mimeType (binary)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: TEXT }]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'logo.png',
        content: Buffer.from([0x89, 0x50, 0x4e, 0x47]).toString('base64'),
        encoding: 'base64',
      });
      const listed = (await callTool(f, 'list_files', { project: seed.indexDocId }))
        .structuredContent as {
        files: Array<Record<string, unknown>>;
      };
      const text = listed.files.find((e) => e.path === 'index.qmd');
      expect(text).toMatchObject({
        type: 'text',
        size: Buffer.byteLength(TEXT, 'utf8'),
        lines: 20,
      });
      expect(typeof text?.mimeType).toBe('string');
      expect(text?.mimeType).toMatch(/^text\//);

      const binary = listed.files.find((e) => e.path === 'logo.png');
      expect(binary).toMatchObject({ type: 'binary', size: 4, mimeType: 'image/png' });
      expect(binary?.lines).toBeUndefined();
    } finally {
      await f.close();
    }
  });
});
