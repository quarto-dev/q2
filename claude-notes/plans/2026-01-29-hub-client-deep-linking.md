# Hub-Client Deep Linking Plan

## Overview

Add deep linking support to hub-client to enable:
1. Opening documents in new browser tabs
2. Persisting file navigation state in URLs
3. Supporting browser back/forward navigation

## Key Constraints

**Privacy Concern**: The `indexDocId` (Automerge DocumentId) acts like a bearer token. Anyone with this ID can access the project. We must NOT store this in:
- Browser URL (appears in history, can be shared/leaked)
- Server logs (if we ever add analytics)
- Referrer headers (if linking to external sites)

## Current Architecture

### What Exists
- **Project identification**: `indexDocId` (bs58-encoded) stored in IndexedDB
- **File identification**: Path strings like `"index.qmd"`, `"docs/chapter1.qmd"`
- **No routing library**: Single SPA, no URL state management
- **Single file model**: One file open at a time, tracked in React state
- **Link interception**: Already extracts `{path, anchor}` from `.qmd` links

### What Doesn't Exist
- URL-based state persistence
- Browser history management
- Cross-tab communication for project context
- Multiple browser tab support

## Proposed Design

### URL Scheme

Use **fragment-only URLs** to avoid server logging:

```
https://hub.quarto.org/#/project/<local-project-id>/file/<encoded-path>
https://hub.quarto.org/#/project/<local-project-id>/file/<encoded-path>#<anchor>
```

Where:
- `<local-project-id>` is the IndexedDB `id` field (UUID), NOT the `indexDocId`
- `<encoded-path>` is URL-encoded file path
- `<anchor>` is optional section anchor

**Why local project ID instead of indexDocId?**
- Local IDs are only meaningful on the same browser/device
- They cannot be used to access the project from another context
- If a URL is accidentally shared, it reveals nothing useful

**Tradeoff**: URLs are not shareable across devices. This is acceptable because:
1. The `indexDocId` is essentially a secret - sharing it shares full access
2. Cross-device sharing would need explicit "share project" flow anyway
3. Local-only deep links still enable multi-tab workflows

### URL Routing Structure

```
#/                                    → Project selector
#/project/<id>                        → Project with default file
#/project/<id>/file/<path>            → Specific file
#/project/<id>/file/<path>#<anchor>   → Specific file + anchor
```

### State Flow

```
URL → State (on load):
1. Parse hash fragment
2. Look up project in IndexedDB by local ID
3. If found: connect to sync server, navigate to file
4. If not found: show "project not found" error

State → URL (on navigation):
1. User changes file → update URL with replaceState (no history entry)
2. User clicks internal link → update URL with pushState (adds history)
3. Browser back/forward → parse URL, navigate to file
```

### History Strategy

- **File changes via sidebar**: Use `replaceState` (no history entry)
- **File changes via link click**: Use `pushState` (adds history entry)
- **Anchor navigation**: Use `pushState` (adds history entry)
- **Project switch**: Use `replaceState` (clean slate)

This mimics VS Code behavior where explicit navigation (clicking links) adds history, but browsing files doesn't.

### Multi-Tab Support

**Option A: Independent Tabs (Recommended)**

Each browser tab is independent:
- Opens with URL → looks up project locally → connects to sync server
- Multiple tabs can view same project (Automerge handles sync)
- No cross-tab communication needed
- Simple implementation

**Option B: Coordinated Tabs (Future Enhancement)**

Use `BroadcastChannel` API for:
- Syncing presence across tabs
- Avoiding duplicate sync connections
- Shared project state

Recommend starting with Option A.

### Opening in New Tab

Add "Open in New Tab" action:
1. Right-click file in sidebar → "Open in New Tab"
2. Ctrl/Cmd+click on file → opens in new tab
3. Constructs URL: `#/project/<local-id>/file/<path>`
4. Opens via `window.open(url, '_blank')`

New tab flow:
1. Loads hub-client
2. Parses URL fragment
3. Finds project in IndexedDB
4. Connects to sync server
5. Navigates to specified file

### Error Handling

| Scenario | Behavior |
|----------|----------|
| Project ID not in IndexedDB | Show "Project not found. It may have been deleted." + link to project selector |
| File not in project | Show project, display "File not found" message, navigate to default file |
| Malformed URL | Redirect to project selector |
| No URL fragment | Show project selector |

### Migration

For users with existing sessions:
- If URL has no fragment and a project was previously selected, go to project selector
- No automatic restoration of previous session (intentional - avoids surprises)

## Implementation Plan

### Phase 1: URL Routing Foundation ✅

- [x] Create URL parsing utilities (`parseHashRoute`, `buildHashRoute`) - `src/utils/routing.ts`
- [x] Create URL generation utilities for building deep links - `src/utils/routing.ts`
- [x] Add hash change event listener in App.tsx - via `useRouting` hook
- [x] Implement basic routing: `#/` → project selector, `#/project/<id>` → project

### Phase 2: File Navigation URLs ✅

- [x] Update file navigation to use `pushState`/`replaceState`
  - Sidebar selection uses `replaceState` (no history entry)
  - Link clicks in preview use `pushState` (adds history)
- [x] Handle browser back/forward for file changes - via effect in Editor.tsx
- [x] Add anchor support to URLs - passed through from Preview
- [x] Integrate with existing link interception - updated `onFileChange` prop to include anchor

### Phase 3: Multi-Tab Support ✅

- [x] Add "Open in New Tab" to file context menu in sidebar
- [x] Add Ctrl/Cmd+click handler for sidebar files
- [x] Handle new tab startup from URL (completed in Phase 1/2)
- [x] Multi-tab scenarios: Each tab operates independently with Automerge sync

### Phase 4: Polish & Error Handling ✅

- [x] Add error states for missing projects/files - handled via `connectionError` state in App.tsx
- [x] Loading states during project connection - already existed via `isConnecting` state
- [x] Update document title to include project/file name - format: `filename — project — Quarto Hub`
- [x] Added "Copy Link" action to file context menu

## Technical Details

### URL Parsing

```typescript
interface DeepLinkRoute {
  type: 'project-selector' | 'project' | 'file';
  projectId?: string;       // Local IndexedDB ID
  filePath?: string;        // Decoded file path
  anchor?: string;          // Section anchor without #
}

function parseHashRoute(hash: string): DeepLinkRoute {
  // Parse #/project/<id>/file/<path>#<anchor>
}

function buildHashRoute(route: DeepLinkRoute): string {
  // Build URL fragment from route
}
```

### Browser History Integration

```typescript
// On file navigation
function navigateToFile(path: string, options?: { addToHistory?: boolean }) {
  const route = buildHashRoute({
    type: 'file',
    projectId: currentProject.id,
    filePath: path,
  });

  if (options?.addToHistory) {
    history.pushState({ route }, '', route);
  } else {
    history.replaceState({ route }, '', route);
  }

  // Update React state
  setCurrentFile(files.find(f => f.path === path));
}

// Handle back/forward
window.addEventListener('popstate', (event) => {
  const route = parseHashRoute(location.hash);
  if (route.type === 'file') {
    setCurrentFile(files.find(f => f.path === route.filePath));
  }
});
```

### New Tab Opening

```typescript
function openFileInNewTab(file: FileEntry) {
  const route = buildHashRoute({
    type: 'file',
    projectId: project.id,
    filePath: file.path,
  });

  window.open(window.location.origin + window.location.pathname + route, '_blank');
}
```

## Security Considerations

1. **Local IDs only**: Never put `indexDocId` in URLs
2. **No server-side routing**: All routing is client-side via hash fragments
3. **IndexedDB lookup required**: URL alone cannot access project data
4. **Referrer protection**: Hash fragments are not sent in Referrer headers

## Future Considerations

- **Shareable links**: Would need explicit "share" flow with proper authorization
- **Workspace URLs**: For multi-project scenarios
- **Editor state in URL**: Cursor position, selection (probably not worth it)
- **Sidebar state**: Which sections are expanded (probably not worth it)

## Related Files

Key files that will need modification:
- `hub-client/src/App.tsx` - Add routing logic
- `hub-client/src/components/Editor.tsx` - URL updates on file change
- `hub-client/src/components/FileSidebar.tsx` - "Open in New Tab" action
- `hub-client/src/services/iframePostProcessor.ts` - Integrate with link handling
- New: `hub-client/src/utils/routing.ts` - URL parsing/building utilities
