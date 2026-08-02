/**
 * getProjectSetDebugSnapshot / buildProjectSetDebugSnapshot — read-only
 * introspection over the project-set service's server + collection
 * connections, consumed by the in-context debug API `quartoDebug.am`
 * (bd-q93tkglb; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * The pure builder is tested against fabricated structural inputs (fake
 * repos and handles); the public accessor adapts the module's live
 * `servers`/`connections` maps and is asserted on the disconnected
 * empty state (real connections need a live sync server — covered by
 * the Phase 1 end-to-end pass instead).
 */

import { describe, it, expect } from 'vitest';
import {
  buildProjectSetDebugSnapshot,
  getProjectSetDebugSnapshot,
  getCollectionHandle,
  type DebugRepoLike,
  type DebugCollectionConnectionLike,
} from './projectSetService';

function fakeRepo(peerId: string, peers: string[]): DebugRepoLike {
  return { peerId, peers };
}

function fakeConnection(
  docId: string,
  syncServer: string,
  handleState: string,
  doc: unknown,
  heads: string[] | null,
): DebugCollectionConnectionLike {
  return {
    docId,
    syncServer,
    handle: {
      state: handleState,
      heads: () => {
        if (heads === null) throw new Error('not ready');
        return heads;
      },
      doc: () => doc,
    },
  };
}

describe('buildProjectSetDebugSnapshot', () => {
  it('maps servers and collections into JSON-serializable form', () => {
    const servers = new Map([
      [
        'wss://hub.example/ws',
        { repo: fakeRepo('peer-self', ['peer-hub']), refCount: 2 },
      ],
    ]);
    const connections = new Map([
      [
        'doc-root',
        fakeConnection(
          'doc-root',
          'wss://hub.example/ws',
          'ready',
          {
            name: 'Personal',
            version: 1,
            projects: { p1: {}, p2: {} },
          },
          ['headA', 'headB'],
        ),
      ],
      [
        'doc-team',
        fakeConnection(
          'doc-team',
          'wss://hub.example/ws',
          'ready',
          { name: 'Team', version: 1, projects: { p3: {} } },
          ['headC'],
        ),
      ],
    ]);

    const snap = buildProjectSetDebugSnapshot(servers, connections, 'doc-root');

    expect(snap).toEqual({
      servers: [
        {
          url: 'wss://hub.example/ws',
          peerId: 'peer-self',
          connectedPeers: ['peer-hub'],
          refCount: 2,
        },
      ],
      collections: [
        {
          docId: 'doc-root',
          syncServer: 'wss://hub.example/ws',
          name: 'Personal',
          isRoot: true,
          entryCount: 2,
          handleState: 'ready',
          heads: ['headA', 'headB'],
        },
        {
          docId: 'doc-team',
          syncServer: 'wss://hub.example/ws',
          name: 'Team',
          isRoot: false,
          entryCount: 1,
          handleState: 'ready',
          heads: ['headC'],
        },
      ],
    });
  });

  it('reports null heads and zero entries for a not-ready handle', () => {
    const connections = new Map([
      [
        'doc-loading',
        fakeConnection('doc-loading', 'wss://hub.example/ws', 'loading', undefined, null),
      ],
    ]);

    const snap = buildProjectSetDebugSnapshot(new Map(), connections, null);

    expect(snap.collections).toEqual([
      {
        docId: 'doc-loading',
        syncServer: 'wss://hub.example/ws',
        name: undefined,
        isRoot: false,
        entryCount: 0,
        handleState: 'loading',
        heads: null,
      },
    ]);
  });
});

describe('getProjectSetDebugSnapshot', () => {
  it('is empty when nothing is connected', () => {
    expect(getProjectSetDebugSnapshot()).toEqual({
      servers: [],
      collections: [],
    });
  });
});

describe('getCollectionHandle', () => {
  // The positive case needs a live connection (real WebSocket + sync
  // server) and is covered by the Phase 1 end-to-end pass.
  it('returns null for an unknown collection doc id', () => {
    expect(getCollectionHandle('not-connected')).toBeNull();
  });
});
