/**
 * Unit tests for the pure identity helpers in userSettings.
 *
 * `actorIdFromUserId` lets auth-less deployments (local-prod /
 * `--allow-insecure-auth`) derive a *stable* Automerge actor id from the
 * local user id, so opening/editing a document still stamps a consistent
 * identity instead of a fresh random actor each session.
 */

import { describe, it, expect } from 'vitest';
import { actorIdFromUserId } from './userSettings';

describe('actorIdFromUserId', () => {
  it('strips dashes from a UUID to form a valid hex actor id', () => {
    expect(actorIdFromUserId('6d914340-d834-489b-934c-58390f9b3301')).toBe(
      '6d914340d834489b934c58390f9b3301',
    );
  });

  it('always yields an even-length lowercase hex string (a valid Automerge actor)', () => {
    for (const id of [
      '6d914340-d834-489b-934c-58390f9b3301',
      '00000000-0000-0000-0000-000000000000',
      'ABCDEF01-2345-6789-ABCD-EF0123456789',
    ]) {
      const actor = actorIdFromUserId(id);
      expect(actor).toMatch(/^[0-9a-f]+$/);
      expect(actor.length % 2).toBe(0);
    }
  });

  it('is deterministic — same userId maps to the same actor id', () => {
    const id = '6d914340-d834-489b-934c-58390f9b3301';
    expect(actorIdFromUserId(id)).toBe(actorIdFromUserId(id));
  });

  it('hex-encodes a non-UUID userId defensively rather than emitting invalid hex', () => {
    const actor = actorIdFromUserId('not-a-uuid!');
    expect(actor).toMatch(/^[0-9a-f]+$/);
    expect(actor.length % 2).toBe(0);
  });
});
