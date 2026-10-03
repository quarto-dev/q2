/**
 * Handler-level tests for the `get_errors` tool (v2: local validation).
 *
 * Same harness pattern as wait-for-change-handler.test.ts: the REAL
 * `registerTools` dispatch runs against a fake ConnectionManager. The
 * local renderer is mocked at its module seam — its own behavior is
 * covered by local-render.test.ts against the real WASM.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CallToolRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type { FilePayload, CaptureRef } from '@quarto/quarto-sync-client';
import type { LocalRenderResult } from './local-render.js';

const renderDiagnostics = vi.hoisted(() => vi.fn());
vi.mock('./local-render.js', () => ({ renderDiagnostics }));

import { registerTools } from './tools.js';
import type { ConnectionManager } from './connection-manager.js';

const ERROR_ITEM = {
  kind: 'error' as const,
  title: 'Unclosed Strong Star Emphasis',
  hints: [],
  start_line: 5,
  start_column: 24,
  details: [],
};
const WARNING_ITEM = { kind: 'warning' as const, title: 'unknown option', hints: [], details: [] };

function cleanResult(overrides: Partial<LocalRenderResult> = {}): LocalRenderResult {
  return {
    checkedContentSha256: 'sha256:abc',
    errors: [],
    warnings: [],
    pass1Failures: [],
    ...overrides,
  };
}

interface FakeStateInit {
  files?: Record<string, FilePayload>;
  captures?: Record<string, CaptureRef>;
}

function harness(init: FakeStateInit): {
  call: (args: Record<string, unknown>) => Promise<CallToolResult>;
} {
  const state = {
    client: {} as never,
    files: new Map(Object.entries(init.files ?? {})),
    waiters: new Set(),
    sidecars: { captures: init.captures ?? {} },
  };
  const manager = {
    async connect(_project: string) {
      return state;
    },
  } as unknown as ConnectionManager;

  let callToolHandler:
    | ((req: { params: { name: string; arguments?: Record<string, unknown> } }, extra: unknown) => Promise<CallToolResult>)
    | undefined;
  const server = {
    setRequestHandler(schema: unknown, cb: unknown) {
      if (schema === CallToolRequestSchema) {
        callToolHandler = cb as typeof callToolHandler;
      }
    },
  } as unknown as Server;

  registerTools(server, manager, false);
  if (!callToolHandler) throw new Error('CallTool handler was not registered');

  return {
    call: (args) => callToolHandler!({ params: { name: 'get_errors', arguments: args } }, {}),
  };
}

function parse(result: CallToolResult): Record<string, unknown> {
  const block = result.content[0];
  if (block.type !== 'text') throw new Error('expected a text result block');
  return JSON.parse(block.text) as Record<string, unknown>;
}

type FileEntry = {
  path: string;
  checkedContentSha256?: string;
  errors?: unknown[];
  warnings?: unknown[];
  note?: string;
  execution?: Record<string, unknown>;
};

beforeEach(() => {
  renderDiagnostics.mockReset();
  renderDiagnostics.mockResolvedValue(cleanResult());
});

describe('handleGetErrors — local validation', () => {
  it('renders the requested path and reports its diagnostics + content hash', async () => {
    renderDiagnostics.mockResolvedValue(
      cleanResult({ errors: [ERROR_ITEM], warnings: [WARNING_ITEM], checkedContentSha256: 'sha256:def' }),
    );
    const h = harness({ files: { 'index.qmd': { type: 'text', text: 'x' } } });

    const out = parse(await h.call({ project: 'idx', path: 'index.qmd' }));
    const files = out.files as FileEntry[];
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe('index.qmd');
    expect(files[0].checkedContentSha256).toBe('sha256:def');
    expect(files[0].errors).toEqual([ERROR_ITEM]);
    expect(files[0].warnings).toEqual([WARNING_ITEM]);
    expect(renderDiagnostics).toHaveBeenCalledTimes(1);
  });

  it('renders every .qmd when no path is given, sorted', async () => {
    const h = harness({
      files: {
        'b.qmd': { type: 'text', text: 'b' },
        'a.qmd': { type: 'text', text: 'a' },
        '_quarto.yml': { type: 'text', text: 'project:\n' },
        'img.png': { type: 'binary', data: new Uint8Array([1]), mimeType: 'image/png' },
      },
    });

    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as FileEntry[];
    expect(files.map((f) => f.path)).toEqual(['a.qmd', 'b.qmd']);
    const rendered = renderDiagnostics.mock.calls.map((c) => c[1]);
    expect(rendered).toEqual(['a.qmd', 'b.qmd']);
  });

  it('folds a sibling pass-1 failure into the sibling entry', async () => {
    renderDiagnostics.mockImplementation(async (_files, path: string) =>
      path === 'index.qmd'
        ? cleanResult({ pass1Failures: [{ path: 'about.qmd', errors: [ERROR_ITEM] }] })
        : cleanResult(),
    );
    const h = harness({ files: { 'index.qmd': { type: 'text', text: 'x' } } });

    const out = parse(await h.call({ project: 'idx', path: 'index.qmd' }));
    const files = out.files as FileEntry[];
    const sibling = files.find((f) => f.path === 'about.qmd');
    expect(sibling).toBeDefined();
    expect(sibling!.errors).toEqual([ERROR_ITEM]);
  });

  it('surfaces capture execution errors and running state, suppresses idle', async () => {
    const h = harness({
      files: { 'a.qmd': { type: 'text', text: 'x' } },
      captures: {
        'a.qmd': { captureDocId: 'c1', state: 'error', lastError: 'kernel died' },
        'b.qmd': { captureDocId: 'c2', state: 'running' },
        'c.qmd': { captureDocId: 'c3', state: 'idle' },
      },
    });

    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as FileEntry[];
    expect(files.find((f) => f.path === 'a.qmd')!.execution).toEqual({
      state: 'error',
      lastError: 'kernel died',
    });
    expect(files.find((f) => f.path === 'b.qmd')!.execution).toEqual({ state: 'running' });
    expect(files.find((f) => f.path === 'c.qmd')).toBeUndefined();
  });

  it('errors clearly when the requested path is not a text file', async () => {
    const h = harness({
      files: { 'img.png': { type: 'binary', data: new Uint8Array([1]), mimeType: 'image/png' } },
    });
    const res = await h.call({ project: 'idx', path: 'img.png' });
    expect(res.isError).toBe(true);
  });

  it('errors clearly when the requested path is missing', async () => {
    const h = harness({ files: {} });
    const res = await h.call({ project: 'idx', path: 'nope.qmd' });
    expect(res.isError).toBe(true);
  });
});
