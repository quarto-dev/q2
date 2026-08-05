/**
 * Tests for the project-list export/import format helpers.
 *
 * The export is the user-facing "Export project list (JSON)" file. Version 5
 * adds `collections` — each collection is a synced ProjectSetDocument, so the
 * export records the *pointers* (docId + syncServer, plus display-only name
 * and member ids); import re-subscribes rather than re-creating anything.
 *
 * Parse accepts every historical shape: v5, the flat v4 object, and the
 * pre-ExportData bare array. See
 * claude-notes/plans/2026-08-04-collections-export.md.
 */

import { describe, it, expect } from 'vitest';
import {
  buildProjectListExport,
  parseProjectListImport,
  PROJECT_LIST_EXPORT_VERSION,
} from './projectListExport';

const projects = [
  {
    indexDocId: 'automerge:abc',
    syncServer: 'wss://s',
    description: 'Alpha',
    addedAt: '2026-07-01T00:00:00.000Z',
    lastAccessed: '2026-08-01T00:00:00.000Z',
  },
  {
    indexDocId: 'automerge:def',
    syncServer: 'wss://s',
    description: 'Beta',
    addedAt: '2026-07-02T00:00:00.000Z',
    lastAccessed: '2026-08-02T00:00:00.000Z',
  },
];

const collections = [
  {
    docId: 'root-doc',
    syncServer: 'wss://s',
    name: 'My projects',
    isRoot: true,
    entries: [{ indexDocId: 'automerge:abc' }, { indexDocId: 'automerge:def' }],
  },
  {
    docId: 'team-doc',
    syncServer: 'wss://s',
    name: 'Team docs',
    isRoot: false,
    entries: [{ indexDocId: 'automerge:abc' }],
  },
];

describe('buildProjectListExport', () => {
  it('produces a v5 export with projects and collection pointers', () => {
    const parsed = JSON.parse(buildProjectListExport(projects, collections));

    expect(parsed.schemaVersion).toBe(PROJECT_LIST_EXPORT_VERSION);
    expect(Date.parse(parsed.exportedAt)).not.toBeNaN();
    expect(parsed.projects).toHaveLength(2);
    expect(parsed.projects[0]).toMatchObject({
      indexDocId: 'automerge:abc',
      syncServer: 'wss://s',
      description: 'Alpha',
      createdAt: '2026-07-01T00:00:00.000Z',
      lastAccessed: '2026-08-01T00:00:00.000Z',
    });
    expect(parsed.collections).toEqual([
      {
        projectSetDocId: 'root-doc',
        syncServer: 'wss://s',
        name: 'My projects',
        isRoot: true,
        projectIds: ['automerge:abc', 'automerge:def'],
      },
      {
        projectSetDocId: 'team-doc',
        syncServer: 'wss://s',
        name: 'Team docs',
        isRoot: false,
        projectIds: ['automerge:abc'],
      },
    ]);
  });

  it('emits an empty collections array (not absent) when none are connected', () => {
    const parsed = JSON.parse(buildProjectListExport(projects, []));
    expect(parsed.schemaVersion).toBe(PROJECT_LIST_EXPORT_VERSION);
    expect(parsed.collections).toEqual([]);
  });

  it('omits a missing collection name rather than inventing one', () => {
    const parsed = JSON.parse(
      buildProjectListExport([], [{ docId: 'd', syncServer: 'wss://s', isRoot: false, entries: [] }]),
    );
    expect(parsed.collections[0].name).toBeUndefined();
    expect(parsed.collections[0].projectIds).toEqual([]);
  });
});

describe('parseProjectListImport', () => {
  it('round-trips a v5 export', () => {
    const json = buildProjectListExport(projects, collections);
    const parsed = parseProjectListImport(json);

    expect(parsed.projects.map((p) => p.indexDocId)).toEqual(['automerge:abc', 'automerge:def']);
    expect(parsed.collections.map((c) => c.projectSetDocId)).toEqual(['root-doc', 'team-doc']);
    expect(parsed.collections[0].isRoot).toBe(true);
  });

  it('accepts a v4 flat export (no collections field)', () => {
    const v4 = JSON.stringify({
      schemaVersion: 4,
      exportedAt: '2026-08-01T00:00:00.000Z',
      projects: [
        { id: '', indexDocId: 'automerge:abc', syncServer: 'wss://s', description: 'Alpha', createdAt: '2026-07-01T00:00:00.000Z', lastAccessed: '2026-08-01T00:00:00.000Z' },
      ],
    });
    const parsed = parseProjectListImport(v4);
    expect(parsed.projects).toHaveLength(1);
    expect(parsed.collections).toEqual([]);
  });

  it('accepts the pre-ExportData bare array format', () => {
    const legacy = JSON.stringify([
      { id: 'x', indexDocId: 'automerge:abc', syncServer: 'wss://s', description: 'Alpha', createdAt: '2026-07-01T00:00:00.000Z', lastAccessed: '2026-08-01T00:00:00.000Z' },
    ]);
    const parsed = parseProjectListImport(legacy);
    expect(parsed.projects).toHaveLength(1);
    expect(parsed.collections).toEqual([]);
  });

  it('rejects an export from a newer schema version', () => {
    const future = JSON.stringify({ schemaVersion: PROJECT_LIST_EXPORT_VERSION + 1, exportedAt: '', projects: [] });
    expect(() => parseProjectListImport(future)).toThrow(/newer/i);
  });

  it('rejects malformed JSON with a clear error', () => {
    expect(() => parseProjectListImport('not json')).toThrow();
  });

  it('rejects a shape that is neither an array nor an ExportData object', () => {
    expect(() => parseProjectListImport('{"nope": true}')).toThrow(/invalid import format/i);
  });

  it('tolerates malformed collection entries by skipping them', () => {
    // A hand-edited file must not take down the whole import: entries missing
    // a docId or syncServer are dropped, valid ones survive.
    const json = JSON.stringify({
      schemaVersion: 5,
      exportedAt: '2026-08-01T00:00:00.000Z',
      projects: [],
      collections: [
        { projectSetDocId: 'good', syncServer: 'wss://s' },
        { projectSetDocId: '', syncServer: 'wss://s' },
        { syncServer: 'wss://s' },
        { projectSetDocId: 'no-server' },
        'garbage',
      ],
    });
    const parsed = parseProjectListImport(json);
    expect(parsed.collections.map((c) => c.projectSetDocId)).toEqual(['good']);
  });
});
