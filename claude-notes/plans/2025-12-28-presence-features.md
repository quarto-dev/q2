# Presence Features for Quarto Hub

**Beads Issue:** `k-evpj` - Add presence features (cursors, selections) to quarto-hub

## Overview

This plan outlines the implementation of collaborative presence features for quarto-hub, allowing users to see the cursors and text selections of other collaborators in real-time.

## Research Summary

### Monaco Editor Presence APIs

The [Monaco Collab Ext](https://github.com/convergencelabs/monaco-collab-ext) library provides purpose-built components for collaborative editing:

| Component | Purpose |
|-----------|---------|
| `RemoteCursorManager` | Renders remote user cursors with colored caret lines and tooltips showing user names |
| `RemoteSelectionManager` | Highlights text selections from remote users with colored backgrounds |
| `EditorContentManager` | Handles content synchronization (not needed - we already have this via Automerge) |

**API Example:**
```typescript
const cursorMgr = new MonacoCollabExt.RemoteCursorManager({
  editor: monacoEditor,
  tooltips: true,
  tooltipDuration: 2
});

const cursor = cursorMgr.addCursor("user-id", "#3498db", "Alice");
cursor.setOffset(142);  // Linear character offset
cursor.show();
```

**Key Features:**
- Supports both linear offsets and `{lineNumber, column}` positions
- Cursor tooltips show username on hover
- Colors are fully customizable per-user
- Clean `dispose()` method for cleanup

### Automerge Ephemeral Messaging

Automerge provides [ephemeral data](https://automerge.org/docs/reference/repositories/ephemeral/) specifically designed for transient state like presence:

```typescript
// Broadcasting presence
handle.broadcast({
  type: 'cursor',
  userId: 'abc123',
  position: 142,
  selection: { start: 142, end: 150 }
});

// Receiving presence
handle.on('ephemeral-message', (message) => {
  // message is automatically CBOR-decoded
  console.log('Received presence:', message);
});
```

**Key Properties:**
- Messages are CBOR-encoded (efficient binary format)
- NOT persisted to the document (perfect for ephemeral state)
- Associated with a specific `DocHandle`
- Automatically distributed to all connected peers

### Current Architecture Integration Points

The existing codebase provides excellent foundations:

| File | Relevant Code |
|------|--------------|
| `Editor.tsx:126` | `editorRef` - Monaco editor instance reference |
| `Editor.tsx:293-295` | `handleEditorMount` - Captures editor on mount |
| `automergeSync.ts:31-32` | `fileHandles: Map<string, DocHandle>` - Per-file document handles |
| `automergeSync.ts:8` | Already imports `DocHandle` from automerge-repo |

## Proposed Architecture

### Presence Message Protocol

```typescript
interface PresenceMessage {
  type: 'presence';
  userId: string;
  userName: string;
  userColor: string;
  filePath: string;
  cursor: number | null;           // Linear offset, null if no cursor
  selection: {                     // null if no selection
    start: number;
    end: number;
  } | null;
  timestamp: number;               // For stale presence detection
}

interface PresenceLeaveMessage {
  type: 'presence-leave';
  userId: string;
}
```

### Component Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                          Editor.tsx                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │
│  │  Monaco Editor  │  │ RemoteCursor    │  │ RemoteSelection │  │
│  │                 │  │ Manager         │  │ Manager         │  │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘  │
│           │                    │                    │            │
│           └──────────┬─────────┴────────────────────┘            │
│                      ▼                                           │
│           ┌─────────────────────┐                               │
│           │  usePresence Hook   │                               │
│           │  (new)              │                               │
│           └──────────┬──────────┘                               │
└──────────────────────┼──────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────────┐
│                    presenceService.ts (new)                       │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ - Broadcasts local cursor/selection changes                 │  │
│  │ - Listens for ephemeral messages on file DocHandles        │  │
│  │ - Maintains map of active users and their presence state   │  │
│  │ - Handles user join/leave detection                        │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────────────┐
│                      automergeSync.ts                             │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ DocHandle.broadcast() ◄──► ephemeral-message events        │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Infrastructure Setup

#### 1.1 Add Dependencies
```bash
npm install @convergencelabs/monaco-collab-ext
```

#### 1.2 Create User Identity Service
Create `hub-client/src/services/userIdentity.ts`:
- Generate or retrieve persistent user ID (localStorage)
- Generate random display name for anonymous users
- Assign consistent user color based on user ID hash

```typescript
interface UserIdentity {
  id: string;
  name: string;
  color: string;
}

export function getUserIdentity(): UserIdentity;
export function generateUserColor(userId: string): string;
```

### Phase 2: Presence Service

#### 2.1 Create Presence Service
Create `hub-client/src/services/presenceService.ts`:

**Core responsibilities:**
- Subscribe to ephemeral messages on file DocHandles
- Broadcast local presence changes (throttled to ~50ms)
- Maintain presence state: `Map<userId, PresenceState>`
- Detect stale presence (e.g., >5 seconds without update → assume disconnected)
- Emit presence change events

```typescript
interface PresenceState {
  userId: string;
  userName: string;
  userColor: string;
  filePath: string;
  cursor: number | null;
  selection: { start: number; end: number } | null;
  lastSeen: number;
}

// Public API
export function initPresence(fileHandles: Map<string, DocHandle>): void;
export function updateLocalPresence(filePath: string, cursor: number, selection?: Range): void;
export function onPresenceChange(callback: (presences: PresenceState[]) => void): () => void;
export function cleanup(): void;
```

#### 2.2 Integrate with automergeSync.ts
- Expose `fileHandles` for presence service to subscribe to
- Add cleanup coordination on disconnect

### Phase 3: UI Integration

#### 3.1 Create usePresence Hook
Create `hub-client/src/hooks/usePresence.ts`:

```typescript
interface UsePresenceResult {
  otherUsers: PresenceState[];
  updateCursor: (offset: number) => void;
  updateSelection: (start: number, end: number) => void;
}

export function usePresence(
  editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor>,
  currentFilePath: string
): UsePresenceResult;
```

**Responsibilities:**
- Track local cursor position changes via Monaco events
- Debounce/throttle updates
- Manage cursor/selection managers lifecycle
- Filter presence to current file only

#### 3.2 Update Editor.tsx
- Import and use `usePresence` hook
- Initialize `RemoteCursorManager` and `RemoteSelectionManager` on editor mount
- Render remote cursors/selections based on presence state
- Subscribe to Monaco cursor/selection events
- Clean up on unmount or file switch

### Phase 4: Polish

#### 4.1 User Experience
- Add user list/indicator showing who's online
- Show user count in header
- Smooth cursor animation (Monaco supports this natively)
- Graceful handling of network disconnection

#### 4.2 Performance Optimization
- Throttle presence broadcasts (50-100ms)
- Batch presence updates from multiple users
- Stale presence cleanup interval (every 10 seconds)
- Limit presence tracking to 20 concurrent users

## File Changes Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `package.json` | Modify | Add `@convergencelabs/monaco-collab-ext` dependency |
| `src/services/userIdentity.ts` | Create | User ID, name, color management |
| `src/services/presenceService.ts` | Create | Ephemeral messaging and presence state |
| `src/hooks/usePresence.ts` | Create | React hook for presence integration |
| `src/components/Editor.tsx` | Modify | Integrate presence hook, render cursors |
| `src/services/automergeSync.ts` | Modify | Expose file handles for presence |
| `src/types/presence.ts` | Create | TypeScript interfaces for presence |

## Open Questions for Discussion

1. **User Identity UX**: Should we prompt users for a display name, or use auto-generated names? Consider adding this to project settings later.

2. **File-scoped vs. Project-scoped Presence**: Current design scopes presence to the currently-viewed file. Should we also show an indicator of which file each user is editing?

3. **Cursor vs. Selection Priority**: When a user has both a cursor and selection, should we show both, or just the selection? Monaco Collab Ext supports both simultaneously.

4. **Offline/Reconnection Handling**: How should presence behave when a user temporarily disconnects? Options:
   - Immediate removal (current default)
   - Grace period (e.g., 30 seconds)
   - Show as "away" state

5. **Color Assignment**: Should colors be:
   - Deterministic based on user ID hash (consistent across sessions)
   - Randomly assigned per session (avoids color collisions)
   - User-selectable (stored in user profile)

## Testing Strategy

1. **Unit Tests**:
   - Presence message serialization/deserialization
   - Color generation from user ID
   - Stale presence detection logic

2. **Integration Tests**:
   - Multi-tab testing in same browser
   - Cursor position synchronization accuracy

3. **Manual Testing**:
   - Open same project in multiple browsers
   - Verify cursor rendering matches position
   - Test selection highlighting
   - Verify cleanup on disconnect

## References

- [Monaco Collab Ext GitHub](https://github.com/convergencelabs/monaco-collab-ext)
- [Monaco Collab Ext npm](https://www.npmjs.com/package/@convergencelabs/monaco-collab-ext)
- [Automerge Ephemeral Data Docs](https://automerge.org/docs/reference/repositories/ephemeral/)
- [Automerge Repo GitHub](https://github.com/automerge/automerge-repo)
