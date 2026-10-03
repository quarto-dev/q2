/**
 * Tests for the `fix_errors` MCP prompt — the one-command entry into
 * the agent fix loop. The prompt only instructs; the LLM does the
 * fixing with the existing tools (get_errors, read_file, patch_file).
 */

import { describe, it, expect } from 'vitest';
import {
  ListPromptsRequestSchema,
  GetPromptRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { registerPrompts } from './prompts.js';

type Handler = (req: { params?: Record<string, unknown> }) => Promise<Record<string, unknown>>;

function harness(): { list: Handler; get: Handler } {
  let list: Handler | undefined;
  let get: Handler | undefined;
  const server = {
    setRequestHandler(schema: unknown, cb: unknown) {
      if (schema === ListPromptsRequestSchema) list = cb as Handler;
      if (schema === GetPromptRequestSchema) get = cb as Handler;
    },
  } as unknown as Server;
  registerPrompts(server);
  if (!list || !get) throw new Error('prompt handlers were not registered');
  return { list, get };
}

describe('fix_errors prompt', () => {
  it('is listed with a required project argument and optional path', async () => {
    const { list } = harness();
    const res = (await list({})) as {
      prompts: Array<{ name: string; arguments?: Array<{ name: string; required?: boolean }> }>;
    };
    const p = res.prompts.find((x) => x.name === 'fix_errors');
    expect(p).toBeDefined();
    expect(p!.arguments).toEqual([
      expect.objectContaining({ name: 'project', required: true }),
      expect.objectContaining({ name: 'path', required: false }),
    ]);
  });

  it('expands to loop instructions naming the project and the tools', async () => {
    const { get } = harness();
    const res = (await get({
      params: { name: 'fix_errors', arguments: { project: 'automerge:abc123' } },
    })) as { messages: Array<{ role: string; content: { type: string; text: string } }> };

    expect(res.messages).toHaveLength(1);
    expect(res.messages[0]!.role).toBe('user');
    const text = res.messages[0]!.content.text;
    expect(text).toContain('automerge:abc123');
    expect(text).toContain('get_errors');
    expect(text).toContain('patch_file');
    expect(text).toMatch(/until/i);
    expect(text).toMatch(/minimal/i);
  });

  it('scopes the instructions to a single file when path is given', async () => {
    const { get } = harness();
    const res = (await get({
      params: { name: 'fix_errors', arguments: { project: 'abc', path: 'chapter2.qmd' } },
    })) as { messages: Array<{ content: { text: string } }> };
    expect(res.messages[0]!.content.text).toContain('chapter2.qmd');
  });

  it('rejects an unknown prompt name', async () => {
    const { get } = harness();
    await expect(get({ params: { name: 'nope' } })).rejects.toThrow(/unknown prompt/i);
  });

  it('rejects a missing project argument', async () => {
    const { get } = harness();
    await expect(get({ params: { name: 'fix_errors', arguments: {} } })).rejects.toThrow(
      /project/i,
    );
  });
});
