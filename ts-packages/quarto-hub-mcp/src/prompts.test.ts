/**
 * Tests for the MCP prompts surface (BP-6, Phase 5 + 452's fix_errors):
 * guided workflow templates so every host/user doesn't rediscover the
 * correct call patterns. Five templates, gated by the server's mode:
 *
 * - `review-draft` (project, path) — critical review of one file with
 *   the draft embedded (read-only safe);
 * - `collaborate-with-human` (project) — presence-aware collaboration
 *   etiquette (read-only safe);
 * - `safe-edit-workflow` (project, path) — the read → patch with
 *   expected_hash → confirm `synced` loop (write modes only);
 * - `fix_errors` (project, path?) — the get_errors → patch_file →
 *   re-check loop (write modes only; 452);
 * - `fix-render-errors` (project, path?) — the render → diagnostics →
 *   patch → re-render loop (only when --allow-render exposed the
 *   render tool, CAP-12).
 *
 * The gating/listing tests run against the in-memory server. fix_errors
 * content tests use a capture harness: the prompt only instructs, so its
 * callback is driven directly (a stub ConnectionManager suffices — the
 * template never touches it). Argument requiredness lives in the zod
 * `argsSchema` (the SDK validates it at the protocol layer).
 */

import { describe, it, expect } from 'vitest';
import type { McpServer } from '@modelcontextprotocol/server';
import type { z } from 'zod';

import type { ConnectionManager } from './connection-manager.js';
import { registerPrompts } from './prompts.js';
import {
  startInMemoryMcp,
  seedProject,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

type PromptArgs = { project: string; path?: string };
type PromptResult = {
  description?: string;
  messages: Array<{ role: string; content: { type: string; text: string } }>;
};

interface CapturedPrompt {
  name: string;
  config: { description?: string; argsSchema: z.ZodType<PromptArgs> };
  cb: (args: PromptArgs) => PromptResult;
}

function fixErrorsHarness(): {
  name: string;
  config: CapturedPrompt['config'];
  get: CapturedPrompt['cb'];
} {
  const captured: CapturedPrompt[] = [];
  const server = {
    registerPrompt(name: string, config: unknown, cb: unknown) {
      captured.push({
        name,
        config: config as CapturedPrompt['config'],
        cb: cb as CapturedPrompt['cb'],
      });
    },
  } as unknown as McpServer;
  registerPrompts(server, {} as unknown as ConnectionManager, {
    readOnly: false,
    allowRender: false,
  });
  const prompt = captured.find((p) => p.name === 'fix_errors');
  if (!prompt) throw new Error('fix_errors prompt was not registered');
  return { name: prompt.name, config: prompt.config, get: prompt.cb };
}

describe('fix_errors prompt', () => {
  it('is registered with a required project argument and optional path', () => {
    const { name, config } = fixErrorsHarness();
    expect(name).toBe('fix_errors');
    expect(config.description).toBeTruthy();
    expect(config.argsSchema.safeParse({ project: 'automerge:abc123' }).success).toBe(true);
    expect(config.argsSchema.safeParse({ project: 'abc', path: 'chapter2.qmd' }).success).toBe(
      true,
    );
    const missing = config.argsSchema.safeParse({});
    expect(missing.success).toBe(false);
    if (!missing.success) {
      expect(missing.error.issues[0]!.path).toContain('project');
    }
  });

  it('expands to loop instructions naming the project and the tools', () => {
    const { get } = fixErrorsHarness();
    const res = get({ project: 'automerge:abc123' });

    expect(res.messages).toHaveLength(1);
    expect(res.messages[0]!.role).toBe('user');
    const text = res.messages[0]!.content.text;
    expect(text).toContain('automerge:abc123');
    expect(text).toContain('get_errors');
    expect(text).toContain('patch_file');
    expect(text).toMatch(/until/i);
    expect(text).toMatch(/minimal/i);
  });

  it('scopes the instructions to a single file when path is given', () => {
    const { get } = fixErrorsHarness();
    const res = get({ project: 'abc', path: 'chapter2.qmd' });
    expect(res.messages[0]!.content.text).toContain('chapter2.qmd');
  });
});

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
  it('exposes all five workflow templates on a full surface', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      expect(await promptNames(f)).toEqual([
        'collaborate-with-human',
        'fix-render-errors',
        'fix_errors',
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
        'fix_errors',
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
