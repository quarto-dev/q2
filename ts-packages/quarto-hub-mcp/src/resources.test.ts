/**
 * MCP resources surface (BP-5, Phase 5): project files as first-class,
 * subscribable MCP resources under the `hub://` URI contract (Q-2).
 *
 * The URI contract (stability matters — hosts cache and attach these):
 *
 *     hub://project/<indexDocId>/<path>[?server=<url-encoded ws url>]
 *
 * The index doc id rides in the PATH, not the host: RFC 3986 host
 * normalizers lowercase, and automerge doc ids are case-sensitive. The
 * literal `project` segment leaves room for other resource kinds
 * (`hub://collection/…`) without a breaking change. A `server=` query
 * parameter mirrors the share-URL grammar for projects a share URL
 * routed to a foreign hub (bd-qt7h8h5g).
 *
 * Covered here (legacy era — the in-memory fixture is a bare 2025-era
 * pair; the modern `subscriptions/listen` path has its own stdio suite,
 * resources-modern.test.ts):
 *
 * - `resources/list` enumerates every connected project's files,
 *   paginated (`nextCursor`) with deterministic order;
 * - `resources/read` returns text contents, and base64 blob contents
 *   for binaries;
 * - `resources/templates/list` advertises the hub:// template;
 * - a legacy client's `resources/subscribe` receives
 *   `notifications/resources/updated` when the file changes (and only
 *   then); file adds/removes emit `notifications/resources/list_changed`;
 * - write-tool results carry a `resource_link` block to the file's URI.
 */

import { describe, it, expect, vi } from 'vitest';
import type { CallToolResult } from '@modelcontextprotocol/client';
import {
  createSyncClient,
  type SyncClient,
} from '@quarto/quarto-sync-client';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';
import {
  buildFileResourceUri,
  parseFileResourceUri,
  RESOURCE_PAGE_SIZE,
} from './resources.js';

// ---------------------------------------------------------------------------
// URI contract unit tests (pure — no fixture)
// ---------------------------------------------------------------------------

describe('hub:// resource URI contract (Q-2)', () => {
  it('round-trips project + path, preserving case and special characters', () => {
    const uri = buildFileResourceUri('AbC123XyZ', 'dir/My File.qmd');
    expect(uri).toBe('hub://project/AbC123XyZ/dir/My%20File.qmd');
    expect(parseFileResourceUri(uri)).toEqual({
      project: 'AbC123XyZ',
      path: 'dir/My File.qmd',
      server: undefined,
    });
  });

  it('carries a foreign hub as ?server=, mirroring the share-URL grammar', () => {
    const uri = buildFileResourceUri('docId9', 'a.qmd', 'wss://other.example/ws');
    expect(uri).toBe(
      `hub://project/docId9/a.qmd?server=${encodeURIComponent('wss://other.example/ws')}`,
    );
    expect(parseFileResourceUri(uri)).toEqual({
      project: 'docId9',
      path: 'a.qmd',
      server: 'wss://other.example/ws',
    });
  });

  it('rejects URIs outside the contract', () => {
    expect(parseFileResourceUri('https://quarto-hub.com/#/share/abc')).toBeNull();
    expect(parseFileResourceUri('hub://collection/abc')).toBeNull();
    expect(parseFileResourceUri('hub://project/')).toBeNull();
    expect(parseFileResourceUri('hub://project/abc')).toBeNull();
    expect(parseFileResourceUri('not a uri')).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Fixture-level resources tests (legacy era)
// ---------------------------------------------------------------------------

interface ListPage {
  resources: Array<{ uri: string; name: string; mimeType?: string; size?: number }>;
  nextCursor?: string;
}

async function listPage(
  f: InMemoryMcpFixture,
  cursor?: string,
): Promise<ListPage> {
  // The per-page path (explicit cursor key) returns the server's raw
  // page; the aggregate path would hide pagination. client.request is
  // the raw verb either way.
  const result = await f.client.request(
    { method: 'resources/list', params: cursor === undefined ? {} : { cursor } },
    undefined,
  );
  return result as ListPage;
}

function textContent(result: {
  contents: Array<{ uri: string; text?: string; blob?: string; mimeType?: string }>;
}): { uri: string; text: string; mimeType?: string } {
  const c = result.contents[0];
  if (!c || c.text === undefined) {
    throw new Error(`expected text resource contents, got ${JSON.stringify(result)}`);
  }
  return c as { uri: string; text: string; mimeType?: string };
}

describe('resources/list + resources/read (BP-5)', () => {
  it('enumerates a connected project\u2019s files as hub:// resources', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'a.qmd', content: 'hello\n' },
        { path: 'dir/b.qmd', content: 'world\n' },
      ]);
      const page = await listPage(f);
      const byName = new Map(page.resources.map((r) => [r.name, r]));
      expect(byName.size).toBe(2);
      const a = byName.get('a.qmd');
      expect(a?.uri).toBe(`hub://project/${seed.indexDocId}/a.qmd`);
      expect(a?.mimeType).toBe('text/markdown');
      expect(a?.size).toBe(6);
      expect(byName.get('dir/b.qmd')?.uri).toBe(
        `hub://project/${seed.indexDocId}/dir/b.qmd`,
      );
      expect(page.nextCursor).toBeUndefined();
    } finally {
      await f.close();
    }
  });

  it('paginates a 500-file project with nextCursor, in deterministic order', async () => {
    const f = await startInMemoryMcp();
    try {
      const files = Array.from({ length: 500 }, (_, i) => ({
        path: `f${String(i).padStart(4, '0')}.qmd`,
        content: `file ${i}\n`,
      }));
      const seed = await seedProject(f, files);

      // Walk the raw pages: every page but the last carries nextCursor,
      // pages are full, and the union is exactly the seeded set.
      const seen = new Map<string, string>();
      let cursor: string | undefined;
      let pages = 0;
      for (;;) {
        const page: ListPage = await listPage(f, cursor);
        pages++;
        for (const r of page.resources) seen.set(r.name, r.uri);
        if (page.nextCursor === undefined) break;
        cursor = page.nextCursor;
        expect(pages, 'pagination never terminates').toBeLessThan(20);
      }
      expect(pages).toBe(Math.ceil(500 / RESOURCE_PAGE_SIZE));
      expect(seen.size).toBe(500);
      expect(seen.get('f0000.qmd')).toBe(`hub://project/${seed.indexDocId}/f0000.qmd`);

      // Order is deterministic: the first raw page is the same on a
      // second call, and names arrive sorted.
      const first = await listPage(f);
      expect(first.resources).toHaveLength(RESOURCE_PAGE_SIZE);
      const names = first.resources.map((r) => r.name);
      expect([...names].sort()).toEqual(names);

      // The SDK client's aggregate path walks every page for us.
      const aggregate = await f.client.listResources();
      expect(aggregate.resources).toHaveLength(500);
    } finally {
      await f.close();
    }
  }, 120000);

  it('lists the files of every connected project, disambiguated by URI', async () => {
    const f = await startInMemoryMcp();
    try {
      const one = await seedProject(f, [{ path: 'one.qmd', content: '1\n' }]);
      const two = await seedProject(f, [{ path: 'two.qmd', content: '2\n' }]);
      const page = await listPage(f);
      const uris = page.resources.map((r) => r.uri).sort();
      expect(uris).toEqual([
        `hub://project/${one.indexDocId}/one.qmd`,
        `hub://project/${two.indexDocId}/two.qmd`,
      ].sort());
    } finally {
      await f.close();
    }
  });

  it('reads a text file as text resource contents', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'hello world\n' }]);
      const result = await f.client.readResource({
        uri: `hub://project/${seed.indexDocId}/a.qmd`,
      });
      const c = textContent(result);
      expect(c.text).toBe('hello world\n');
      expect(c.mimeType).toBe('text/markdown');
      expect(c.uri).toBe(`hub://project/${seed.indexDocId}/a.qmd`);
    } finally {
      await f.close();
    }
  });

  it('reads a binary file as base64 blob contents', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'img.png',
        content: png.toString('base64'),
        encoding: 'base64',
      });
      const result = await f.client.readResource({
        uri: `hub://project/${seed.indexDocId}/img.png`,
      });
      const c = result.contents[0];
      expect(c && 'blob' in c && c.blob).toBe(png.toString('base64'));
      expect(c?.mimeType).toBe('image/png');
      expect(c && 'text' in c).toBe(false);
    } finally {
      await f.close();
    }
  });

  it('rejects reads of unknown or malformed URIs with a protocol error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      await expect(
        f.client.readResource({ uri: `hub://project/${seed.indexDocId}/nope.qmd` }),
      ).rejects.toThrow(/nope\.qmd/);
      await expect(
        f.client.readResource({ uri: 'https://example.com/not-ours' }),
      ).rejects.toThrow();
    } finally {
      await f.close();
    }
  });

  it('advertises the hub:// resource template', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await f.client.listResourceTemplates();
      const t = result.resourceTemplates.find((t) => t.name === 'project-file');
      expect(t).toBeDefined();
      expect(t?.uriTemplate).toBe('hub://project/{indexDocId}/{path}');
    } finally {
      await f.close();
    }
  });
});

describe('resource subscriptions, legacy era (BP-5)', () => {
  /** A second sync client playing the human collaborator. */
  async function startCollaborator(
    hubUrl: string,
    indexDocId: string,
  ): Promise<SyncClient> {
    const client = createSyncClient({
      onFileAdded() {},
      onFileChanged() {},
      onBinaryChanged() {},
      onFileRemoved() {},
    });
    await client.connect(hubUrl, indexDocId, undefined, undefined, undefined, {
      requireOnline: true,
      peerTimeoutMs: 8000,
    });
    return client;
  }

  it('delivers resources/updated for a subscribed file only', async () => {
    const f = await startInMemoryMcp();
    let collab: SyncClient | undefined;
    try {
      const seed = await seedProject(f, [
        { path: 'a.qmd', content: 'v1\n' },
        { path: 'b.qmd', content: 'v1\n' },
      ]);
      const aUri = `hub://project/${seed.indexDocId}/a.qmd`;
      const bUri = `hub://project/${seed.indexDocId}/b.qmd`;

      const updated: string[] = [];
      f.client.setNotificationHandler('notifications/resources/updated', (n) => {
        updated.push(n.params.uri);
      });
      await f.client.subscribeResource({ uri: aUri });

      collab = await startCollaborator(f.hub.url, seed.indexDocId);
      collab.updateFileContent('a.qmd', 'v2 subscribed\n');
      collab.updateFileContent('b.qmd', 'v2 not subscribed\n');

      await vi.waitFor(
        () => {
          expect(updated).toContain(aUri);
        },
        { timeout: 10000, interval: 25 },
      );
      // The subscribed file's notification arrived, proving the pipe
      // works; the unsubscribed file's must never arrive.
      await new Promise((r) => setTimeout(r, 500));
      expect(updated).not.toContain(bUri);

      // After unsubscribe, further edits to a.qmd are silent too.
      await f.client.unsubscribeResource({ uri: aUri });
      const before = updated.length;
      collab.updateFileContent('a.qmd', 'v3 after unsubscribe\n');
      await new Promise((r) => setTimeout(r, 750));
      expect(updated.length).toBe(before);
    } finally {
      await collab?.disconnect();
      await f.close();
    }
  }, 30000);

  it('emits resources/list_changed when files are added or removed', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const listChanged: number[] = [];
      f.client.setNotificationHandler('notifications/resources/list_changed', () => {
        listChanged.push(Date.now());
      });

      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'new.qmd',
        content: 'added\n',
      });
      await vi.waitFor(
        () => {
          expect(listChanged.length).toBeGreaterThanOrEqual(1);
        },
        { timeout: 10000, interval: 25 },
      );

      await callTool(f, 'delete_file', {
        project: seed.indexDocId,
        path: 'new.qmd',
      });
      await vi.waitFor(
        () => {
          expect(listChanged.length).toBeGreaterThanOrEqual(2);
        },
        { timeout: 10000, interval: 25 },
      );
    } finally {
      await f.close();
    }
  }, 30000);

  it('a debounced list_changed firing after server close does not reject unhandled', async () => {
    // The bridge's notifications are best-effort fan-out: closing the
    // fixture emits `disconnected` events whose debounced
    // sendResourceListChanged lands AFTER the transport is gone. That
    // close-race must be swallowed, not crash the process as an
    // unhandled rejection (the 82-error verify failure that prompted
    // this test — vitest fails the run on unhandled errors even when
    // every assertion passes).
    const f = await startInMemoryMcp();
    const rejections: unknown[] = [];
    const onRejection = (reason: unknown): void => {
      rejections.push(reason);
    };
    process.on('unhandledRejection', onRejection);
    try {
      await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      await f.close();
      // Settle past the debounce window (150 ms) with margin.
      await new Promise((r) => setTimeout(r, 500));
      expect(rejections).toEqual([]);
    } finally {
      process.off('unhandledRejection', onRejection);
    }
  }, 30000);
});

describe('resource_link blocks in write results (BP-5)', () => {
  function resourceLinks(result: CallToolResult): Array<{ uri: string; name?: string }> {
    return result.content
      .filter((b) => b.type === 'resource_link')
      .map((b) => ({ uri: (b as { uri: string }).uri, name: (b as { name?: string }).name }));
  }

  it('write_file and patch_file results link the file\u2019s hub:// URI', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const uri = `hub://project/${seed.indexDocId}/a.qmd`;

      const written = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2\n',
      });
      expect(resourceLinks(written)).toEqual([{ uri, name: 'a.qmd' }]);

      const patched = await callTool(f, 'patch_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        old_string: 'v2',
        new_string: 'v3',
      });
      expect(resourceLinks(patched)).toEqual([{ uri, name: 'a.qmd' }]);
    } finally {
      await f.close();
    }
  });

  it('create_file and rename_file results link the (new) file URI', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const created = await callTool(f, 'create_file', {
        project: seed.indexDocId,
        path: 'new.qmd',
        content: 'n\n',
      });
      expect(resourceLinks(created)).toEqual([
        { uri: `hub://project/${seed.indexDocId}/new.qmd`, name: 'new.qmd' },
      ]);

      const renamed = await callTool(f, 'rename_file', {
        project: seed.indexDocId,
        old_path: 'new.qmd',
        new_path: 'renamed.qmd',
      });
      expect(resourceLinks(renamed)).toEqual([
        { uri: `hub://project/${seed.indexDocId}/renamed.qmd`, name: 'renamed.qmd' },
      ]);
    } finally {
      await f.close();
    }
  });
});
