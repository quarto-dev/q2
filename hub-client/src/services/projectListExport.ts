/**
 * Project-list export/import format (the "Export project list (JSON)" file).
 *
 * Version 5 adds `collections`. Each collection is a synced Automerge
 * ProjectSetDocument, so the export records *pointers* — docId + syncServer —
 * plus display-only metadata (`name`, `projectIds`) for human readability.
 * Import re-subscribes to the pointers; it never recreates collection
 * documents or writes membership (the synced doc is the source of truth, and
 * membership arrives with sync).
 *
 * Parse accepts every historical shape: v5, the flat v4 ExportData object,
 * and the pre-ExportData bare array of projects.
 *
 * See claude-notes/plans/2026-08-04-collections-export.md.
 */

import type { ProjectEntryV2 } from './storage/types';

/** Version stamped on new exports. Independent of the IDB migration version. */
export const PROJECT_LIST_EXPORT_VERSION = 5;

/** A collection pointer in the export file. */
export interface ExportedCollection {
  /** Automerge document id of the collection's ProjectSetDocument. */
  projectSetDocId: string;
  /** Sync server the collection lives on. */
  syncServer: string;
  /** Display only — the synced document is authoritative. */
  name?: string;
  /** True for the personal root superset. Import never re-subscribes the root. */
  isRoot?: boolean;
  /** Member indexDocIds at export time. Display/debug only; import ignores it. */
  projectIds?: string[];
}

/** Narrow input shapes so callers aren't coupled to component/service types. */
export interface ExportableProject {
  indexDocId: string;
  syncServer: string;
  description: string;
  addedAt: string;
  lastAccessed: string;
}

export interface ExportableCollection {
  docId: string;
  syncServer: string;
  name?: string;
  isRoot: boolean;
  entries: ReadonlyArray<{ indexDocId: string }>;
}

export interface ParsedProjectListImport {
  projects: ProjectEntryV2[];
  collections: ExportedCollection[];
}

/** Build the v5 export JSON string. */
export function buildProjectListExport(
  projects: ReadonlyArray<ExportableProject>,
  collections: ReadonlyArray<ExportableCollection>,
): string {
  return JSON.stringify(
    {
      schemaVersion: PROJECT_LIST_EXPORT_VERSION,
      exportedAt: new Date().toISOString(),
      projects: projects.map((p) => ({
        id: '',
        indexDocId: p.indexDocId,
        syncServer: p.syncServer,
        description: p.description,
        createdAt: p.addedAt,
        lastAccessed: p.lastAccessed,
      })),
      collections: collections.map((c) => ({
        projectSetDocId: c.docId,
        syncServer: c.syncServer,
        ...(c.name !== undefined ? { name: c.name } : {}),
        isRoot: c.isRoot,
        projectIds: c.entries.map((e) => e.indexDocId),
      })),
    },
    null,
    2,
  );
}

/**
 * Parse an export file of any historical shape.
 *
 * @throws on malformed JSON, an unrecognized shape, or a schemaVersion newer
 *   than this build understands (mirrors `importData`'s forward-compat guard,
 *   but hard — silently dropping unknown future data would be worse than
 *   asking the user to update).
 */
export function parseProjectListImport(json: string): ParsedProjectListImport {
  const parsed: unknown = JSON.parse(json);

  if (Array.isArray(parsed)) {
    // Pre-ExportData format: bare array of projects.
    return { projects: parsed as ProjectEntryV2[], collections: [] };
  }

  if (
    parsed !== null &&
    typeof parsed === 'object' &&
    typeof (parsed as { schemaVersion?: unknown }).schemaVersion === 'number'
  ) {
    const data = parsed as {
      schemaVersion: number;
      projects?: unknown;
      collections?: unknown;
    };
    if (data.schemaVersion > PROJECT_LIST_EXPORT_VERSION) {
      throw new Error(
        `Import file is from a newer version of Quarto Hub (schema ${data.schemaVersion}); update the app and try again`,
      );
    }
    const projects = Array.isArray(data.projects) ? (data.projects as ProjectEntryV2[]) : [];
    const collections = Array.isArray(data.collections)
      ? data.collections.filter(isValidExportedCollection)
      : [];
    return { projects, collections };
  }

  throw new Error('Invalid import format: expected array of projects or ExportData object');
}

/** A collection pointer is usable only with a non-empty docId and server. */
function isValidExportedCollection(c: unknown): c is ExportedCollection {
  if (c === null || typeof c !== 'object') return false;
  const e = c as { projectSetDocId?: unknown; syncServer?: unknown };
  return (
    typeof e.projectSetDocId === 'string' &&
    e.projectSetDocId.length > 0 &&
    typeof e.syncServer === 'string' &&
    e.syncServer.length > 0
  );
}
