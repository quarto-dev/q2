/**
 * Presence Service
 *
 * Manages real-time presence (cursors, selections) for collaborative editing.
 * Uses Automerge's ephemeral messaging to broadcast and receive presence state.
 */

import type { DocHandle, DocHandleEphemeralMessagePayload } from '@automerge/automerge-repo';
import { next as A } from '@automerge/automerge';
import { getFileHandle } from '@quarto/preview-runtime';
import { getUserIdentity } from './userSettings';
import type { UserSettings } from './storage/types';

/**
 * Presence state for a remote user.
 *
 * `cursor` and `selection` carry Automerge cursor strings
 * (see `A.getCursor` / `A.getCursorPosition`) — opaque tokens anchored to
 * the current file's `['text']` sequence. The receiver resolves them to
 * numeric offsets against its own local doc; this is what keeps remote
 * cursors stable under concurrent edits without hand-rolled OT.
 */
export interface PresenceState {
  peerId: string;
  userId: string;
  userName: string;
  userColor: string;
  filePath: string;
  cursor: string | null;
  selection: { start: string; end: string } | null;
  lastSeen: number;
}

/**
 * Presence message broadcast via ephemeral messaging. Same wire shape as
 * {@link PresenceState}'s cursor fields — see PresenceState doc comment.
 */
interface PresenceMessage {
  type: 'presence';
  peerId: string;
  userId: string;
  userName: string;
  userColor: string;
  cursor: string | null;
  selection: { start: string; end: string } | null;
}

/**
 * Leave message broadcast when user leaves a file or disconnects.
 */
interface PresenceLeaveMessage {
  type: 'leave';
  peerId: string;
}

type EphemeralMessage = PresenceMessage | PresenceLeaveMessage;

/**
 * Callback for presence changes.
 */
type PresenceChangeCallback = (presences: PresenceState[]) => void;

/**
 * Configuration options for the presence service.
 */
interface PresenceConfig {
  /** How often to broadcast presence updates (ms). Default: 50 */
  broadcastThrottleMs: number;
  /** How long before a user is considered stale (ms). Default: 5000 */
  staleThresholdMs: number;
  /** How often to clean up stale presences (ms). Default: 2000 */
  cleanupIntervalMs: number;
}

const DEFAULT_CONFIG: PresenceConfig = {
  broadcastThrottleMs: 50,
  staleThresholdMs: 5000,
  cleanupIntervalMs: 2000,
};

/**
 * Internal state for the presence service.
 */
interface PresenceServiceState {
  // Our identity
  peerId: string;
  identity: UserSettings | null;

  // Current file we're tracking presence for
  currentFilePath: string | null;
  currentHandle: DocHandle<unknown> | null;

  // Remote presences for current file
  remotePresences: Map<string, PresenceState>;

  // Our current cursor/selection state
  localCursor: number | null;
  localSelection: { start: number; end: number } | null;

  // Throttling
  lastBroadcastTime: number;
  pendingBroadcast: ReturnType<typeof setTimeout> | null;

  // Cleanup interval
  cleanupInterval: ReturnType<typeof setInterval> | null;

  // Subscribers
  subscribers: Set<PresenceChangeCallback>;

  // Event listener cleanup
  messageHandler: ((payload: DocHandleEphemeralMessagePayload<unknown>) => void) | null;
  visibilityHandler: (() => void) | null;
  focusHandler: (() => void) | null;

  // Visibility gate: set while hidden by `notifySubscribers`; flushed by
  // the visibilitychange/focus handlers. See
  // claude-notes/plans/2026-04-24-hub-client-visibility-gating.md.
  pendingNotify: boolean;

  // Configuration
  config: PresenceConfig;
}

const state: PresenceServiceState = {
  peerId: crypto.randomUUID(),
  identity: null,
  currentFilePath: null,
  currentHandle: null,
  remotePresences: new Map(),
  localCursor: null,
  localSelection: null,
  lastBroadcastTime: 0,
  pendingBroadcast: null,
  cleanupInterval: null,
  subscribers: new Set(),
  messageHandler: null,
  visibilityHandler: null,
  focusHandler: null,
  pendingNotify: false,
  config: DEFAULT_CONFIG,
};

/**
 * Initialize the presence service.
 * Must be called before using other presence functions.
 */
export async function initPresence(config?: Partial<PresenceConfig>): Promise<void> {
  // Merge config
  if (config) {
    state.config = { ...DEFAULT_CONFIG, ...config };
  }

  // Load user identity
  state.identity = await getUserIdentity();

  // Start cleanup interval
  if (state.cleanupInterval) {
    clearInterval(state.cleanupInterval);
  }
  state.cleanupInterval = setInterval(cleanupStalePresences, state.config.cleanupIntervalMs);

  // Attach visibility + focus listeners. Both flush via the same
  // idempotent helper, so double-firing is harmless. `focus` covers the
  // documented cases where visibilitychange doesn't fire (Chrome/Edge
  // DevTools "Emulate a focused page"; Firefox macOS Cmd-H → Cmd-Tab;
  // headless Chrome on tab switch; Brave/Chrome on HTTP with DevTools).
  attachVisibilityListeners();
}

/**
 * Clean up the presence service.
 * Call this when disconnecting or unmounting.
 */
export function cleanupPresence(): void {
  // Broadcast leave message if we have a current file
  if (state.currentHandle) {
    broadcastLeave();
  }

  // Stop listening to current handle
  stopListening();

  // Detach visibility + focus listeners
  detachVisibilityListeners();

  // Clear cleanup interval
  if (state.cleanupInterval) {
    clearInterval(state.cleanupInterval);
    state.cleanupInterval = null;
  }

  // Clear pending broadcast
  if (state.pendingBroadcast) {
    clearTimeout(state.pendingBroadcast);
    state.pendingBroadcast = null;
  }

  // Clear state
  state.currentFilePath = null;
  state.currentHandle = null;
  state.remotePresences.clear();
  state.localCursor = null;
  state.localSelection = null;
  state.pendingNotify = false;

  // Notify subscribers
  notifySubscribers();
}

/**
 * Set the current file for presence tracking.
 * Call this when the user switches to editing a different file.
 */
export function setCurrentFile(filePath: string | null): void {
  // If same file, do nothing
  if (filePath === state.currentFilePath) {
    return;
  }

  // Broadcast leave on old file
  if (state.currentHandle) {
    broadcastLeave();
    stopListening();
  }

  // Clear remote presences when switching files
  state.remotePresences.clear();
  state.localCursor = null;
  state.localSelection = null;

  // Set new file
  state.currentFilePath = filePath;

  if (filePath) {
    const handle = getFileHandle(filePath);
    if (handle) {
      state.currentHandle = handle;
      startListening();
    } else {
      state.currentHandle = null;
    }
  } else {
    state.currentHandle = null;
  }

  // Notify subscribers
  notifySubscribers();
}

/**
 * Update the local cursor position.
 * This will be broadcast to other users (throttled).
 */
export function updateCursor(offset: number | null): void {
  state.localCursor = offset;
  scheduleBroadcast();
}

/**
 * Update the local selection range.
 * This will be broadcast to other users (throttled).
 */
export function updateSelection(selection: { start: number; end: number } | null): void {
  state.localSelection = selection;
  scheduleBroadcast();
}

/**
 * Update both cursor and selection at once.
 * This is more efficient than calling updateCursor and updateSelection separately.
 */
export function updatePresence(
  cursor: number | null,
  selection: { start: number; end: number } | null
): void {
  state.localCursor = cursor;
  state.localSelection = selection;
  scheduleBroadcast();
}

/**
 * Get the current remote presences for the current file.
 */
export function getRemotePresences(): PresenceState[] {
  return Array.from(state.remotePresences.values());
}

/**
 * Subscribe to presence changes.
 * Returns an unsubscribe function.
 */
export function onPresenceChange(callback: PresenceChangeCallback): () => void {
  state.subscribers.add(callback);

  // Immediately call with current state
  callback(getRemotePresences());

  return () => {
    state.subscribers.delete(callback);
  };
}

/**
 * Get the current user's identity.
 */
export function getLocalIdentity(): UserSettings | null {
  return state.identity;
}

/**
 * Refresh the local identity from storage.
 * Call this after the user updates their name or color.
 */
export async function refreshIdentity(): Promise<void> {
  state.identity = await getUserIdentity();
  // Immediately broadcast updated identity
  broadcastPresence();
}

/**
 * Get the local peer ID.
 * This is unique per browser session.
 */
export function getLocalPeerId(): string {
  return state.peerId;
}

// Internal functions

function startListening(): void {
  if (!state.currentHandle) return;

  state.messageHandler = (payload: DocHandleEphemeralMessagePayload<unknown>) => {
    // The message is wrapped in a payload object
    const message = payload.message as EphemeralMessage;
    if (message && typeof message === 'object' && 'type' in message) {
      handleEphemeralMessage(message);
    }
  };

  // Listen for ephemeral messages on the document handle
  state.currentHandle.on('ephemeral-message', state.messageHandler);
}

function stopListening(): void {
  if (!state.currentHandle || !state.messageHandler) return;

  state.currentHandle.off('ephemeral-message', state.messageHandler);
  state.messageHandler = null;
}

function handleEphemeralMessage(message: EphemeralMessage): void {
  // Ignore our own messages
  if (message.peerId === state.peerId) {
    return;
  }

  if (message.type === 'leave') {
    // Remove the user from presences
    state.remotePresences.delete(message.peerId);
    notifySubscribers();
    return;
  }

  if (message.type === 'presence') {
    // Update or add the user's presence
    const presence: PresenceState = {
      peerId: message.peerId,
      userId: message.userId,
      userName: message.userName,
      userColor: message.userColor,
      filePath: state.currentFilePath!,
      cursor: message.cursor,
      selection: message.selection,
      lastSeen: Date.now(),
    };

    state.remotePresences.set(message.peerId, presence);
    notifySubscribers();
  }
}

function scheduleBroadcast(): void {
  const now = Date.now();
  const timeSinceLastBroadcast = now - state.lastBroadcastTime;

  // If enough time has passed, broadcast immediately
  if (timeSinceLastBroadcast >= state.config.broadcastThrottleMs) {
    broadcastPresence();
    return;
  }

  // Otherwise, schedule a broadcast if one isn't already pending
  if (!state.pendingBroadcast) {
    const delay = state.config.broadcastThrottleMs - timeSinceLastBroadcast;
    state.pendingBroadcast = setTimeout(() => {
      state.pendingBroadcast = null;
      broadcastPresence();
    }, delay);
  }
}

function broadcastPresence(): void {
  if (!state.currentHandle || !state.identity) {
    return;
  }

  // Convert the raw editor offsets we've been accumulating into Automerge
  // cursors, anchored to the current doc's `['text']` sequence. `doc()`
  // throws on deleted/unavailable handles; a throw here during a 50 ms
  // throttled tick should be a silent skip, not a crash.
  let cursor: string | null = null;
  let selection: { start: string; end: string } | null = null;
  try {
    const doc = state.currentHandle.doc();
    if (state.localCursor !== null) {
      cursor = A.getCursor(doc, ['text'], state.localCursor);
    }
    if (state.localSelection !== null) {
      selection = {
        start: A.getCursor(doc, ['text'], state.localSelection.start),
        end: A.getCursor(doc, ['text'], state.localSelection.end),
      };
    }
  } catch {
    return;
  }

  const message: PresenceMessage = {
    type: 'presence',
    peerId: state.peerId,
    userId: state.identity.userId,
    userName: state.identity.userName,
    userColor: state.identity.userColor,
    cursor,
    selection,
  };

  state.currentHandle.broadcast(message);
  state.lastBroadcastTime = Date.now();
}

function broadcastLeave(): void {
  if (!state.currentHandle) {
    return;
  }

  const message: PresenceLeaveMessage = {
    type: 'leave',
    peerId: state.peerId,
  };

  state.currentHandle.broadcast(message);
}

function cleanupStalePresences(): void {
  const now = Date.now();
  let changed = false;

  for (const [peerId, presence] of state.remotePresences) {
    if (now - presence.lastSeen > state.config.staleThresholdMs) {
      state.remotePresences.delete(peerId);
      changed = true;
    }
  }

  if (changed) {
    notifySubscribers();
  }
}

function notifySubscribers(): void {
  // Visibility gate: while the tab is hidden, flag a pending notify and
  // coalesce to a single callback fire on visibilitychange → visible (or
  // window focus). Data stays fresh in `remotePresences` — only the
  // subscriber invocation is deferred, which is what drives the Monaco
  // decoration recompute in `usePresence`.
  if (typeof document !== 'undefined' && document.visibilityState === 'hidden') {
    state.pendingNotify = true;
    return;
  }
  doNotifySubscribers();
}

function doNotifySubscribers(): void {
  const presences = getRemotePresences();
  for (const callback of state.subscribers) {
    callback(presences);
  }
}

function flushPendingNotify(): void {
  if (!state.pendingNotify) return;
  state.pendingNotify = false;
  doNotifySubscribers();
}

function attachVisibilityListeners(): void {
  if (typeof document === 'undefined') return;

  // Re-attach cleanly even if initPresence is called twice (tests do this).
  detachVisibilityListeners();

  state.visibilityHandler = () => {
    if (document.visibilityState === 'visible') {
      flushPendingNotify();
    }
  };
  state.focusHandler = () => {
    flushPendingNotify();
  };
  document.addEventListener('visibilitychange', state.visibilityHandler);
  window.addEventListener('focus', state.focusHandler);
}

function detachVisibilityListeners(): void {
  if (typeof document === 'undefined') return;
  if (state.visibilityHandler) {
    document.removeEventListener('visibilitychange', state.visibilityHandler);
    state.visibilityHandler = null;
  }
  if (state.focusHandler) {
    window.removeEventListener('focus', state.focusHandler);
    state.focusHandler = null;
  }
}

// ============================================================================
// Testing Utilities
// ============================================================================

/**
 * Reset the presence service state for testing.
 *
 * This function resets all module-level state to initial values,
 * ensuring test isolation. Call this in beforeEach() to prevent
 * state leakage between tests.
 *
 * @internal For testing only - not part of the public API
 */
export function _resetForTesting(): void {
  // Clean up any active subscriptions
  cleanupPresence();

  // Reset state to initial values
  state.peerId = crypto.randomUUID();
  state.identity = null;
  state.currentFilePath = null;
  state.currentHandle = null;
  state.remotePresences = new Map();
  state.localCursor = null;
  state.localSelection = null;
  state.lastBroadcastTime = 0;
  state.pendingBroadcast = null;
  state.cleanupInterval = null;
  state.subscribers = new Set();
  state.messageHandler = null;
  state.visibilityHandler = null;
  state.focusHandler = null;
  state.pendingNotify = false;
  state.config = { ...DEFAULT_CONFIG };
}

/**
 * Get the internal state for test assertions.
 *
 * @internal For testing only - not part of the public API
 */
export function _getStateForTesting(): Readonly<PresenceServiceState> {
  return state;
}
