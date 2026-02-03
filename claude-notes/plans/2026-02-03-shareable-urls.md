# Shareable Project URLs for hub-client

**Issue:** bd-8exa
**Status:** Implementation Complete - Awaiting Manual Testing

## Overview

Implement shareable URLs that allow users to share Quarto Hub projects with others. Currently, URLs use local IndexedDB UUIDs which are only meaningful on the same browser/device. Shareable URLs will use the automerge index document ID, enabling cross-device/cross-user sharing.

## Security Context

Automerge document IDs behave like **bearer tokens** - anyone with the ID can access the project. This creates several security considerations:

1. **Minimize URL exposure**: The indexDocId should appear in the URL only when copied/shared. After visiting, the URL should be replaced with a local ID-based URL.
2. **No browser history**: Use `replaceState()` to prevent the shareable URL from appearing in browser history/bookmarks.
3. **No logging**: Never log indexDocId values.

## Current Architecture

### URL Scheme (routing.ts)
```
#/                                    → Project selector
#/project/<local-id>                  → Project with default file
#/project/<local-id>/file/<path>      → Specific file
#/project/<local-id>/file/<path>#<a>  → File + anchor
```

### Key Components
- **routing.ts**: URL parsing/building, Route types
- **useRouting.ts**: React hook for navigation
- **App.tsx**: Route resolution, project loading on URL change
- **projectStorage.ts**: IndexedDB operations, has `getProjectByIndexDocId()`
- **ProjectSelector.tsx**: "Connect to existing project" flow

## Proposed URL Scheme

### Shareable URL Format
```
#/share/<indexDocId>?server=<syncServer>&file=<path>
```

- `indexDocId`: bs58-encoded automerge document ID (without `automerge:` prefix for URL brevity)
- `server`: Sync server URL (**always included** for explicitness)
- `file`: Current file path (**always included** when copying from Editor)

### Why use query parameters?
- Keeps the URL structure simple
- Allows adding optional parameters without complicating the path
- Server URL can contain special characters that are easier to encode as a query param

## User Flows

### Flow 1: Copy Shareable Link

1. User is in a project, clicks "Share" button (new UI element in Editor header)
2. **Share modal dialog opens** with:
   - Warning message: "Anyone with this link can access and edit this project permanently."
   - Read-only text field showing the shareable URL
   - "Copy Link" button
   - "Cancel" button
3. User clicks "Copy Link"
4. URL is copied to clipboard
5. Toast notification: "Link copied to clipboard"
6. Modal closes (or user can close manually)

### Flow 2: Open Shareable Link (Existing Project)

1. User visits shareable URL
2. App parses indexDocId from URL
3. App looks up project by indexDocId using `getProjectByIndexDocId()`
4. Project found → Redirect to local ID-based URL (using `replaceState`)
5. Connect to project and display

### Flow 3: Open Shareable Link (New Project)

1. User visits shareable URL
2. App parses indexDocId from URL
3. `getProjectByIndexDocId()` returns undefined
4. App shows "Connect to shared project" dialog (pre-filled with indexDocId and server)
5. User confirms and provides optional description
6. System creates local project entry with generated UUID
7. Redirect to local ID-based URL (using `replaceState`)
8. Connect to project and display

### Flow 4: Invalid/Unreachable Shared Project

1. User visits shareable URL
2. App parses indexDocId from URL
3. Either:
   - Project not found locally AND connection fails → Show error, offer retry
   - Server unreachable → Show error, stay on project selector

## Implementation Plan

### Phase 1: Route Infrastructure

- [x] Add `ShareRoute` type to routing.ts
- [x] Update `parseHashRoute()` to recognize `#/share/<indexDocId>` pattern
- [x] Parse query parameters (server, file)
- [x] Add `buildShareableUrl()` function
- [x] Add tests for new routing functions

### Phase 2: Share Link Resolution

- [x] Create `useShareLinkResolver` hook (or integrate into App.tsx)
- [x] On detecting ShareRoute:
  - Extract indexDocId and server from URL
  - Immediately replace URL with `#/` (using replaceState) to clear sensitive data
  - Look up project by indexDocId
  - If found: navigate to local URL
  - If not found: show connect dialog
- [x] Handle the file path redirect (if provided in shareable URL)

### Phase 3: Connect Dialog for Shared Projects

- [x] Create `ShareConnectDialog` component (or extend ProjectSelector)
- [x] Pre-fill indexDocId and server from shareable URL
- [x] User provides description (optional)
- [x] On confirm: create project entry, connect, navigate to local URL
- [x] On cancel: navigate to project selector

### Phase 4: Share Dialog UI

- [x] Create `ShareDialog` component (modal)
  - Warning text about permanent access
  - Read-only text field with shareable URL
  - "Copy Link" button
  - "Cancel" button
- [x] Add "Share" button to Editor header (opens ShareDialog)
- [x] Build shareable URL including:
  - indexDocId (from project)
  - server (from project)
  - file (current file path)
- [x] Copy to clipboard using Clipboard API (with fallback)
- [ ] Show toast notification on successful copy (dialog auto-closes after copy)
- [x] Style the modal consistent with existing dialogs (ProjectSelector, NewFileDialog)

### Phase 5: Edge Cases and Polish

- [x] Handle URL with invalid indexDocId format (invalid formats fail at connection time - acceptable)
- [x] Handle connection failures gracefully (already implemented via setConnectionError)
- [x] Add loading state during share link resolution (already implemented via setIsConnecting)
- [x] Ensure no sensitive data in console logs (removed indexDocId from console.log)
- [ ] Test browser history behavior (back/forward with shareable links) - manual testing needed

## Technical Details

### ShareRoute Type
```typescript
export interface ShareRoute {
  type: 'share';
  indexDocId: string;      // Without 'automerge:' prefix
  syncServer: string;      // Always required in generated URLs
  filePath?: string;       // File to open (always included when copying)
}
```

### URL Examples
```
// Full URL (always generated this way)
#/share/4XyZabc123...?server=wss%3A%2F%2Fsync.automerge.org&file=docs%2Fintro.qmd

// When parsing, server defaults to wss://sync.automerge.org if somehow missing
```

### ShareDialog Component Props
```typescript
interface ShareDialogProps {
  isOpen: boolean;
  onClose: () => void;
  shareableUrl: string;
  onCopied?: () => void;   // Callback after successful copy
}
```

### State Machine for Share Link Resolution

```
[URL with ShareRoute]
         ↓
[Clear URL immediately (replaceState to #/)]
         ↓
[Look up by indexDocId]
         ↓
    ┌────┴────┐
    ↓         ↓
 [Found]   [Not Found]
    ↓         ↓
[Navigate  [Show Connect
 to local   Dialog]
 URL]         ↓
    ↓      [User confirms]
    └────────→↓
        [Navigate to local URL]
        [Connect to project]
```

## Design Decisions

1. **Server in URLs**: Always include the server URL, even when it's the default.
   - Simplifies the "connect to existing project" flow
   - URLs are explicit and future-proof

2. **File path in URL**: Always include the current file path.
   - Recipients open directly to the relevant file
   - They can still navigate to other files

3. **Permanent access warning**: Show a modal dialog with friction before copying.
   - Warning text: "Anyone with this link can access and edit this project permanently."
   - User must explicitly click "Copy Link" in the modal
   - Prevents accidental sharing

4. **Clipboard permissions**: Use `navigator.clipboard.writeText()` with fallback to `document.execCommand('copy')` for older browsers/HTTP contexts.

## Testing Strategy

### Unit Tests (routing.ts)
- Parse shareable URLs with various parameter combinations
- Build shareable URLs from project data
- Edge cases: missing params, special characters, URL encoding

### Integration Tests
- Share link resolution: existing project found
- Share link resolution: new project flow
- URL replacement (no sensitive data in history)
- Clipboard operations

### Manual Testing
- Copy link in one browser, open in another (incognito)
- Copy link, close browser, reopen and paste
- Verify browser history doesn't contain indexDocId

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| IndexDocId leaked via referrer header | Shareable URLs are hash-based (#/share/...) which browsers typically don't send in Referer headers |
| User bookmarks shareable URL before redirect | Replace URL immediately in synchronous code path |
| Clipboard API not available | Provide fallback mechanism |
| Server URL contains special characters | URL-encode the server parameter |
| User accidentally shares sensitive project | Modal dialog with warning adds friction before copying |
| User doesn't understand permanence of sharing | Clear warning text in modal: "permanently" |
