# Rename UI Tweaks

## Overview

Two small UX fixes for the file rename flow in hub-client's FileSidebar component.

## Work Items

- [x] Fix: renaming to the same name should be a no-op (cancel), not trigger "file exists" error
- [x] Fix: rename input should start with full text selected so typing replaces the old name

## Details

### Bug 1: Same-name rename shows error

**Current behavior**: User clicks Rename, then clicks away (blur) without changing the name. `handleRenameSubmit` calls `onRenameFile(file, file.path)`, which calls `renameFile(oldPath, newPath)` in automergeSync. The backend throws "File already exists" because `oldPath === newPath` and the file exists at that path.

**Fix**: In `handleRenameSubmit` (FileSidebar.tsx line 187), check if the trimmed value equals the original path. If so, just cancel (clear state) without calling `onRenameFile`.

**Location**: `hub-client/src/components/FileSidebar.tsx`, `handleRenameSubmit` callback (~line 187).

### Bug 2: Cursor starts at end of filename

**Current behavior**: `startRename` calls `focus()` on the input, which places the cursor at the end. User must manually select all text before typing a new name.

**Fix**: After `focus()`, call `select()` on the input ref to select all text. This way the user can immediately type a new name (replacing the selection) or press Home/End to position the cursor.

**Location**: `hub-client/src/components/FileSidebar.tsx`, `startRename` callback (~line 179).
