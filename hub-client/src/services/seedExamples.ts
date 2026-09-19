/**
 * Seed a new user's "Examples / Templates" collection (bd-3fwtdhil).
 *
 * A brand-new browser gets an empty personal root (bd-4h1hv60p) and, right
 * after it, this: one extra collection holding every project choice the
 * registry flags as `seed` — the Meeting Notes, Website, Article, and
 * Presentation examples. Each is a short instructional project the user can
 * read, edit, or fork. The hub is the only surface that offers them
 * (bd-d147nkqx); the flag and the order both come from the Rust registry, so
 * there is no second list here to drift.
 *
 * Pure orchestration over injected dependencies. Failures are per project:
 * a scaffold or document-creation error skips that example and the rest
 * still land. Only a failure to create the collection itself aborts.
 *
 * Plan: claude-notes/plans/2026-09-17-get-started-collection.md
 */

/** Name of the seeded collection, as shown on the projects home. */
export const SEED_COLLECTION_NAME = 'Examples / Templates';

/** The slice of a registry choice this module reads. */
export interface SeedChoice {
  id: string;
  name: string;
  description: string;
  /** Set on the choices that seed the collection; absent means false. */
  seed?: boolean;
}

/** A scaffold file as `create_project` returns it. */
export interface SeedScaffoldFile {
  path: string;
  content_type: 'text' | 'binary';
  content: string;
  mime_type?: string;
}

export interface SeedDeps {
  /** Portable sync-server value to store and share (not the resolved URL). */
  syncServer: string;
  /** Turn the portable value into the absolute ws(s):// URL the client connects to. */
  resolveSyncServerUrl: (syncServer: string) => string;
  /** The hub surface's project choices, from WASM. */
  getProjectChoices: () => Promise<SeedChoice[]>;
  /** Scaffold one choice into files, from WASM. */
  createProject: (
    choiceId: string,
    title: string,
  ) => Promise<{ success: boolean; error?: string; files?: SeedScaffoldFile[] }>;
  /** Create the Automerge documents for a scaffold. */
  createNewProject: (options: {
    syncServer: string;
    files: Array<{ path: string; content: string; contentType: 'text' | 'binary'; mimeType?: string }>;
  }) => Promise<{ indexDocId: string }>;
  /** Record the project in local storage (IndexedDB). */
  addLocalProject: (indexDocId: string, syncServer: string, description: string) => Promise<unknown>;
  /** Create a named collection on the root's server; returns its doc id. */
  createCollection: (name: string) => Promise<string>;
  /** File a project entry in a collection. */
  addProjectToCollection: (
    collectionDocId: string,
    entry: { indexDocId: string; syncServer: string; description: string },
  ) => void;
  /** Diagnostic sink; defaults to console.warn at the call site. */
  log: (message: string, detail: unknown) => void;
}

export interface SeedResult {
  collectionDocId: string;
  /** Choice ids that landed in the collection, in registry order. */
  seeded: string[];
  /** Choice ids that were skipped after an error. */
  failed: string[];
}

/**
 * Create the collection and fill it. Returns null when there is nothing to
 * seed or the collection could not be created.
 */
export async function seedExampleProjects(deps: SeedDeps): Promise<SeedResult | null> {
  const choices = (await deps.getProjectChoices()).filter((c) => c.seed === true);
  if (choices.length === 0) return null;

  let collectionDocId: string;
  try {
    collectionDocId = await deps.createCollection(SEED_COLLECTION_NAME);
  } catch (err) {
    deps.log(`Could not create the "${SEED_COLLECTION_NAME}" collection; skipping examples`, err);
    return null;
  }

  const result: SeedResult = { collectionDocId, seeded: [], failed: [] };

  for (const choice of choices) {
    try {
      const scaffold = await deps.createProject(choice.id, choice.name);
      if (!scaffold.success || !scaffold.files) {
        throw new Error(scaffold.error ?? 'scaffold returned no files');
      }
      const created = await deps.createNewProject({
        syncServer: deps.resolveSyncServerUrl(deps.syncServer),
        files: scaffold.files.map((f) => ({
          path: f.path,
          content: f.content,
          contentType: f.content_type,
          mimeType: f.mime_type,
        })),
      });
      await deps.addLocalProject(created.indexDocId, deps.syncServer, choice.name);
      deps.addProjectToCollection(collectionDocId, {
        indexDocId: created.indexDocId,
        syncServer: deps.syncServer,
        description: choice.name,
      });
      result.seeded.push(choice.id);
    } catch (err) {
      deps.log(`Could not seed example ${choice.id}; skipping it`, err);
      result.failed.push(choice.id);
    }
  }

  return result;
}
