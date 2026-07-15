/**
 * Resolve the authoring actor to open a project with, and decide whether the
 * open must be gated behind a hub sign-in.
 *
 * Connection-gated auth (bd-u4p8xhdc): a local project (no sync server) always
 * opens, authored under the stable per-browser local actor. A hub project
 * (has a sync server) uses the server-derived HMAC actor, which requires a
 * valid session:
 *
 *   - `string`    → actor resolved; open with it.
 *   - `undefined` → auth is disabled (e.g. an insecure/no-auth hub); open with
 *                   no explicit actor. No sign-in needed.
 *   - `null`      → the hub rejected us (401/403) — we need a session we don't
 *                   have. `onNeedsSignIn` fires so the caller can surface the
 *                   sign-in prompt instead of a silent no-op, and `null` is
 *                   returned so the caller still abandons this open attempt.
 */
export interface OpenActorDeps {
  /** Stable per-browser local actor for local-only projects. */
  getLocalActor: () => Promise<string>;
  /** Server-derived HMAC actor for a hub project (three-valued; see above). */
  resolveHubActor: (indexDocId: string) => Promise<string | null | undefined>;
  /** Called when a hub open needs a session we don't have (prompt sign-in). */
  onNeedsSignIn: () => void;
}

export async function resolveActorForOpen(
  indexDocId: string,
  syncServer: string,
  deps: OpenActorDeps,
): Promise<string | null | undefined> {
  if (!syncServer) {
    // Local-only project — always openable, no session required.
    return deps.getLocalActor();
  }
  const actor = await deps.resolveHubActor(indexDocId);
  if (actor === null) {
    // Hub project needs a session we don't have. Surface the sign-in prompt
    // rather than letting the caller's `=== null` guard fail silently.
    deps.onNeedsSignIn();
  }
  return actor;
}
