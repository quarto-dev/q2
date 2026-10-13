/**
 * Tests for the `fix_errors` MCP prompt — the one-command entry into
 * the agent fix loop. The prompt only instructs; the LLM does the
 * fixing with the existing tools (get_errors, read_file, patch_file).
 *
 * Post-SDK-v2 the prompt is registered through `McpServer.registerPrompt`:
 * the harness captures the (name, config, callback) triple and drives the
 * callback directly. Argument requiredness lives in the zod `argsSchema`
 * (the SDK validates it at the protocol layer), so these tests assert on
 * the schema itself. An unknown prompt name is the SDK's own protocol
 * error, not our handler's concern, so that case is not tested here.
 */

import { describe, it, expect } from 'vitest';
import type { McpServer } from '@modelcontextprotocol/server';
import type { z } from 'zod';
import { registerPrompts } from './prompts.js';

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

function harness(): { name: string; config: CapturedPrompt['config']; get: CapturedPrompt['cb'] } {
  let captured: CapturedPrompt | undefined;
  const server = {
    registerPrompt(name: string, config: unknown, cb: unknown) {
      captured = { name, config: config as CapturedPrompt['config'], cb: cb as CapturedPrompt['cb'] };
    },
  } as unknown as McpServer;
  registerPrompts(server);
  if (!captured) throw new Error('prompt was not registered');
  return { name: captured.name, config: captured.config, get: captured.cb };
}

describe('fix_errors prompt', () => {
  it('is registered with a required project argument and optional path', () => {
    const { name, config } = harness();
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
    const { get } = harness();
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
    const { get } = harness();
    const res = get({ project: 'abc', path: 'chapter2.qmd' });
    expect(res.messages[0]!.content.text).toContain('chapter2.qmd');
  });
});
