/**
 * Unit tests for the pure identity helpers in userSettings.
 *
 * `authorIdFromUserId` lets auth-less deployments (local-prod /
 * `--allow-insecure-auth`) derive a *stable* Automerge author id from the
 * local user id, so opening/editing a document still stamps a consistent
 * attribution identity even though the actor id is random per document
 * instance (author-ID transition, D2/D5).
 */

import { describe, it, expect } from 'vitest';
import { authorIdFromUserId } from './userSettings';

describe('authorIdFromUserId', () => {
  it('strips dashes from a UUID to form a valid hex author id', () => {
    expect(authorIdFromUserId('6d914340-d834-489b-934c-58390f9b3301')).toBe(
      '6d914340d834489b934c58390f9b3301',
    );
  });

  it('always yields an even-length lowercase hex string (a valid Automerge author)', () => {
    for (const id of [
      '6d914340-d834-489b-934c-58390f9b3301',
      '00000000-0000-0000-0000-000000000000',
      'ABCDEF01-2345-6789-ABCD-EF0123456789',
    ]) {
      const author = authorIdFromUserId(id);
      expect(author).toMatch(/^[0-9a-f]+$/);
      expect(author.length % 2).toBe(0);
    }
  });

  it('is deterministic — same userId maps to the same author id', () => {
    const id = '6d914340-d834-489b-934c-58390f9b3301';
    expect(authorIdFromUserId(id)).toBe(authorIdFromUserId(id));
  });

  it('hex-encodes a non-UUID userId defensively rather than emitting invalid hex', () => {
    const author = authorIdFromUserId('not-a-uuid!');
    expect(author).toMatch(/^[0-9a-f]+$/);
    expect(author.length % 2).toBe(0);
  });
});
