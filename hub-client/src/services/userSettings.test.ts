/**
 * Unit tests for the pure identity helpers in userSettings.
 *
 * `authorIdFromUserId` lets auth-less deployments (local-prod /
 * `--allow-insecure-auth`) derive a *stable* Automerge author id from the
 * local user id, so opening/editing a document still stamps a consistent
 * attribution identity even though the actor id is random per document
 * instance (author-ID transition, D2/D5).
 */

import { afterEach, describe, it, expect, vi } from 'vitest';
import { applyIdentityParam, authorIdFromUserId, getUserIdentity, identityFromSearch } from './userSettings';

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

describe('identityFromSearch', () => {
  it('reads the name from ?as= and derives a stable color from it', () => {
    const a = identityFromSearch('?as=Maurice');
    expect(a?.name).toBe('Maurice');
    expect(a?.color).toMatch(/^#[0-9A-Fa-f]{6}$/);
    expect(identityFromSearch('?as=Maurice')?.color).toBe(a?.color);
  });

  it('takes an explicit valid ?color= over the derived one', () => {
    expect(identityFromSearch('?as=Myra&color=%23E91E63')).toEqual({ name: 'Myra', color: '#E91E63' });
  });

  it('ignores an invalid ?color= and falls back to the derived color', () => {
    const got = identityFromSearch('?as=Myra&color=red');
    expect(got?.name).toBe('Myra');
    expect(got?.color).toMatch(/^#[0-9A-Fa-f]{6}$/);
  });

  it('trims the name and decodes it', () => {
    expect(identityFromSearch('?as=%20Myra%20Quill%20')?.name).toBe('Myra Quill');
  });

  it('returns null when there is no usable name', () => {
    expect(identityFromSearch('')).toBeNull();
    expect(identityFromSearch('?page=a.qmd')).toBeNull();
    expect(identityFromSearch('?as=')).toBeNull();
    expect(identityFromSearch('?as=%20%20')).toBeNull();
  });
});

describe('applyIdentityParam', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('sets the stored name and color from ?as= in the ephemeral preview build', async () => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '1');
    const applied = await applyIdentityParam('?as=Maurice&color=%232196F3');
    expect(applied?.userName).toBe('Maurice');
    expect(applied?.userColor).toBe('#2196F3');
    // Presence reads the identity back from storage, so a later read must agree.
    const stored = await getUserIdentity();
    expect(stored.userName).toBe('Maurice');
    expect(stored.userColor).toBe('#2196F3');
  });

  it('does nothing without the parameter', async () => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '1');
    expect(await applyIdentityParam('?page=a.qmd')).toBeNull();
  });

  it('does nothing outside the ephemeral build, so a link cannot rename a persistent profile', async () => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '0');
    expect(await applyIdentityParam('?as=Maurice')).toBeNull();
  });
});
