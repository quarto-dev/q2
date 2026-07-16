/**
 * resolveActorForOpen (bd-u4p8xhdc follow-up).
 *
 * Bug: with connection-gated auth, selecting a HUB project (one with a sync
 * server) while logged off silently did nothing — the actor resolve returned
 * null (401) and the open path just `return`ed with no feedback. A local
 * project must still open with no session; a hub project that needs a session
 * we don't have must PROMPT sign-in, not fail silently.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 */

import { describe, it, expect, vi } from 'vitest';

import { resolveActorForOpen } from './openActor';

describe('resolveActorForOpen', () => {
  it('opens a local project (no sync server) under the local actor, no sign-in prompt', async () => {
    const onNeedsSignIn = vi.fn();
    const actor = await resolveActorForOpen('automerge:local', '', {
      getLocalActor: async () => 'localactor00000000000000000000aa',
      resolveHubActor: async () => null, // must not be consulted for a local project
      onNeedsSignIn,
    });
    expect(actor).toBe('localactor00000000000000000000aa');
    expect(onNeedsSignIn).not.toHaveBeenCalled();
  });

  it('opens a hub project with the resolved HMAC actor when signed in', async () => {
    const onNeedsSignIn = vi.fn();
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', {
      getLocalActor: async () => 'localactor',
      resolveHubActor: async () => 'hubactor11111111111111111111bbbb',
      onNeedsSignIn,
    });
    expect(actor).toBe('hubactor11111111111111111111bbbb');
    expect(onNeedsSignIn).not.toHaveBeenCalled();
  });

  it('prompts sign-in when a hub project needs a session we do not have (401 → null)', async () => {
    const onNeedsSignIn = vi.fn();
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', {
      getLocalActor: async () => 'localactor',
      resolveHubActor: async () => null, // 401/403 from /auth/actor
      onNeedsSignIn,
    });
    // Still returns null so the caller abandons the open...
    expect(actor).toBeNull();
    // ...but surfaces the sign-in prompt instead of a silent no-op.
    expect(onNeedsSignIn).toHaveBeenCalledTimes(1);
  });

  it('opens a hub project with no actor when auth is disabled (undefined, no prompt)', async () => {
    const onNeedsSignIn = vi.fn();
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', {
      getLocalActor: async () => 'localactor',
      resolveHubActor: async () => undefined, // AUTH_ENABLED false → insecure/no-auth hub
      onNeedsSignIn,
    });
    expect(actor).toBeUndefined();
    expect(onNeedsSignIn).not.toHaveBeenCalled();
  });

  // B1 (bd-qklxdkwh, epic bd-xxjy9yfp) supersedes the prompt-sign-in
  // behavior for *cached* hub projects: a logged-off open of a cached hub
  // project falls back to the local actor and opens from cache instead of
  // firing onNeedsSignIn; only a genuinely never-cached + offline project
  // surfaces a precise "can't open" reason. These land red→green with B1's
  // seam extension (cache-awareness in the openActor deps).
  it.todo('B1: cached hub project + null HMAC actor opens under the local actor (no prompt)');
  it.todo('B1: never-cached hub project + offline reports a precise "not cached" reason');
});
