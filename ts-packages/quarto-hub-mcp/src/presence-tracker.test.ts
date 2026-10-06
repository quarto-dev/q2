/**
 * Unit tests for PresenceTracker (CAP-8): staleness, pruning, leave
 * handling, and schema rejection — driven with an injected clock and
 * fabricated handles. Wire-path coverage (real sync delivery, cursor
 * resolution against a real replica) lives in presence.test.ts.
 */

import { describe, it, expect, vi } from 'vitest';
import { EventEmitter } from 'node:events';

import {
  PresenceTracker,
  PRESENCE_ACTIVE_WITHIN_MS,
  PRESENCE_PRUNE_AFTER_MS,
} from './presence-tracker.js';
import type { SyncClient } from '@quarto/quarto-sync-client';

class FakeHandle extends EventEmitter {
  doc(): unknown {
    return { text: 'hello\n' };
  }
}

function fakeClient(fileHandle: FakeHandle, indexHandle: FakeHandle): SyncClient {
  return {
    getFilePaths: () => ['a.qmd'],
    getFileHandle: (path: string) => (path === 'a.qmd' ? fileHandle : null),
    getIndexHandle: () => indexHandle,
  } as unknown as SyncClient;
}

const MESSAGE = {
  type: 'presence' as const,
  peerId: 'peer-1',
  userId: 'user-1',
  userName: 'Ula',
  userColor: '#123456',
  cursor: null,
  selection: null,
};

describe('PresenceTracker', () => {
  it('records a presence broadcast on a file handle with its path', () => {
    const fileHandle = new FakeHandle();
    const tracker = new PresenceTracker(fakeClient(fileHandle, new FakeHandle()));
    tracker.attach();
    fileHandle.emit('ephemeral-message', { message: MESSAGE });

    const snap = tracker.snapshot();
    expect(snap).toHaveLength(1);
    expect(snap[0].peer_id).toBe('peer-1');
    expect(snap[0].user_name).toBe('Ula');
    expect(snap[0].file_path).toBe('a.qmd');
    expect(snap[0].active).toBe(true);
    tracker.dispose();
  });

  it('marks a peer inactive past the active window and prunes it past the prune window', () => {
    const fileHandle = new FakeHandle();
    let t = 1_000;
    const tracker = new PresenceTracker(fakeClient(fileHandle, new FakeHandle()), {
      now: () => t,
    });
    tracker.attach();
    fileHandle.emit('ephemeral-message', { message: MESSAGE });

    t = 1_000 + PRESENCE_ACTIVE_WITHIN_MS - 1;
    expect(tracker.snapshot()[0].active).toBe(true);

    t = 1_000 + PRESENCE_ACTIVE_WITHIN_MS + 1;
    const stale = tracker.snapshot();
    expect(stale).toHaveLength(1);
    expect(stale[0].active).toBe(false);
    expect(stale[0].last_seen_ms_ago).toBe(PRESENCE_ACTIVE_WITHIN_MS + 1);

    t = 1_000 + PRESENCE_PRUNE_AFTER_MS + 1;
    expect(tracker.snapshot()).toHaveLength(0);
    tracker.dispose();
  });

  it('drops a peer on leave', () => {
    const fileHandle = new FakeHandle();
    const tracker = new PresenceTracker(fakeClient(fileHandle, new FakeHandle()));
    tracker.attach();
    fileHandle.emit('ephemeral-message', { message: MESSAGE });
    expect(tracker.snapshot()).toHaveLength(1);
    fileHandle.emit('ephemeral-message', { message: { type: 'leave', peerId: 'peer-1' } });
    expect(tracker.snapshot()).toHaveLength(0);
    tracker.dispose();
  });

  it('ignores malformed messages without throwing', () => {
    const fileHandle = new FakeHandle();
    const tracker = new PresenceTracker(fakeClient(fileHandle, new FakeHandle()));
    tracker.attach();
    fileHandle.emit('ephemeral-message', { message: { type: 'presence' } });
    fileHandle.emit('ephemeral-message', { message: 'garbage' });
    fileHandle.emit('ephemeral-message', { message: { unrelated: true } });
    expect(tracker.snapshot()).toHaveLength(0);
    tracker.dispose();
  });

  it('stops listening after dispose', () => {
    const fileHandle = new FakeHandle();
    const tracker = new PresenceTracker(fakeClient(fileHandle, new FakeHandle()));
    tracker.attach();
    tracker.dispose();
    fileHandle.emit('ephemeral-message', { message: MESSAGE });
    expect(tracker.snapshot()).toHaveLength(0);
  });
});
