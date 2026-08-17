/**
 * Tests for CollectionConnectError — the typed error the collections
 * connect path throws instead of automerge-repo's bare
 * `Document <id> is unavailable` (bd-tux4m6od).
 *
 * The message wording is locked here (same convention as
 * quarto-sync-client's dangling-entries.test.ts): these strings are
 * user-facing UI copy, and the 2026-06-12 incident showed that vague
 * unavailable-text sends responders down the wrong path. Change the
 * copy deliberately or not at all.
 */

import { describe, it, expect } from 'vitest';
import { CollectionConnectError } from './collectionConnectError';

const DOC_ID = '2Agx7kENjysHSujsVgirvykVKECf';

describe('CollectionConnectError', () => {
  it('is an Error with kind, docId, and cause', () => {
    const cause = new Error(`Document ${DOC_ID} is unavailable`);
    const err = new CollectionConnectError('not-found', DOC_ID, cause);
    expect(err).toBeInstanceOf(Error);
    expect(err.name).toBe('CollectionConnectError');
    expect(err.kind).toBe('not-found');
    expect(err.docId).toBe(DOC_ID);
    expect(err.cause).toBe(cause);
  });

  it('auth-expired: names the expired session and the sign-in remedy', () => {
    const err = new CollectionConnectError('auth-expired', DOC_ID);
    expect(err.message).toBe('Your session has expired. Sign in again, then retry.');
  });

  it('offline: names the unreachable server and the connection remedy', () => {
    const err = new CollectionConnectError('offline', DOC_ID);
    expect(err.message).toBe(
      "Can't reach the sync server — you appear to be offline. " +
        'Check your connection and retry.',
    );
  });

  it('sync-unreachable: distinguishes a healthy sign-in from a dead sync channel', () => {
    const err = new CollectionConnectError('sync-unreachable', DOC_ID);
    expect(err.message).toBe(
      "Signed in, but the sync connection won't open. Reload the page to " +
        'retry; if this keeps happening the sync server may be down.',
    );
  });

  it('not-found: names the server-side absence, the doc id, and the re-share remedy', () => {
    const err = new CollectionConnectError('not-found', DOC_ID);
    expect(err.message).toBe(
      `This collection isn't available on the sync server (document ${DOC_ID}). ` +
        'The link may be stale, or the collection may not have finished ' +
        'syncing from its owner — ask them to open Quarto Hub and share it again.',
    );
  });

  it('unknown: preserves the underlying error text and the doc id', () => {
    const err = new CollectionConnectError('unknown', DOC_ID, new Error('boom'));
    expect(err.message).toBe(`Couldn't load the collection (document ${DOC_ID}): boom`);
  });

  it('unknown: stringifies a non-Error cause', () => {
    const err = new CollectionConnectError('unknown', DOC_ID, 'string failure');
    expect(err.message).toBe(
      `Couldn't load the collection (document ${DOC_ID}): string failure`,
    );
  });

  it('unknown without a cause still identifies the document', () => {
    const err = new CollectionConnectError('unknown', DOC_ID);
    expect(err.message).toBe(`Couldn't load the collection (document ${DOC_ID})`);
  });
});
