---
title: 'Share Link Single-Click Flow'
date: 2026-03-30
description: 'Makes a share link open its project in one click by creating the local project entry automatically when it is missing, and requires the project name in the share URL.'
---

## Overview

Currently, when a user receives a share link for a project they haven't previously connected to, the flow is interrupted: they land on the ProjectSelector with a pre-filled "Connect to Project" form, requiring them to click "Connect" before accessing the document. The goal is to make share links work in a single click regardless of whether the recipient already has the project in their local settings.

Additionally, the share URL currently lacks the project name, so when auto-creating a project entry for a new recipient, there's no human-readable name available.

## Share URL Format

Current:
```
#/share/<indexDocId>?server=<syncServer>&file=<filePath>
```

New:
```
#/share/<indexDocId>?server=<syncServer>&file=<filePath>&name=<projectName>
```

All three query parameters (`server`, `file`, `name`) are required. If any are missing, show a clear error message telling the user the share link is malformed.

## User Flow Matrix

### State 1: Auth enabled, user logged in, project exists locally

No change needed. Same one-click behavior.

Flow:
1. User clicks share link
2. `parseHashRoute` extracts share data
3. URL is immediately cleared (security)
4. `projectStorage.getProjectByIndexDocId()` finds existing project
5. Connect to Automerge, load files, navigate to shared file

### State 2: Auth enabled, user logged in, project does NOT exist locally

**Current**: Shows ProjectSelector with pre-filled "Connect to Shared Project" form. User must click "Connect".
**New**: Auto-create project entry and connect immediately. No intermediate form.

Flow:
1. User clicks share link
2. `parseHashRoute` extracts share data (including `name`)
3. URL is immediately cleared (security)
4. `projectStorage.getProjectByIndexDocId()` returns undefined
5. Auto-call `projectStorage.addProject(indexDocId, syncServer, name)`
6. Connect to Automerge, load files, navigate to shared file
7. If connection fails, show error on project selector

### State 3: Auth enabled, user NOT logged in, project exists locally

No change needed. The pre-auth hash preservation already handles this correctly.

Flow:
1. User clicks share link
2. Auth check fails → LoginScreen shown
3. `savePreAuthHash()` stores `#/share/...` in sessionStorage
4. User logs in via Google OAuth
5. Redirect back to app → `restorePreAuthHash()` restores the hash
6. Proceeds as State 1

### State 4: Auth enabled, user NOT logged in, project does NOT exist locally

**Current**: Login → "Connect to Shared Project" form → click "Connect".
**New**: Login → auto-create + connect.

Flow:
1. User clicks share link
2. Auth check fails → LoginScreen shown
3. `savePreAuthHash()` stores `#/share/...` in sessionStorage
4. User logs in via Google OAuth
5. Redirect back to app → `restorePreAuthHash()` restores the hash
6. Proceeds as State 2 (auto-create + connect)

### State 5: Auth disabled, project exists locally

No change needed. One-click already works.

### State 6: Auth disabled, project does NOT exist locally

**Current**: Shows "Connect to Shared Project" form.
**New**: Auto-create project entry and connect immediately.

Same as State 2, steps 1-7.

### Error state: Malformed share URL

If `server`, `file`, or `name` is missing from the share URL, navigate to the project selector and show an error: "This share link is incomplete. Please ask the sender to share a new link."

## Work Items

### Phase 1: URL changes

- [x] Add `name` field to `ShareRoute` interface in `routing.ts` (required `string`)
- [x] Update `parseHashRoute` to extract `name` query parameter; missing fields → empty strings, validated by App.tsx
- [x] Update `buildHashRoute` for share routes to include `name` parameter
- [x] Update `buildShareableUrl` to accept and include `projectName`
- [x] Update `Editor.tsx` to pass `project.description` to `buildShareableUrl`
- [x] Update `ShareDialog` to accept `string | undefined` for shareableUrl (no file → no URL)

### Phase 2: Auto-connect on share link

- [x] In `App.tsx` share link handler: validate that all required share fields are present; if not, show error on project selector
- [x] When project doesn't exist locally, auto-create via `projectStorage.addProject()` using the share data (indexDocId, syncServer, name), then connect
- [x] Remove `pendingShareData` state and all related plumbing from `App.tsx` and `ProjectSelector.tsx`

### Phase 3: Cleanup

- [x] Remove share-specific UI from `ProjectSelector.tsx` ("Connect to Shared Project" heading, hint text, `pendingShareData` props)
- [x] Keep the manual "Connect to Project" form for advanced use
- [x] Update all routing tests for new ShareRoute type (65 tests passing)

## Key Files to Modify

| File | Changes |
|------|---------|
| `src/utils/routing.ts` | Add `name` to ShareRoute, update parse/build functions |
| `src/App.tsx` | Auto-create project on share link, remove pendingShareData, add validation |
| `src/components/Editor.tsx` | Pass project name to `buildShareableUrl` |
| `src/components/ProjectSelector.tsx` | Remove pendingShareData handling |

## Design Decisions

1. **Auto-create, no confirmation**: The share link is already an implicit "I want to access this project" signal. No security benefit from a confirmation step (the indexDocId is the auth token).

2. **Required fields, no fallbacks**: Since hub-client is pre-alpha, all share URL fields are required. Malformed URLs get a clear error message rather than silent degradation.

3. **Error handling**: If auto-connect fails after creating the project entry, show an error on the project selector. The project entry remains in IndexedDB so the user can retry from their project list.
