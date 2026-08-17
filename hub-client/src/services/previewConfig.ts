/**
 * Preview Session Config
 *
 * `q2 preview` serves `GET /api/preview/config` with session-level
 * settings (bd-ov4gqk3m) — currently `allowEdit`, which mirrors the
 * CLI's `--allow-edit` flag: whether edits made in the UI are written
 * back to the host's files on disk. Without `--allow-edit` the session
 * is an ephemeral sandbox: edits sync live to everyone connected but
 * are never persisted.
 *
 * The endpoint exists only on the per-session preview server (a `--join`
 * guest's local TCP proxy splices every connection through to the
 * host, so guests read the host's value). A standalone hub has no such
 * route — it 404s or answers with the SPA fallback — and both outcomes
 * are treated here as "not a preview session" (null), so the editor
 * shows no ephemeral-session UI against a real hub.
 */

import { hubPath } from '../utils/routing';

/**
 * Editor-mode boot params served as `editorBoot` (bd-7htq16rx): the
 * share-route coordinates the host's own boot URL was built from.
 * Present only on editor-UI sessions. Ephemeral storage mode
 * (bd-sw4xy1vw) uses these to rebuild the session after a page reload,
 * when the in-memory project entry is gone.
 */
export interface EditorBoot {
  /** Index document id (may carry an `automerge:` prefix). */
  indexDocId: string;
  /** The share route's file param — a `.qmd` path in the project. */
  file: string;
  /** Project name shown in the editor UI. */
  name: string;
}

export interface PreviewSessionConfig {
  /** Mirrors the host's `--allow-edit`: edits persist to disk. */
  allowEdit: boolean;
  /** Editor-mode boot params; absent on viewer-UI and older servers. */
  editorBoot?: EditorBoot;
}

/** Delay before the single retry of a transport-failed config fetch. */
const RETRY_DELAY_MS = 750;

/**
 * Validate a raw `editorBoot` value. Anything short of three non-empty
 * strings is dropped (treated as absent) — a partial boot record would
 * build a broken share route, and the share handler's own validation
 * would reject it anyway.
 */
function parseEditorBoot(value: unknown): EditorBoot | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const { indexDocId, file, name } = value as Record<string, unknown>;
  if (
    typeof indexDocId === 'string' && indexDocId.length > 0 &&
    typeof file === 'string' && file.length > 0 &&
    typeof name === 'string' && name.length > 0
  ) {
    return { indexDocId, file, name };
  }
  return undefined;
}

/**
 * Fetch the preview session config, or null when the serving server is
 * not a `q2 preview` session (standalone hub, dev server) or the fetch
 * fails. Only a response carrying an explicit boolean `allowEdit`
 * counts — SPA-fallback HTML and older servers both yield null.
 *
 * A transport-level failure (fetch rejects) is retried once: for a
 * `--join` guest the boot fetch can race the tunnel's connection
 * handshake, and the config is fetched only once per boot — a dropped
 * request would hide the ephemeral-session banner for the whole
 * session. Definitive answers (non-ok status, non-JSON or malformed
 * body) are not retried: on a standalone hub they are the expected
 * "not a preview session" signal.
 */
export async function fetchPreviewSessionConfig(): Promise<PreviewSessionConfig | null> {
  for (let attempt = 0; ; attempt++) {
    let res: Response;
    try {
      res = await fetch(hubPath('/api/preview/config'), { credentials: 'same-origin' });
    } catch {
      if (attempt === 0) {
        await new Promise((resolve) => setTimeout(resolve, RETRY_DELAY_MS));
        continue;
      }
      return null;
    }
    if (!res.ok) return null;
    try {
      const data: unknown = await res.json();
      if (typeof data !== 'object' || data === null) return null;
      const { allowEdit, editorBoot } = data as {
        allowEdit?: unknown;
        editorBoot?: unknown;
      };
      if (typeof allowEdit !== 'boolean') return null;
      const parsedBoot = parseEditorBoot(editorBoot);
      return parsedBoot ? { allowEdit, editorBoot: parsedBoot } : { allowEdit };
    } catch {
      return null;
    }
  }
}
