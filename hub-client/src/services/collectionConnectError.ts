/**
 * CollectionConnectError — typed failure for the collections connect path.
 *
 * automerge-repo's `repo.find()` rejects with the bare
 * `Document <id> is unavailable` for every failure shape: server missing
 * the doc, websocket rejected with 401 (expired session), plain network
 * outage, or a sync channel that never opened. Surfacing that string
 * verbatim gave users nothing to act on (bd-tux4m6od). The connect path
 * classifies the failure into one of these kinds instead; `message` is
 * user-facing copy owned here (wording locked by
 * collectionConnectError.test.ts).
 */

export type CollectionConnectFailureKind =
  /** The hub answered HTTP but rejected the session (401/403 on /auth/me). */
  | 'auth-expired'
  /** The hub is unreachable over HTTP too — client-side network outage. */
  | 'offline'
  /** HTTP + session are fine, but no sync peer arrived over the websocket. */
  | 'sync-unreachable'
  /** A live sync peer answered and does not have the document. */
  | 'not-found'
  /** Anything else — the underlying error text is preserved. */
  | 'unknown';

function messageFor(
  kind: CollectionConnectFailureKind,
  docId: string,
  cause: unknown,
): string {
  switch (kind) {
    case 'auth-expired':
      return 'Your session has expired. Sign in again, then retry.';
    case 'offline':
      return (
        "Can't reach the sync server — you appear to be offline. " +
        'Check your connection and retry.'
      );
    case 'sync-unreachable':
      return (
        "Signed in, but the sync connection won't open. Reload the page to " +
        'retry; if this keeps happening the sync server may be down.'
      );
    case 'not-found':
      return (
        `This collection isn't available on the sync server (document ${docId}). ` +
        'The link may be stale, or the collection may not have finished ' +
        'syncing from its owner — ask them to open Quarto Hub and share it again.'
      );
    case 'unknown': {
      const detail =
        cause === undefined
          ? ''
          : `: ${cause instanceof Error ? cause.message : String(cause)}`;
      return `Couldn't load the collection (document ${docId})${detail}`;
    }
  }
}

export class CollectionConnectError extends Error {
  readonly kind: CollectionConnectFailureKind;
  /** Automerge doc id of the collection that failed to connect. */
  readonly docId: string;

  constructor(kind: CollectionConnectFailureKind, docId: string, cause?: unknown) {
    super(messageFor(kind, docId, cause), cause === undefined ? undefined : { cause });
    this.name = 'CollectionConnectError';
    this.kind = kind;
    this.docId = docId;
  }
}
