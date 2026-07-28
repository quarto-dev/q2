/**
 * MCP-level integration test for `get_errors` against the in-process
 * test hub: the diagnostics sidecar is planted by mutating the project
 * index through the hub's own repo handle (standing in for the
 * hub-client preview publisher), then a fresh MCP server instance
 * reads it back through the real tool surface — including the
 * staleness flip after the agent edits the file via `write_file`.
 *
 * Same harness as dangling-entries.test.ts: drives the real server
 * binary (dist/index.js) over stdio; no external network.
 */

import { createHash } from 'node:crypto';
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import type { DocumentId } from '@automerge/automerge-repo';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';

import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

const BROKEN_CONTENT = '---\ntitle: [broken\n---\n';

function sha256(text: string): string {
  return `sha256:${createHash('sha256').update(text, 'utf8').digest('hex')}`;
}

describe('get_errors at the MCP tool surface (test hub)', () => {
  let hub: TestHub;
  let client: McpTestClient;
  let indexDocId: string;

  beforeAll(async () => {
    hub = await startTestHub();

    const creator = new McpTestClient();
    await creator.start(['--server', hub.url]);
    const created = await creator.callTool('create_project', {
      files: [{ path: 'index.qmd', content: BROKEN_CONTENT }],
    });
    expect(created.isError).not.toBe(true);
    const parsed = JSON.parse(created.content[0]!.text) as {
      indexDocId: string;
      files: Array<{ path: string; docId: string }>;
    };
    indexDocId = parsed.indexDocId;
    expect(await hub.hubHasDoc(indexDocId, 8000)).toBe(true);
    for (const f of parsed.files) {
      expect(await hub.hubHasDoc(f.docId, 8000)).toBe(true);
    }
    await creator.stop();

    // Plant the sidecars the way a rendering client would publish them.
    const handle = await hub.repo.find<IndexDocument>(indexDocId as DocumentId);
    handle.change((d) => {
      d.diagnostics = {
        'index.qmd': {
          contentHash: sha256(BROKEN_CONTENT),
          asOf: '2026-07-16T12:00:00.000Z',
          source: 'hub-client-preview',
          items: [
            {
              kind: 'error',
              title: 'YAML parse error',
              hints: ['close the bracket'],
              start_line: 2,
              start_column: 8,
              details: [],
            },
          ],
        },
      };
      d.captures = {
        'index.qmd': { captureDocId: 'cap-1', state: 'error', lastError: 'kernel died' },
      };
    });

    client = new McpTestClient();
    await client.start(['--server', hub.url]);
  }, 60000);

  afterAll(async () => {
    await client?.stop();
    await hub.stop();
  });

  it('reports fresh preview diagnostics and the execution error', async () => {
    const result = await client.callTool('get_errors', { project: indexDocId });
    expect(result.isError).not.toBe(true);

    const report = JSON.parse(result.content[0]!.text) as {
      files: Array<{
        path: string;
        preview?: { stale: boolean; source: string; errors: Array<{ title: string; start_line?: number }>; warnings: unknown[] };
        execution?: { state: string; lastError?: string };
      }>;
      note?: string;
    };
    expect(report.note).toBeUndefined();
    expect(report.files).toHaveLength(1);

    const entry = report.files[0]!;
    expect(entry.path).toBe('index.qmd');
    expect(entry.preview!.stale).toBe(false);
    expect(entry.preview!.source).toBe('hub-client-preview');
    expect(entry.preview!.errors).toHaveLength(1);
    expect(entry.preview!.errors[0]!.title).toBe('YAML parse error');
    expect(entry.preview!.errors[0]!.start_line).toBe(2);
    expect(entry.execution).toEqual({ state: 'error', lastError: 'kernel died' });
  }, 60000);

  it('flips to stale after the agent edits the file', async () => {
    const written = await client.callTool('write_file', {
      project: indexDocId,
      path: 'index.qmd',
      content: '---\ntitle: fixed\n---\n',
    });
    expect(written.isError).not.toBe(true);

    const result = await client.callTool('get_errors', {
      project: indexDocId,
      path: 'index.qmd',
    });
    expect(result.isError).not.toBe(true);
    const report = JSON.parse(result.content[0]!.text) as {
      files: Array<{ path: string; preview?: { stale: boolean } }>;
    };
    // The published diagnostics still describe the broken content;
    // the agent must treat them as saying nothing about its edit.
    expect(report.files[0]!.preview!.stale).toBe(true);
  }, 60000);
});
