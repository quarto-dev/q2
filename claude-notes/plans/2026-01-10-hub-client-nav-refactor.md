# Hub-Client Navigation Refactor Plan

**Issue:** k-wc81
**Created:** 2026-01-10
**Status:** Ready for Implementation

## Decisions Made

| Question | Decision |
|----------|----------|
| Current file indicator | Option B: Minimal top bar (breadcrumb style) |
| User count location | Status tab |
| Tab format | Text only (for now) |
| "Disconnect" button text | "Choose New Project" |

## Overview

Replace the top navigation bar in hub-client with a tabbed interface in the left pane. This consolidates navigation and settings into a more organized sidebar structure.

## Final Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│  docs/guide/intro.qmd                              (minimal top bar)     │
├───────────────────┬────────────────────────────┬─────────────────────────┤
│ Files | Project | │                            │                         │
│ Status | Settings │      Monaco Editor         │       Preview           │
│ | About           │                            │                         │
├───────────────────┤                            │                         │
│                   │                            │                         │
│  (tab content     │                            │                         │
│   area - e.g.     │                            │                         │
│   file tree when  │                            │                         │
│   Files selected) │                            │                         │
│                   │                            │                         │
│                   │                            │                         │
│                   │                            │                         │
│                   │                            │                         │
└───────────────────┴────────────────────────────┴─────────────────────────┘
```

The minimal top bar spans the full width and shows only the currently-edited file path. All other UI elements move into the sidebar tabs.

## Current Top Nav Bar Elements (Complete Audit)

The current top navigation bar (`Editor.tsx` lines 635-670) contains:

| Element | Location | Purpose |
|---------|----------|---------|
| Project title (h1) | `.project-info` | Display project name from `project.description` |
| WASM status indicator | `.status-indicators` | Shows "Loading WASM...", "Ready", or "WASM Error" |
| User count badge | `.status-indicators` | Shows collaborator count with tooltip of usernames |
| Current file indicator | `.current-file-indicator` | Shows path of open file |
| Scroll sync checkbox | `.toolbar-actions` | Toggle scroll synchronization |
| Disconnect button | `.toolbar-actions` | Return to project selector |

## Proposed Tab Structure

### Tab 1: Files (Current Sidebar)
- File tree with directory grouping
- New file button (+)
- Context menu (rename/delete)
- Drag-and-drop upload support

### Tab 2: Project
- Project name
- Index document ID
  - Displayed as truncated hash
  - Click to copy full automerge URL to clipboard
  - Visual feedback on copy (checkmark or brief "Copied!" text)
- "Choose New Project" button (replaces "Disconnect")

### Tab 3: Status
- WASM renderer status (Loading/Ready/Error)
- **User count indicator** (moved from top nav)
  - Number of collaborators
  - Tooltip or expandable section showing usernames
  - Consider showing user presence dots/avatars in future

### Tab 4: Settings
- Scroll sync toggle checkbox
- (Future: other user preferences)

### Tab 5: About Hub
- Commit indicator (same format as project selector)
- Links to:
  - Quarto Hub documentation
  - GitHub repository
  - Changelog (placeholder for now)
- Version information

## UI Element Placement Analysis

### Current File Indicator - Options

The user asked for input on where to place the currently-edited file indicator. Here are the options with pros/cons:

#### Option A: Files Tab Header (Recommended)
```
┌─────────────────────────────────┐
│ [Files] [Project] [Status] ... │
├─────────────────────────────────┤
│ ◆ Currently editing:            │
│   docs/guide/intro.qmd          │
├─────────────────────────────────┤
│ 📁 docs/                        │
│   📄 guide/intro.qmd  ←(active) │
│   📄 guide/setup.qmd            │
│ 📄 index.qmd                    │
└─────────────────────────────────┘
```
**Pros:**
- Contextually relevant (it's about files)
- Always visible when working with files
- Clear visual hierarchy

**Cons:**
- Takes vertical space in Files tab
- Redundant with the active file highlight in the tree

#### Option B: Minimal Top Bar (Breadcrumb Style)
```
┌─────────────────────────────────────────────────┐
│ docs/guide/intro.qmd                            │
├─────────────────────────────────────────────────┤
│ [Sidebar]  │  [Editor]  │  [Preview]            │
```
**Pros:**
- Always visible regardless of active tab
- Familiar breadcrumb pattern
- Doesn't take sidebar space

**Cons:**
- Keeps a top element (though minimal)
- May feel disconnected from sidebar reorganization

#### Option C: Editor Pane Header
```
┌──────────────────────────────────────────────────┐
│ Sidebar │      Editor                 │ Preview  │
│         │ ┌─────────────────────────┐ │          │
│ [Tabs]  │ │ docs/guide/intro.qmd    │ │          │
│         │ ├─────────────────────────┤ │          │
│ Files   │ │ (Monaco editor)         │ │          │
│ ...     │ │                         │ │          │
```
**Pros:**
- Directly associated with editor content
- Clear context for what you're editing

**Cons:**
- Takes vertical space from editor
- Need to style consistently with new tab system

#### Option D: Tab Badge / Icon Indicator
Show a small indicator on the Files tab itself when a file is open:
```
[Files •] [Project] [Status] ...
```
With the full path shown only when hovering or when Files tab is active.

**Pros:**
- Minimal space usage
- Unobtrusive

**Cons:**
- Less discoverable
- Requires interaction to see full path

### Recommendation

**Primary choice: Option B (Minimal Top Bar)** with a very slim breadcrumb-style display. This:
- Keeps the current file always visible (important for context)
- Allows the sidebar to focus on its new tab structure
- Maintains a consistent editing context across all tabs

**Alternative: Option C (Editor Pane Header)** if we want to fully eliminate the top bar. The current file is editing context, so it makes sense above the editor.

## Implementation Phases

### Phase 1: Tab Infrastructure
1. Create `SidebarTabs.tsx` component with tab switching logic
2. Define tab data structure and icons
3. Style tab buttons to match dark theme
4. Update `FileSidebar.tsx` to be a child component

### Phase 2: Move Existing Features
1. Move WASM status to Status tab
2. Move user count indicator to Status tab
3. Move scroll sync to Settings tab
4. Move disconnect to Project tab (rename to "Choose New Project")
5. Add project info to Project tab

### Phase 3: New Features
1. Implement index document ID display with copy-to-clipboard
2. Create About Hub tab with commit indicator
3. Add placeholder for changelog

### Phase 4: Minimal Top Bar for Current File
1. Replace full top nav with slim breadcrumb-style bar showing only current file path
2. Style to be minimal and unobtrusive
3. Adjust layout CSS to account for reduced header height

### Phase 5: Polish
1. Keyboard navigation between tabs
2. Tab persistence (remember last active tab)
3. Responsive behavior
4. Accessibility (ARIA labels, focus management)

## Technical Considerations

### State Management
- Tab selection state: local to sidebar component
- Current file: already managed in Editor.tsx
- Settings like scroll sync: may need to lift state or use context

### CSS Changes
- Repurpose `.editor-header` for minimal breadcrumb (slim height, just file path)
- Add tab button styles (text-only tabs)
- Ensure smooth transitions between tabs
- Maintain consistent dark theme colors

### Component Structure
```
Editor.tsx
├── MinimalHeader.tsx  // Slim bar showing current file path only
└── SidebarContainer.tsx
    ├── TabBar.tsx
    │   └── TabButton.tsx (x5)
    └── TabContent.tsx
        ├── FilesTab.tsx (current FileSidebar)
        ├── ProjectTab.tsx
        ├── StatusTab.tsx
        ├── SettingsTab.tsx
        └── AboutTab.tsx
```

## Resolved Questions

1. **Current file indicator placement** - Option B: Minimal top bar (breadcrumb style)
2. **User count location** - Status tab
3. **Tab icons vs text** - Text only for now
4. **"Disconnect" naming** - "Choose New Project"
5. **Tab order** - Files, Project, Status, Settings, About (approved)

## Files to Modify

- `hub-client/src/components/Editor.tsx` - Replace top nav with minimal header, integrate new sidebar
- `hub-client/src/components/Editor.css` - Update layout styles, slim header
- `hub-client/src/components/FileSidebar.tsx` - Refactor into tab content
- `hub-client/src/components/FileSidebar.css` - Refactor styles

**New files:**
- `hub-client/src/components/MinimalHeader.tsx` - Slim breadcrumb bar for current file
- `hub-client/src/components/MinimalHeader.css` - Minimal header styles
- `hub-client/src/components/SidebarTabs.tsx` - Tab container and switching logic
- `hub-client/src/components/SidebarTabs.css` - Tab styles
- `hub-client/src/components/tabs/ProjectTab.tsx` - Project info, doc ID, "Choose New Project"
- `hub-client/src/components/tabs/StatusTab.tsx` - WASM status, user count
- `hub-client/src/components/tabs/SettingsTab.tsx` - Scroll sync toggle
- `hub-client/src/components/tabs/AboutTab.tsx` - Commit indicator, links
