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

export interface PreviewSessionConfig {
  /** Mirrors the host's `--allow-edit`: edits persist to disk. */
  allowEdit: boolean;
}

/**
 * Fetch the preview session config, or null when the serving server is
 * not a `q2 preview` session (standalone hub, dev server) or the fetch
 * fails. Only a response carrying an explicit boolean `allowEdit`
 * counts — SPA-fallback HTML and older servers both yield null.
 */
export async function fetchPreviewSessionConfig(): Promise<PreviewSessionConfig | null> {
  try {
    const res = await fetch(hubPath('/api/preview/config'), { credentials: 'same-origin' });
    if (!res.ok) return null;
    const data: unknown = await res.json();
    if (typeof data !== 'object' || data === null) return null;
    const { allowEdit } = data as { allowEdit?: unknown };
    if (typeof allowEdit !== 'boolean') return null;
    return { allowEdit };
  } catch {
    return null;
  }
}
