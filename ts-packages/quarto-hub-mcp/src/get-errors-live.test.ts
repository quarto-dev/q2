/**
 * MCP-level integration test for `get_errors` (v2: local validation)
 * against the in-process test hub: the real server binary
 * (dist/index.js, which loads the real WASM host) over stdio.
 *
 * Pins the whole agent loop with zero cross-peer choreography:
 * create a broken project → get_errors reports the diagnostic for
 * exactly that content → patch_file fixes it → get_errors immediately
 * reports clean, no waiting on any other peer.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import type { DocumentId } from '@automerge/automerge-repo';

import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

const BROKEN = '---\ntitle: ok\n---\n\nHello **unclosed strong\n';
const FIXED = '---\ntitle: ok\n---\n\nHello **closed strong**\n';

describe('get_errors at the MCP tool surface (test hub, local WASM)', () => {
  let hub: TestHub;
  let client: McpTestClient;
  let indexDocId: string;

  beforeAll(async () => {
    hub = await startTestHub();
    client = new McpTestClient();
    await client.start(['--server', hub.url]);

    const created = await client.callTool('create_project', {
      files: [
        { path: 'index.qmd', content: BROKEN },
        { path: '_quarto.yml', content: 'project:\n  type: default\n' },
      ],
    });
    expect(created.isError).not.toBe(true);
    indexDocId = (JSON.parse(created.content[0]!.text) as { indexDocId: string }).indexDocId;
    expect(await hub.hubHasDoc(indexDocId as DocumentId, 8000)).toBe(true);
  }, 120000);

  afterAll(async () => {
    await client?.stop();
    await hub.stop();
  });

  it('reports the render diagnostic for the broken document', async () => {
    const result = await client.callTool('get_errors', {
      project: indexDocId,
      path: 'index.qmd',
    });
    expect(result.isError).not.toBe(true);

    const report = JSON.parse(result.content[0]!.text) as {
      files: Array<{
        path: string;
        checkedContentSha256?: string;
        errors: Array<{ title: string; start_line?: number }>;
      }>;
    };
    const entry = report.files.find((f) => f.path === 'index.qmd')!;
    expect(entry.errors).toHaveLength(1);
    expect(entry.errors[0]!.title).toBe('Unclosed Strong Star Emphasis');
    expect(entry.errors[0]!.start_line).toBe(5);
    expect(entry.checkedContentSha256).toMatch(/^sha256:[0-9a-f]{64}$/);
  }, 120000);

  it('reports clean immediately after the agent fixes the file', async () => {
    const patched = await client.callTool('patch_file', {
      project: indexDocId,
      path: 'index.qmd',
      old_string: 'Hello **unclosed strong',
      new_string: 'Hello **closed strong**',
    });
    expect(patched.isError).not.toBe(true);

    const result = await client.callTool('get_errors', {
      project: indexDocId,
      path: 'index.qmd',
    });
    expect(result.isError).not.toBe(true);
    const report = JSON.parse(result.content[0]!.text) as {
      files: Array<{ path: string; errors: unknown[]; warnings: unknown[] }>;
    };
    const entry = report.files.find((f) => f.path === 'index.qmd')!;
    expect(entry.errors).toEqual([]);
    // Sanity: the fixed content really is what we think it is.
    const read = await client.callTool('read_file', { project: indexDocId, path: 'index.qmd' });
    expect(read.content[0]!.text).toBe(FIXED);
  }, 120000);
});
