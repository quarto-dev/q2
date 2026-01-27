/**
 * Tests for presenceService
 *
 * These tests verify the presence service's behavior for real-time
 * collaborative editing features: cursor tracking, selections, and
 * presence state management.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  initPresence,
  cleanupPresence,
  setCurrentFile,
  updateCursor,
  updateSelection,
  updatePresence,
  getRemotePresences,
  onPresenceChange,
  getLocalIdentity,
  getLocalPeerId,
  _resetForTesting,
  _getStateForTesting,
} from './presenceService';

// Mock the userSettings module
vi.mock('./userSettings', () => ({
  getUserIdentity: vi.fn().mockResolvedValue({
    key: 'identity',
    userId: 'test-user-123',
    userName: 'Test User',
    userColor: '#3498db',
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
  }),
}));

// Mock the automergeSync module
vi.mock('./automergeSync', () => ({
  getFileHandle: vi.fn().mockReturnValue(null),
}));

describe('presenceService', () => {
  beforeEach(() => {
    _resetForTesting();
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanupPresence();
    vi.useRealTimers();
  });

  describe('initialization', () => {
    it('should initialize with user identity', async () => {
      await initPresence();
      const identity = getLocalIdentity();

      expect(identity).not.toBeNull();
      expect(identity?.userId).toBe('test-user-123');
      expect(identity?.userName).toBe('Test User');
      expect(identity?.userColor).toBe('#3498db');
    });

    it('should generate a unique peer ID', async () => {
      await initPresence();
      const peerId = getLocalPeerId();

      expect(peerId).toBeTruthy();
      expect(typeof peerId).toBe('string');
      // UUID format check
      expect(peerId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i);
    });

    it('should start cleanup interval', async () => {
      await initPresence();
      const state = _getStateForTesting();

      expect(state.cleanupInterval).not.toBeNull();
    });

    it('should accept custom configuration', async () => {
      await initPresence({
        broadcastThrottleMs: 100,
        staleThresholdMs: 10000,
        cleanupIntervalMs: 5000,
      });

      const state = _getStateForTesting();
      expect(state.config.broadcastThrottleMs).toBe(100);
      expect(state.config.staleThresholdMs).toBe(10000);
      expect(state.config.cleanupIntervalMs).toBe(5000);
    });
  });

  describe('cleanup', () => {
    it('should clear state on cleanup', async () => {
      await initPresence();
      cleanupPresence();

      const state = _getStateForTesting();
      expect(state.currentFilePath).toBeNull();
      expect(state.currentHandle).toBeNull();
      expect(state.remotePresences.size).toBe(0);
      expect(state.localCursor).toBeNull();
      expect(state.localSelection).toBeNull();
    });

    it('should clear cleanup interval on cleanup', async () => {
      await initPresence();
      cleanupPresence();

      const state = _getStateForTesting();
      expect(state.cleanupInterval).toBeNull();
    });
  });

  describe('cursor and selection tracking', () => {
    beforeEach(async () => {
      await initPresence();
    });

    it('should update cursor position', () => {
      updateCursor(42);
      const state = _getStateForTesting();
      expect(state.localCursor).toBe(42);
    });

    it('should update selection range', () => {
      updateSelection({ start: 10, end: 20 });
      const state = _getStateForTesting();
      expect(state.localSelection).toEqual({ start: 10, end: 20 });
    });

    it('should update both cursor and selection together', () => {
      updatePresence(15, { start: 10, end: 25 });
      const state = _getStateForTesting();
      expect(state.localCursor).toBe(15);
      expect(state.localSelection).toEqual({ start: 10, end: 25 });
    });

    it('should clear cursor when set to null', () => {
      updateCursor(42);
      updateCursor(null);
      const state = _getStateForTesting();
      expect(state.localCursor).toBeNull();
    });

    it('should clear selection when set to null', () => {
      updateSelection({ start: 10, end: 20 });
      updateSelection(null);
      const state = _getStateForTesting();
      expect(state.localSelection).toBeNull();
    });
  });

  describe('file switching', () => {
    beforeEach(async () => {
      await initPresence();
    });

    it('should set current file path', () => {
      setCurrentFile('index.qmd');
      const state = _getStateForTesting();
      expect(state.currentFilePath).toBe('index.qmd');
    });

    it('should clear cursor and selection when switching files', () => {
      setCurrentFile('file1.qmd');
      updatePresence(10, { start: 5, end: 15 });

      setCurrentFile('file2.qmd');

      const state = _getStateForTesting();
      expect(state.localCursor).toBeNull();
      expect(state.localSelection).toBeNull();
    });

    it('should clear remote presences when switching files', () => {
      // Manually add a remote presence for testing
      const state = _getStateForTesting();
      state.remotePresences.set('peer-1', {
        peerId: 'peer-1',
        userId: 'user-1',
        userName: 'User 1',
        userColor: '#ff0000',
        filePath: 'file1.qmd',
        cursor: 10,
        selection: null,
        lastSeen: Date.now(),
      });

      setCurrentFile('file2.qmd');

      expect(state.remotePresences.size).toBe(0);
    });

    it('should not change state when setting same file', () => {
      setCurrentFile('index.qmd');
      updatePresence(10, { start: 5, end: 15 });

      setCurrentFile('index.qmd');

      const state = _getStateForTesting();
      // Cursor and selection should be preserved when "switching" to same file
      expect(state.localCursor).toBe(10);
      expect(state.localSelection).toEqual({ start: 5, end: 15 });
    });
  });

  describe('subscription', () => {
    beforeEach(async () => {
      await initPresence();
    });

    it('should call subscriber with initial empty presences', () => {
      const callback = vi.fn();
      onPresenceChange(callback);

      expect(callback).toHaveBeenCalledWith([]);
    });

    it('should return unsubscribe function', () => {
      const callback = vi.fn();
      const unsubscribe = onPresenceChange(callback);

      expect(typeof unsubscribe).toBe('function');
    });

    it('should not call callback after unsubscribe', () => {
      const callback = vi.fn();
      const unsubscribe = onPresenceChange(callback);

      // Clear initial call
      callback.mockClear();

      unsubscribe();

      // Trigger a state change that would notify subscribers
      setCurrentFile('test.qmd');

      expect(callback).not.toHaveBeenCalled();
    });

    it('should support multiple subscribers', () => {
      const callback1 = vi.fn();
      const callback2 = vi.fn();

      onPresenceChange(callback1);
      onPresenceChange(callback2);

      expect(callback1).toHaveBeenCalled();
      expect(callback2).toHaveBeenCalled();
    });
  });

  describe('remote presences', () => {
    beforeEach(async () => {
      await initPresence();
    });

    it('should return empty array when no remote presences', () => {
      const presences = getRemotePresences();
      expect(presences).toEqual([]);
    });

    it('should return remote presences when present', () => {
      // Manually add remote presence for testing
      const state = _getStateForTesting();
      state.remotePresences.set('peer-1', {
        peerId: 'peer-1',
        userId: 'user-1',
        userName: 'Alice',
        userColor: '#ff0000',
        filePath: 'index.qmd',
        cursor: 42,
        selection: { start: 40, end: 45 },
        lastSeen: Date.now(),
      });

      const presences = getRemotePresences();
      expect(presences).toHaveLength(1);
      expect(presences[0].userName).toBe('Alice');
      expect(presences[0].cursor).toBe(42);
    });
  });

  describe('stale presence cleanup', () => {
    beforeEach(async () => {
      await initPresence({
        staleThresholdMs: 1000,
        cleanupIntervalMs: 100,
      });
    });

    it('should remove stale presences after threshold', () => {
      const state = _getStateForTesting();

      // Add a presence with old timestamp
      state.remotePresences.set('stale-peer', {
        peerId: 'stale-peer',
        userId: 'stale-user',
        userName: 'Stale User',
        userColor: '#cccccc',
        filePath: 'index.qmd',
        cursor: 0,
        selection: null,
        lastSeen: Date.now() - 2000, // 2 seconds ago (stale)
      });

      // Fast-forward time to trigger cleanup
      vi.advanceTimersByTime(150);

      expect(state.remotePresences.size).toBe(0);
    });

    it('should keep fresh presences', () => {
      const state = _getStateForTesting();

      // Add a fresh presence
      state.remotePresences.set('fresh-peer', {
        peerId: 'fresh-peer',
        userId: 'fresh-user',
        userName: 'Fresh User',
        userColor: '#00ff00',
        filePath: 'index.qmd',
        cursor: 10,
        selection: null,
        lastSeen: Date.now(),
      });

      // Fast-forward time to trigger cleanup
      vi.advanceTimersByTime(150);

      expect(state.remotePresences.size).toBe(1);
    });
  });

  describe('test isolation', () => {
    it('should have clean state after reset', async () => {
      await initPresence();
      setCurrentFile('test.qmd');
      updatePresence(10, { start: 5, end: 15 });

      _resetForTesting();

      const state = _getStateForTesting();
      expect(state.identity).toBeNull();
      expect(state.currentFilePath).toBeNull();
      expect(state.localCursor).toBeNull();
      expect(state.localSelection).toBeNull();
      expect(state.remotePresences.size).toBe(0);
    });

    it('should generate new peer ID after reset', async () => {
      await initPresence();
      const peerId1 = getLocalPeerId();

      _resetForTesting();
      await initPresence();
      const peerId2 = getLocalPeerId();

      expect(peerId1).not.toBe(peerId2);
    });
  });
});
