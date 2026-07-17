/**
 * Auth-loss teardown decision (extracted from App.tsx).
 *
 * The teardown must fire only on a genuine auth LOSS — a session that
 * existed is now gone — and only for a hub-backed project. The pre-B1
 * state-based rule ("no auth && hub project open" → tear down) killed
 * B1's headline feature (bd-qklxdkwh): a cached hub project deliberately
 * opened while logged off, under the local actor, was unmounted on the
 * very next render — a flash of the editor, then back to the selector.
 *
 * Plan: claude-notes/plans/2026-07-15-hub-client-offline-cached-hub-projects.md
 */

import { describe, it, expect } from 'vitest';

import { shouldTeardownOnAuthChange } from './authTeardown';

const base = {
  authEnabled: true,
  hadAuth: true,
  hasAuth: false,
  authLoading: false,
  projectSyncServer: 'wss://hub.example.com/ws' as string | undefined,
};

describe('shouldTeardownOnAuthChange', () => {
  it('tears down a hub project on a genuine auth loss (signed in → signed out)', () => {
    expect(shouldTeardownOnAuthChange(base)).toBe(true);
  });

  it('does NOT tear down a hub project opened while already logged off (B1 offline open)', () => {
    expect(shouldTeardownOnAuthChange({ ...base, hadAuth: false })).toBe(false);
  });

  it('never tears down a local project (no sync server)', () => {
    expect(shouldTeardownOnAuthChange({ ...base, projectSyncServer: undefined })).toBe(false);
    expect(shouldTeardownOnAuthChange({ ...base, projectSyncServer: '' })).toBe(false);
  });

  it('does not fire while the auth probe is still loading', () => {
    expect(shouldTeardownOnAuthChange({ ...base, authLoading: true })).toBe(false);
  });

  it('does not fire when auth is disabled at build time', () => {
    expect(shouldTeardownOnAuthChange({ ...base, authEnabled: false })).toBe(false);
  });

  it('does not fire while a session is present', () => {
    expect(shouldTeardownOnAuthChange({ ...base, hasAuth: true })).toBe(false);
  });
});
