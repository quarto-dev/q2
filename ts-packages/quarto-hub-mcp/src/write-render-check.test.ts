/**
 * Write tools auto-check the content they just wrote: after write_file /
 * patch_file / create_file touches a .qmd, the response carries a render
 * check of the NEW content so the agent sees immediately whether the
 * edit broke (or fixed) the document — no separate get_errors call
 * needed to close a batch of updates.
 *
 * Same harness pattern as get-errors-handler.test.ts: real
 * `registerTools` dispatch, fake ConnectionManager, renderer mocked at
 * the module seam (its behavior is covered by local-render.test.ts).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { CallToolResult, McpServer } from '@modelcontextprotocol/server';
import type { FilePayload } from '@quarto/quarto-sync-client';
import type { LocalRenderResult } from './local-render.js';

const renderDiagnostics = vi.hoisted(() => vi.fn());
vi.mock('./local-render.js', () => ({ renderDiagnostics }));

import { registerTools } from './tools.js';
import { RENDER_CHECK_TIMEOUT_MS } from './timeout.js';
import type { ConnectionManager } from './connection-manager.js';

const ERROR_ITEM = {
  kind: 'error' as const,
  title: 'Unclosed Strong Star Emphasis',
  hints: [],
  start_line: 16,
  start_column: 50,
  details: [],
};
const WARNING_ITEM = { kind: 'warning' as const, title: 'raw HTML', hints: [], details: [] };

function cleanResult(overrides: Partial<LocalRenderResult> = {}): LocalRenderResult {
  return {
    checkedContentSha256: 'sha256:abc',
    errors: [],
    warnings: [],
    pass1Failures: [],
    ...overrides,
  };
}

function harness(files: Record<string, FilePayload>): {
  call: (name: string, args: Record<string, unknown>) => Promise<CallToolResult>;
  files: Map<string, FilePayload>;
} {
  const fileMap = new Map(Object.entries(files));
  const client = {
    getUnavailableFiles: () => [],
    updateFileContent: (path: string, content: string) => {
      fileMap.set(path, { type: 'text', text: content });
    },
    createFile: async (path: string, content: string) => {
      fileMap.set(path, { type: 'text', text: content });
    },
  };
  const state = {
    client: client as never,
    files: fileMap,
    waiters: new Set(),
    sidecars: { captures: {} },
  };
  const manager = {
    async connect(_project: string) {
      return state;
    },
    // ERG-2 bounded delivery wait: report every write as acknowledged.
    async awaitDelivery(_project: string, _paths: string[], _timeoutMs: number) {
      return true;
    },
  } as unknown as ConnectionManager;

  const callbacks = new Map<string, (args: Record<string, unknown>) => Promise<CallToolResult>>();
  const server = {
    registerTool(name: string, _config: unknown, cb: unknown) {
      callbacks.set(name, cb as (args: Record<string, unknown>) => Promise<CallToolResult>);
    },
  } as unknown as McpServer;

  registerTools(server, manager, false);

  return {
    call: (name, args) => {
      const cb = callbacks.get(name);
      if (!cb) throw new Error(`${name} callback was not registered`);
      return cb(args);
    },
    files: fileMap,
  };
}

function textOf(result: CallToolResult): string {
  const block = result.content[0];
  if (block.type !== 'text') throw new Error('expected a text result block');
  return block.text;
}

// Write-tool responses are structured (BP-1): the text block is the JSON
// envelope; the render check rides as its `renderCheck` field.
function parse(result: CallToolResult): Record<string, unknown> {
  return JSON.parse(textOf(result)) as Record<string, unknown>;
}

beforeEach(() => {
  renderDiagnostics.mockReset();
  renderDiagnostics.mockResolvedValue(cleanResult());
});

describe('write tools render-check the new .qmd content', () => {
  it('patch_file reports a clean render check for the patched content', async () => {
    const h = harness({ 'a.qmd': { type: 'text', text: 'Hello **world**\n' } });

    const res = await h.call('patch_file', {
      project: 'idx',
      path: 'a.qmd',
      old_string: 'world',
      new_string: 'there',
    });

    const out = parse(res);
    expect(out.path).toBe('a.qmd');
    expect(out.renderCheck).toMatch(/render check: clean/i);
    // The check ran against the NEW content, not the pre-edit content.
    expect(renderDiagnostics).toHaveBeenCalledTimes(1);
    const [checkedFiles, checkedPath] = renderDiagnostics.mock.calls[0] as [
      Map<string, FilePayload>,
      string,
    ];
    expect(checkedPath).toBe('a.qmd');
    const payload = checkedFiles.get('a.qmd');
    expect(payload?.type === 'text' && payload.text).toBe('Hello **there**\n');
  });

  it('patch_file reports the errors the new content renders with', async () => {
    renderDiagnostics.mockResolvedValue(cleanResult({ errors: [ERROR_ITEM] }));
    const h = harness({ 'a.qmd': { type: 'text', text: 'fine\n' } });

    const res = await h.call('patch_file', {
      project: 'idx',
      path: 'a.qmd',
      old_string: 'fine',
      new_string: '**broken',
    });

    const out = parse(res);
    expect(out.path).toBe('a.qmd');
    expect(out.renderCheck).toMatch(/render check: 1 error/i);
    expect(out.renderCheck).toContain('Unclosed Strong Star Emphasis');
    expect(res.isError).not.toBe(true); // the write itself succeeded
  });

  it('write_file (update) render-checks the replacement content', async () => {
    const h = harness({ 'a.qmd': { type: 'text', text: 'old\n' } });

    const res = await h.call('write_file', { project: 'idx', path: 'a.qmd', content: 'new\n' });

    expect(parse(res).renderCheck).toMatch(/render check: clean/i);
    const [checkedFiles] = renderDiagnostics.mock.calls[0] as [Map<string, FilePayload>, string];
    const payload = checkedFiles.get('a.qmd');
    expect(payload?.type === 'text' && payload.text).toBe('new\n');
  });

  it('write_file (create) and create_file render-check the initial content', async () => {
    const h = harness({});
    const created = await h.call('write_file', { project: 'idx', path: 'new.qmd', content: 'x\n' });
    expect(parse(created).created).toBe(true);
    expect(parse(created).renderCheck).toMatch(/render check: clean/i);

    const h2 = harness({});
    const created2 = await h2.call('create_file', { project: 'idx', path: 'n2.qmd', content: 'y\n' });
    expect(parse(created2).created).toBe(true);
    expect(parse(created2).renderCheck).toMatch(/render check: clean/i);
  });

  it('mentions warning count on a clean check but does not dump warnings', async () => {
    renderDiagnostics.mockResolvedValue(cleanResult({ warnings: [WARNING_ITEM, WARNING_ITEM] }));
    const h = harness({ 'a.qmd': { type: 'text', text: 'x\n' } });

    const res = await h.call('write_file', { project: 'idx', path: 'a.qmd', content: 'y\n' });

    const out = parse(res);
    expect(out.renderCheck).toMatch(/render check: clean \(2 warnings/i);
    expect(out.renderCheck).not.toContain('raw HTML');
  });

  it('does not render-check non-qmd writes', async () => {
    const h = harness({ '_quarto.yml': { type: 'text', text: 'project:\n' } });

    const res = await h.call('write_file', { project: 'idx', path: '_quarto.yml', content: 'x\n' });

    expect(parse(res).path).toBe('_quarto.yml');
    expect(parse(res).renderCheck).toBeUndefined();
    expect(renderDiagnostics).not.toHaveBeenCalled();
  });

  it('a failed render check never fails the write', async () => {
    renderDiagnostics.mockRejectedValue(new Error('wasm exploded'));
    const h = harness({ 'a.qmd': { type: 'text', text: 'x\n' } });

    const res = await h.call('write_file', { project: 'idx', path: 'a.qmd', content: 'y\n' });

    expect(res.isError).not.toBe(true);
    const out = parse(res);
    expect(out.path).toBe('a.qmd');
    expect(out.renderCheck).toMatch(/render check unavailable/i);
    expect(out.renderCheck).toContain('get_errors');
  });

  it('a hung render check degrades after the timeout instead of hanging the write', async () => {
    vi.useFakeTimers();
    try {
      renderDiagnostics.mockReturnValue(new Promise(() => {}));
      const h = harness({ 'a.qmd': { type: 'text', text: 'x\n' } });

      const p = h.call('write_file', { project: 'idx', path: 'a.qmd', content: 'y\n' });
      await vi.advanceTimersByTimeAsync(RENDER_CHECK_TIMEOUT_MS + 1000);
      const res = await p;

      // The write itself succeeded; the check degraded, it did not hang.
      expect(res.isError).not.toBe(true);
      const out = parse(res);
      expect(out.path).toBe('a.qmd');
      expect(out.renderCheck).toMatch(/render check unavailable/i);
      expect(out.renderCheck).toMatch(/timed out/i);
    } finally {
      vi.useRealTimers();
    }
  });
});
