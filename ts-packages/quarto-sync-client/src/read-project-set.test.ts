/**
 * `readProjectSetDoc` — one-shot read of a ProjectSetDocument by doc id
 * (quarto-hub-mcp CAP-3 `list_projects` MVP). Stands up a temporary
 * repo + adapter against the sync server, fetches the document, and
 * tears down — no SyncClient needed, because a project-set document is
 * not a project index.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { generateAutomergeUrl } from '@automerge/automerge-repo';

import type { ProjectSetDocument } from '@quarto/quarto-automerge-schema';

import { readProjectSetDoc } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  await hub.stop();
});

/** Mint a project-set document on the hub (server-side ground truth). */
function seedProjectSet(doc: ProjectSetDocument): string {
  const handle = hub.repo.create<ProjectSetDocument>();
  handle.change((d) => {
    d.version = doc.version;
    d.projects = doc.projects;
    if (doc.name !== undefined) d.name = doc.name;
  });
  return handle.documentId;
}

const ENTRY = {
  indexDocId: 'automerge:proj1',
  syncServer: 'wss://quarto-hub.com/ws',
  description: 'My book',
  addedAt: '2026-10-01T00:00:00.000Z',
  lastAccessed: '2026-10-05T00:00:00.000Z',
};

describe('readProjectSetDoc', () => {
  it('reads a project-set document by bare doc id', async () => {
    const docId = seedProjectSet({ version: 1, name: 'Charlie’s projects', projects: { proj1: ENTRY } });
    const doc = await readProjectSetDoc({ serverUrl: hub.url, docId });
    expect(doc.version).toBe(1);
    expect(doc.name).toBe('Charlie’s projects');
    expect(Object.keys(doc.projects)).toEqual(['proj1']);
    expect(doc.projects.proj1.description).toBe('My book');
  });

  it('accepts an automerge:-prefixed id', async () => {
    const docId = seedProjectSet({ version: 1, projects: { proj1: ENTRY } });
    const doc = await readProjectSetDoc({ serverUrl: hub.url, docId: `automerge:${docId}` });
    expect(Object.keys(doc.projects)).toEqual(['proj1']);
  });

  it('rejects a document that is not a project set', async () => {
    const handle = hub.repo.create<{ text: string }>();
    handle.change((d) => {
      d.text = 'definitely not a project set';
    });
    await expect(
      readProjectSetDoc({ serverUrl: hub.url, docId: handle.documentId }),
    ).rejects.toThrow(/not a project-set/i);
  });

  it('rejects an unavailable document', async () => {
    // A freshly minted (therefore never-served) but valid document id.
    await expect(
      readProjectSetDoc({
        serverUrl: hub.url,
        docId: generateAutomergeUrl(),
        peerTimeoutMs: 8000,
      }),
    ).rejects.toThrow(/unavailable|not served/i);
  }, 20000);
});
