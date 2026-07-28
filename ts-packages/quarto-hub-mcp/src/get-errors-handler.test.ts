/**
 * Handler-level tests for the `get_errors` tool (handleGetErrors).
 *
 * Same harness pattern as wait-for-change-handler.test.ts: the REAL
 * `registerTools` dispatch runs against a fake ConnectionManager whose
 * `connect` returns a fabricated project state. Coverage:
 *
 *   - the no-publisher note when the diagnostics sidecar is empty
 *   - `stale` false when the stored contentHash matches the current text
 *   - `stale` true on hash mismatch, missing file, and binary payloads
 *   - errors/warnings split by diagnostic kind
 *   - execution errors surfaced from the captures sidecar (state 'error'
 *     with lastError; 'running' surfaced; 'idle' suppressed)
 *   - the optional `path` filter
 */

import { createHash } from 'node:crypto';
import { describe, it, expect } from 'vitest';
import { CallToolRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type { FilePayload, CaptureRef, FileDiagnostics } from '@quarto/quarto-sync-client';
import { registerTools } from './tools.js';
import type { ConnectionManager } from './connection-manager.js';

function sha256(text: string): string {
  return `sha256:${createHash('sha256').update(text, 'utf8').digest('hex')}`;
}

interface FakeStateInit {
  files?: Record<string, FilePayload>;
  captures?: Record<string, CaptureRef>;
  diagnostics?: Record<string, FileDiagnostics>;
}

/**
 * Register the real tool handlers against a fake manager whose `connect`
 * resolves to a state fabricated from `init`.
 */
function harness(init: FakeStateInit): {
  call: (args: Record<string, unknown>) => Promise<CallToolResult>;
} {
  const state = {
    client: {} as never,
    files: new Map(Object.entries(init.files ?? {})),
    waiters: new Set(),
    sidecars: {
      captures: init.captures ?? {},
      diagnostics: init.diagnostics ?? {},
    },
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

  const call = (args: Record<string, unknown>) =>
    callToolHandler!({ params: { name: 'get_errors', arguments: args } }, {});

  return { call };
}

function parse(result: CallToolResult): Record<string, unknown> {
  const block = result.content[0];
  if (block.type !== 'text') throw new Error('expected a text result block');
  return JSON.parse(block.text) as Record<string, unknown>;
}

const ERROR_ITEM = {
  kind: 'error' as const,
  title: 'YAML parse error',
  hints: ['close the bracket'],
  start_line: 2,
  start_column: 8,
  details: [],
};

const WARNING_ITEM = {
  kind: 'warning' as const,
  title: 'unknown option',
  hints: [],
  details: [],
};

function fileDiagnostics(
  content: string,
  items: FileDiagnostics['items'],
): FileDiagnostics {
  return {
    contentHash: sha256(content),
    asOf: '2026-07-16T12:00:00.000Z',
    source: 'hub-client-preview',
    items,
  };
}

describe('handleGetErrors — no publisher yet', () => {
  it('returns a note explaining diagnostics appear when a preview is open', async () => {
    const h = harness({ files: { 'index.qmd': { type: 'text', text: '# hi' } } });
    const out = parse(await h.call({ project: 'idx' }));
    expect(out.note).toContain('preview');
    expect(out.files).toEqual([]);
  });

  it('still reports execution errors when only the captures sidecar exists', async () => {
    const h = harness({
      files: { 'index.qmd': { type: 'text', text: '# hi' } },
      captures: {
        'index.qmd': { captureDocId: 'cap-1', state: 'error', lastError: 'kernel died' },
      },
    });
    const out = parse(await h.call({ project: 'idx' }));
    expect(out.note).toContain('preview');
    const files = out.files as Array<Record<string, unknown>>;
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe('index.qmd');
    expect(files[0].execution).toEqual({ state: 'error', lastError: 'kernel died' });
  });
});

describe('handleGetErrors — staleness', () => {
  it('reports stale: false when the stored hash matches the current file text', async () => {
    const content = '---\ntitle: [broken\n---\n';
    const h = harness({
      files: { 'index.qmd': { type: 'text', text: content } },
      diagnostics: { 'index.qmd': fileDiagnostics(content, [ERROR_ITEM]) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as Array<Record<string, unknown>>;
    expect(files).toHaveLength(1);
    const preview = files[0].preview as Record<string, unknown>;
    expect(preview.stale).toBe(false);
    expect(preview.asOf).toBe('2026-07-16T12:00:00.000Z');
    expect(preview.source).toBe('hub-client-preview');
  });

  it('reports stale: true when the file has changed since the render', async () => {
    const h = harness({
      files: { 'index.qmd': { type: 'text', text: 'edited since the render' } },
      diagnostics: { 'index.qmd': fileDiagnostics('what was rendered', [ERROR_ITEM]) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as Array<Record<string, unknown>>;
    expect((files[0].preview as Record<string, unknown>).stale).toBe(true);
  });

  it('reports stale: true when the file is missing from the project', async () => {
    const h = harness({
      diagnostics: { 'gone.qmd': fileDiagnostics('old content', [ERROR_ITEM]) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as Array<Record<string, unknown>>;
    expect((files[0].preview as Record<string, unknown>).stale).toBe(true);
  });

  it('reports stale: true when the payload is binary', async () => {
    const h = harness({
      files: { 'img.png': { type: 'binary', data: new Uint8Array([1]), mimeType: 'image/png' } },
      diagnostics: { 'img.png': fileDiagnostics('old text', [ERROR_ITEM]) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as Array<Record<string, unknown>>;
    expect((files[0].preview as Record<string, unknown>).stale).toBe(true);
  });
});

describe('handleGetErrors — content shape', () => {
  it('splits items into errors and warnings by kind', async () => {
    const content = 'x';
    const h = harness({
      files: { 'a.qmd': { type: 'text', text: content } },
      diagnostics: { 'a.qmd': fileDiagnostics(content, [ERROR_ITEM, WARNING_ITEM]) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const preview = (out.files as Array<Record<string, unknown>>)[0]
      .preview as Record<string, unknown>;
    expect(preview.errors).toEqual([ERROR_ITEM]);
    expect(preview.warnings).toEqual([WARNING_ITEM]);
  });

  it('an empty items array reports a clean render (no errors, no warnings)', async () => {
    const content = '# fine';
    const h = harness({
      files: { 'a.qmd': { type: 'text', text: content } },
      diagnostics: { 'a.qmd': fileDiagnostics(content, []) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const preview = (out.files as Array<Record<string, unknown>>)[0]
      .preview as Record<string, unknown>;
    expect(preview.errors).toEqual([]);
    expect(preview.warnings).toEqual([]);
    expect(preview.stale).toBe(false);
    expect(out.note).toBeUndefined();
  });

  it('surfaces a running capture but suppresses idle ones', async () => {
    const content = 'x';
    const h = harness({
      files: {
        'running.qmd': { type: 'text', text: content },
        'idle.qmd': { type: 'text', text: content },
      },
      captures: {
        'running.qmd': { captureDocId: 'cap-r', state: 'running' },
        'idle.qmd': { captureDocId: 'cap-i', state: 'idle' },
      },
      diagnostics: {
        'running.qmd': fileDiagnostics(content, []),
        'idle.qmd': fileDiagnostics(content, []),
      },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as Array<Record<string, unknown>>;
    const running = files.find((f) => f.path === 'running.qmd')!;
    const idle = files.find((f) => f.path === 'idle.qmd')!;
    expect(running.execution).toEqual({ state: 'running' });
    expect(idle.execution).toBeUndefined();
  });

  it('filters to a single path when `path` is given', async () => {
    const content = 'x';
    const h = harness({
      files: {
        'a.qmd': { type: 'text', text: content },
        'b.qmd': { type: 'text', text: content },
      },
      diagnostics: {
        'a.qmd': fileDiagnostics(content, [ERROR_ITEM]),
        'b.qmd': fileDiagnostics(content, [WARNING_ITEM]),
      },
    });
    const out = parse(await h.call({ project: 'idx', path: 'b.qmd' }));
    const files = out.files as Array<Record<string, unknown>>;
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe('b.qmd');
  });

  it('lists paths sorted and unions diagnostics + capture paths', async () => {
    const content = 'x';
    const h = harness({
      files: { 'b.qmd': { type: 'text', text: content } },
      captures: { 'a.qmd': { captureDocId: 'cap', state: 'error', lastError: 'boom' } },
      diagnostics: { 'b.qmd': fileDiagnostics(content, [ERROR_ITEM]) },
    });
    const out = parse(await h.call({ project: 'idx' }));
    const files = out.files as Array<Record<string, unknown>>;
    expect(files.map((f) => f.path)).toEqual(['a.qmd', 'b.qmd']);
  });
});
