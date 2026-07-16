/**
 * Resolve the authoring actor to open a project with, and decide whether
 * the open must be gated behind a hub sign-in or reported as unopenable.
 *
 * Connection-gated auth (bd-u4p8xhdc): a local project (no sync server)
 * always opens, authored under the stable per-browser local actor. A hub
 * project (has a sync server) prefers the server-derived HMAC actor, which
 * requires a valid session. `resolveHubActor` is three-valued and, by
 * whether it *resolves* or *throws*, also tells us online-vs-offline:
 *
 *   - `string`    → actor resolved (signed in); open with it.
 *   - `undefined` → auth disabled (insecure/no-auth hub); open with no
 *                   explicit actor. No sign-in needed.
 *   - `null`      → the hub answered 401/403 (logged off, but we ARE
 *                   online — the request completed).
 *   - *throws*    → the request itself failed (offline / hub unreachable).
 *                   A network error is a `TypeError`; any other error
 *                   (e.g. an HTTP 500) is a real failure and propagates.
 *
 * Offline-cached hub projects (bd-qklxdkwh, epic bd-xxjy9yfp): when there
 * is no HMAC actor (logged off or offline) we no longer abandon. If the
 * project is already cached locally we open it from cache under the local
 * actor — a reconnect later switches back to the HMAC actor and bridges
 * authorship (B3). Only a project that is *not* cached needs a decision:
 *
 *   - online but logged off → `onNeedsSignIn` (signing in fetches it),
 *   - offline               → `onCannotOpenOffline` (nothing can be done).
 *
 * In both uncached cases `null` is returned so the caller abandons the
 * open; the callbacks surface the reason instead of a silent no-op.
 */
export interface OpenActorDeps {
  /** Stable per-browser local actor, used for local-only and cached-offline opens. */
  getLocalActor: () => Promise<string>;
  /** Server-derived HMAC actor for a hub project (three-valued; throws when offline). */
  resolveHubActor: (indexDocId: string) => Promise<string | null | undefined>;
  /** True iff the project's docs are already in the local cache. */
  isCached: (indexDocId: string) => Promise<boolean>;
  /** Hub project not cached, but we're online — signing in can fetch it. */
  onNeedsSignIn: () => void;
  /** Hub project not cached and we're offline — it genuinely can't be opened. */
  onCannotOpenOffline: () => void;
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

  let hubActor: string | null | undefined;
  let offline = false;
  try {
    hubActor = await deps.resolveHubActor(indexDocId);
  } catch (err) {
    // A network failure (offline / hub unreachable) surfaces as a
    // TypeError from fetch. Anything else (HTTP 500, etc.) is a genuine
    // error the caller should see — rethrow it.
    if (err instanceof TypeError) {
      offline = true;
      hubActor = null;
    } else {
      throw err;
    }
  }

  if (hubActor === null) {
    // No usable session (logged off) or offline. Open from cache under the
    // local actor if we have a cached copy; the hub connection degrades to
    // offline-from-cache and a later reconnect (B3) restores the HMAC actor.
    if (await deps.isCached(indexDocId)) {
      return deps.getLocalActor();
    }
    // Not cached: signing in would fetch it (online), but offline we can't.
    if (offline) {
      deps.onCannotOpenOffline();
    } else {
      deps.onNeedsSignIn();
    }
    return null;
  }

  return hubActor; // string (signed in) or undefined (auth disabled)
}
