/**
 * Exit-drain at the stdio level (bd-10deu8h4) — the exact 2026-06-12
 * accident, replayed against the real server binary.
 *
 * The incident: a `q2 mcp` session ran `create_project`, read the file
 * back from process memory, and the host closed stdin. The stdin-EOF
 * shutdown (bd-9jq2a060, correct behavior) ran `disconnectAll()` and
 * exited before the new file document's sync to the hub completed. The
 * index entry escaped; the file document's only copy died with the
 * process (MCP clients use memory storage) — a dangling index entry
 * that bricked the project for every client.
 *
 * Contract under test: shutdown drains outbound sync (bounded) before
 * `process.exit`, while preserving the prompt stdin-EOF exit that
 * stdio-hygiene.test.ts asserts. Ground truth is the hub's own repo.
 */

import { describe, it, expect, beforeAll, afterAll, afterEach } from 'vitest';

import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

// Enough payload to keep delivery genuinely in flight when stdin
// closes right after the tool result — a single tiny file wins the
// race by luck on loopback, masking the defect. Both levers matter:
// many docs (per-doc synchronizer backlog) and real bytes (encode +
// send time).
const FILE_COUNT = 64;
const FILE_BYTES = 64 * 1024;

function accidentFiles() {
  return Array.from({ length: FILE_COUNT }, (_, i) => ({
    path: `q2-mcp-hello-${i}.qmd`,
    content: `file ${i}\n${'x'.repeat(FILE_BYTES)}\n`,
  }));
}

interface CreateProjectResponse {
  indexDocId: string;
  files: Array<{ path: string; docId: string }>;
}

describe('exit drains outbound sync (bd-10deu8h4)', () => {
  let hub: TestHub;
  let client: McpTestClient;

  beforeAll(async () => {
    hub = await startTestHub();
  });

  afterAll(async () => {
    await hub.stop();
  });

  afterEach(async () => {
    await client.stop();
  });

  it('create_project then immediate stdin EOF must not lose the created docs', async () => {
    client = new McpTestClient();
    await client.start(['--server', hub.url]);

    const result = await client.callTool('create_project', {
      files: accidentFiles(),
    });
    const created = JSON.parse(result.content[0]!.text) as CreateProjectResponse;
    expect(created.files).toHaveLength(FILE_COUNT);

    // The exact accident: the host closes stdin right after the tool
    // call returns. The server must still exit promptly (bd-9jq2a060)…
    expect(await client.endStdinAndWaitForExit(5000)).toBe(true);

    // …but not before the created documents reached the hub.
    expect(
      await hub.hubHasDoc(created.indexDocId),
      'index doc must reach the hub',
    ).toBe(true);
    for (const f of created.files) {
      expect(
        await hub.hubHasDoc(f.docId),
        `file doc for ${f.path} must reach the hub — its only copy was in-process`,
      ).toBe(true);
    }
  }, 30000);
});
