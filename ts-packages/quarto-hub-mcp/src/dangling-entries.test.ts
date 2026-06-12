/**
 * MCP-level graceful degradation for dangling index entries
 * (bd-vm5e5u10).
 *
 * Production evidence (2026-06-12): one index entry pointing at a
 * document that never reached the hub made `connect_project` fail for
 * the whole Demo Playground. Required behavior at the tool surface:
 *
 *  - `connect_project` / `list_files` succeed and list the dangling
 *    file with `"status": "unavailable"`;
 *  - `read_file` of the dangling file is a clear per-file error
 *    naming the path; the project stays connected and other files
 *    keep working;
 *  - `delete_file` of the dangling entry works (it only edits the
 *    index) — the self-service repair that the 2026-06-12 incident
 *    needed manual surgery for.
 *
 * Drives the real server binary (dist/index.js) over stdio against
 * the in-process test hub; the ghost entry is minted by mutating the
 * project index through the hub's own repo handle.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import {
  generateAutomergeUrl,
  parseAutomergeUrl,
  type DocumentId,
} from '@automerge/automerge-repo';

import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

describe('dangling index entries at the MCP tool surface', () => {
  let hub: TestHub;
  let client: McpTestClient;
  let indexDocId: string;
  let ghostId: string;

  beforeAll(async () => {
    hub = await startTestHub();

    // Create the project through one MCP server instance...
    const creator = new McpTestClient();
    await creator.start(['--server', hub.url]);
    const created = await creator.callTool('create_project', {
      files: [
        { path: 'real-one.qmd', content: 'first real file\n' },
        { path: 'real-two.qmd', content: 'second real file\n' },
      ],
    });
    expect(created.isError).not.toBe(true);
    const parsed = JSON.parse(created.content[0]!.text) as {
      indexDocId: string;
      files: Array<{ path: string; docId: string }>;
    };
    indexDocId = parsed.indexDocId;

    // ...wait until the hub actually holds every document (creation
    // syncs in the background), then retire the creator.
    expect(await hub.hubHasDoc(indexDocId, 8000)).toBe(true);
    for (const f of parsed.files) {
      expect(await hub.hubHasDoc(f.docId, 8000)).toBe(true);
    }
    await creator.stop();

    // Mint the dangling entry: a valid-format doc id no repo holds.
    ghostId = parseAutomergeUrl(generateAutomergeUrl()).documentId;
    const handle = await hub.repo.find<{ files: Record<string, string> }>(
      indexDocId as DocumentId,
    );
    handle.change((d) => {
      d.files['ghost.qmd'] = ghostId;
    });

    // A fresh server instance does the cold connect (the incident path).
    client = new McpTestClient();
    await client.start(['--server', hub.url]);
  }, 60000);

  afterAll(async () => {
    await client?.stop();
    await hub.stop();
  });

  it('connect_project succeeds and marks the ghost unavailable', async () => {
    // RED today: the tool returns `Error in connect_project:
    // Document automerge:… is unavailable`.
    const result = await client.callTool('connect_project', { project: indexDocId });
    expect(result.isError).not.toBe(true);

    const payload = JSON.parse(result.content[0]!.text) as {
      project: string;
      files: Array<{ path: string; type?: string; status?: string; docId?: string }>;
    };
    expect(payload.files.map((f) => f.path).sort()).toEqual([
      'ghost.qmd',
      'real-one.qmd',
      'real-two.qmd',
    ]);

    const ghost = payload.files.find((f) => f.path === 'ghost.qmd')!;
    expect(ghost.status).toBe('unavailable');
    expect(ghost.docId).toBe(ghostId);

    for (const real of ['real-one.qmd', 'real-two.qmd']) {
      const entry = payload.files.find((f) => f.path === real)!;
      expect(entry.type).toBe('text');
      expect(entry.status).not.toBe('unavailable');
    }
  }, 60000);

  it('read_file of the ghost errors naming the path; real files still read', async () => {
    const ghostRead = await client.callTool('read_file', {
      project: indexDocId,
      path: 'ghost.qmd',
    });
    expect(ghostRead.isError).toBe(true);
    expect(ghostRead.content[0]!.text).toContain("'ghost.qmd'");
    expect(ghostRead.content[0]!.text).toContain('unavailable');

    // The project stays connected and usable in the same session.
    const realRead = await client.callTool('read_file', {
      project: indexDocId,
      path: 'real-one.qmd',
    });
    expect(realRead.isError).not.toBe(true);
    expect(realRead.content[0]!.text).toBe('first real file\n');
  }, 60000);

  it('delete_file of the ghost succeeds and removes the index entry', async () => {
    const deleted = await client.callTool('delete_file', {
      project: indexDocId,
      path: 'ghost.qmd',
    });
    expect(deleted.isError).not.toBe(true);
    expect(deleted.content[0]!.text).toContain('ghost.qmd');

    const listed = await client.callTool('list_files', { project: indexDocId });
    expect(listed.isError).not.toBe(true);
    const files = JSON.parse(listed.content[0]!.text) as Array<{ path: string }>;
    expect(files.map((f) => f.path).sort()).toEqual(['real-one.qmd', 'real-two.qmd']);
  }, 60000);
});
