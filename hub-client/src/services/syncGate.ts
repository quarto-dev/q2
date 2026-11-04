/**
 * Sync gate — whether Automerge repos may open a network adapter.
 *
 * Optional-auth prototype: when auth is configured but the user has not
 * signed in, the hub would reject every WebSocket upgrade, so repos are
 * built with local adapters only (storage, no network). Signing in opens
 * the gate; signing out closes it. Repo factories (`projectSetService`
 * for collections, `connectAndLoadContents` for projects) consult
 * `isSyncAllowed()` at build time and `projectSetService` also attaches /
 * detaches its adapters live on `onSyncAllowedChange`.
 *
 * Defaults to allowed so auth-less deployments and tests are unaffected;
 * App closes it at boot when auth is enabled and re-evaluates once
 * `/auth/me` has answered.
 */

type Listener = (allowed: boolean) => void;

let allowed = true;
const listeners = new Set<Listener>();

export function isSyncAllowed(): boolean {
  return allowed;
}

export function setSyncAllowed(next: boolean): void {
  if (next === allowed) return;
  allowed = next;
  for (const listener of listeners) listener(next);
}

/** Subscribe to gate changes. Returns an unsubscribe function. */
export function onSyncAllowedChange(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
