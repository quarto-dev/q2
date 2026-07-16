/**
 * resolveActorForOpen.
 *
 * v1 (bd-u4p8xhdc): with connection-gated auth, selecting a HUB project
 * (one with a sync server) while logged off must not fail silently — a
 * local project opens under the local actor; a hub project that needs a
 * session prompts sign-in.
 *
 * B1 (bd-qklxdkwh, epic bd-xxjy9yfp): a logged-off/offline open of a
 * *cached* hub project now opens from cache under the local actor instead
 * of prompting. Only an *uncached* hub project needs a decision — prompt
 * sign-in when online (signing in can fetch it) vs. report unopenable when
 * offline. `resolveHubActor` resolving `null` means logged-off-but-online;
 * it *throwing* a network error means offline.
 *
 * Plan: claude-notes/plans/2026-07-15-hub-client-offline-cached-hub-projects.md
 */

import { describe, it, expect, vi } from 'vitest';

import { resolveActorForOpen, type OpenActorDeps } from './openActor';

/** Deps with sensible defaults; override per case. */
function makeDeps(overrides: Partial<OpenActorDeps> = {}): OpenActorDeps {
  return {
    getLocalActor: async () => 'localactor00000000000000000000aa',
    resolveHubActor: async () => 'hubactor11111111111111111111bbbb',
    isCached: async () => false,
    onNeedsSignIn: vi.fn(),
    onCannotOpenOffline: vi.fn(),
    ...overrides,
  };
}

describe('resolveActorForOpen', () => {
  it('opens a local project (no sync server) under the local actor, no prompts', async () => {
    const deps = makeDeps({
      resolveHubActor: vi.fn(async () => null), // must not be consulted
      getLocalActor: async () => 'localactor00000000000000000000aa',
    });
    const actor = await resolveActorForOpen('automerge:local', '', deps);
    expect(actor).toBe('localactor00000000000000000000aa');
    expect(deps.resolveHubActor).not.toHaveBeenCalled();
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });

  it('opens a hub project with the resolved HMAC actor when signed in', async () => {
    const deps = makeDeps({ resolveHubActor: async () => 'hubactor11111111111111111111bbbb' });
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', deps);
    expect(actor).toBe('hubactor11111111111111111111bbbb');
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });

  it('opens a hub project with no actor when auth is disabled (undefined, no prompt)', async () => {
    const deps = makeDeps({ resolveHubActor: async () => undefined });
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', deps);
    expect(actor).toBeUndefined();
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });

  it('prompts sign-in for an UNCACHED hub project when logged off but online (401 → null)', async () => {
    const deps = makeDeps({
      resolveHubActor: async () => null, // 401/403, request completed → online
      isCached: async () => false,
    });
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', deps);
    expect(actor).toBeNull();
    expect(deps.onNeedsSignIn).toHaveBeenCalledTimes(1);
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });

  it('B1: opens a CACHED hub project under the local actor when logged off (null), no prompt', async () => {
    const deps = makeDeps({
      resolveHubActor: async () => null,
      isCached: async () => true,
      getLocalActor: async () => 'localactor00000000000000000000aa',
    });
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', deps);
    expect(actor).toBe('localactor00000000000000000000aa');
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });

  it('B1: opens a CACHED hub project under the local actor when offline (fetch throws)', async () => {
    const deps = makeDeps({
      resolveHubActor: async () => {
        throw new TypeError('Failed to fetch'); // offline
      },
      isCached: async () => true,
      getLocalActor: async () => 'localactor00000000000000000000aa',
    });
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', deps);
    expect(actor).toBe('localactor00000000000000000000aa');
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });

  it('B1: reports offline-unopenable for an UNCACHED hub project when offline (fetch throws)', async () => {
    const deps = makeDeps({
      resolveHubActor: async () => {
        throw new TypeError('Failed to fetch'); // offline
      },
      isCached: async () => false,
    });
    const actor = await resolveActorForOpen('automerge:hub', 'wss://hub', deps);
    expect(actor).toBeNull();
    expect(deps.onCannotOpenOffline).toHaveBeenCalledTimes(1);
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
  });

  it('propagates a genuine (non-network) resolve error instead of swallowing it', async () => {
    const deps = makeDeps({
      resolveHubActor: async () => {
        throw new Error('/auth/actor failed: 500'); // real server error
      },
    });
    await expect(resolveActorForOpen('automerge:hub', 'wss://hub', deps)).rejects.toThrow('500');
    expect(deps.onNeedsSignIn).not.toHaveBeenCalled();
    expect(deps.onCannotOpenOffline).not.toHaveBeenCalled();
  });
});
