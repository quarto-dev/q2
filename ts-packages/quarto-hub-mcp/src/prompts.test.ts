/**
 * MCP prompts surface (BP-6, Phase 5): guided workflow templates so
 * every host/user doesn't rediscover the correct call patterns. Four
 * templates, gated by the server's mode:
 *
 * - `review-draft` (project, path) — critical review of one file with
 *   the draft embedded (read-only safe);
 * - `collaborate-with-human` (project) — presence-aware collaboration
 *   etiquette (read-only safe);
 * - `safe-edit-workflow` (project, path) — the read → patch with
 *   expected_hash → confirm `synced` loop (write modes only);
 * - `fix-render-errors` (project, path?) — the render → diagnostics →
 *   patch → re-render loop (only when --allow-render exposed the
 *   render tool, CAP-12).
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

async function promptNames(f: InMemoryMcpFixture): Promise<string[]> {
  const result = await f.client.listPrompts();
  return result.prompts.map((p) => p.name).sort();
}

/** Flatten every text/resource block of a prompts/get result to text. */
async function promptText(
  f: InMemoryMcpFixture,
  name: string,
  args: Record<string, string>,
): Promise<string> {
  const result = await f.client.getPrompt({ name, arguments: args });
  expect(result.messages.length).toBeGreaterThan(0);
  return result.messages
    .map((m) => {
      const c = m.content;
      if (c.type === 'text') return c.text;
      if (c.type === 'resource') {
        const r = c.resource;
        return 'text' in r ? r.text : (r.blob ?? '');
      }
      return '';
    })
    .join('\n---\n');
}

describe('prompts/list (BP-6)', () => {
  it('exposes all four workflow templates on a full surface', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      expect(await promptNames(f)).toEqual([
        'collaborate-with-human',
        'fix-render-errors',
        'review-draft',
        'safe-edit-workflow',
      ]);
    } finally {
      await f.close();
    }
  });

  it('omits edit- and render-flavored prompts in read-only mode', async () => {
    const f = await startInMemoryMcp({ readOnly: true });
    try {
      expect(await promptNames(f)).toEqual(['collaborate-with-human', 'review-draft']);
    } finally {
      await f.close();
    }
  });

  it('omits fix-render-errors when the render tool is not exposed', async () => {
    const f = await startInMemoryMcp();
    try {
      expect(await promptNames(f)).toEqual([
        'collaborate-with-human',
        'review-draft',
        'safe-edit-workflow',
      ]);
    } finally {
      await f.close();
    }
  });

  it('declares argument schemas hosts can render', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const { prompts } = await f.client.listPrompts();
      const review = prompts.find((p) => p.name === 'review-draft');
      expect(review?.arguments?.map((a) => a.name).sort()).toEqual(['path', 'project']);
      expect(review?.arguments?.every((a) => a.required === true)).toBe(true);
      const fix = prompts.find((p) => p.name === 'fix-render-errors');
      expect(fix?.arguments?.map((a) => a.name)).toContain('project');
      expect(fix?.arguments?.find((a) => a.name === 'path')?.required).toBe(false);
    } finally {
      await f.close();
    }
  });
});

describe('prompts/get (BP-6)', () => {
  it('review-draft embeds the draft and the review protocol', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'draft.qmd', content: '# My Draft\n\nSome text.\n' },
      ]);
      const text = await promptText(f, 'review-draft', {
        project: seed.indexDocId,
        path: 'draft.qmd',
      });
      expect(text).toContain('# My Draft');
      expect(text).toContain('Some text.');
      expect(text).toMatch(/review/i);
      // The prompt must steer toward proposing patches, not rewrites.
      expect(text).toMatch(/patch_file/);
    } finally {
      await f.close();
    }
  });

  it('safe-edit-workflow teaches the hash-disciplined edit loop', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const text = await promptText(f, 'safe-edit-workflow', {
        project: seed.indexDocId,
        path: 'a.qmd',
      });
      expect(text).toMatch(/read_file/);
      expect(text).toMatch(/expected_hash/);
      expect(text).toMatch(/patch_file/);
      expect(text).toMatch(/synced/);
    } finally {
      await f.close();
    }
  });

  it('collaborate-with-human teaches presence-aware etiquette', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const text = await promptText(f, 'collaborate-with-human', {
        project: seed.indexDocId,
      });
      expect(text).toMatch(/list_presence/);
      expect(text).toMatch(/wait_for_change/);
      expect(text).toMatch(/expected_hash/);
    } finally {
      await f.close();
    }
  });

  it('fix-render-errors teaches the render → diagnostics → patch loop', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const text = await promptText(f, 'fix-render-errors', {
        project: seed.indexDocId,
        path: 'a.qmd',
      });
      expect(text).toMatch(/render/);
      expect(text).toMatch(/diagnostics/);
      expect(text).toMatch(/patch_file/);
    } finally {
      await f.close();
    }
  });

  it('rejects unknown prompt names with a protocol error', async () => {
    const f = await startInMemoryMcp();
    try {
      await expect(
        f.client.getPrompt({ name: 'no-such-prompt', arguments: {} }),
      ).rejects.toThrow(/no-such-prompt/);
    } finally {
      await f.close();
    }
  });
});
