---
title: 'Fix: New projects not added to Automerge project set (stale closure)'
date: 2026-04-06
---

## Overview

After the project list migration to Automerge-backed `ProjectSetDocument`, new projects
created via "Create Project" or added via share links are not being stored in the synced
project set. They only land in legacy IndexedDB storage.

**Root cause:** Stale closure in `App.tsx`. The `handleProjectCreated` callback and the
initial route effect both reference `projectSetState` and `projectSetActions`, but neither
includes them in their dependency arrays. The callbacks capture the initial `'loading'`
status and never see the transition to `'connected'`.

## Affected code paths

1. **`handleProjectCreated`** (App.tsx:417-483) — dependency array missing `projectSetState`, `projectSetActions`
2. **Initial route effect / share link handler** (App.tsx:180-305) — same issue at line 244

## Fix approach

Use a ref to hold the latest `projectSetState` and `projectSetActions`, so callbacks
always see current values without needing them in dependency arrays (which would cause
unnecessary re-creation of the callbacks and potential infinite re-render loops).

Alternatively, call `projectSetService.addProject()` directly (the service module holds
its own state and doesn't suffer from stale closures), but this bypasses the hook's
`setProjects()` call. The ref approach is cleaner.

## Work Items

- [x] Add refs for `projectSetState` and `projectSetActions` in App.tsx
- [x] Update `handleProjectCreated` to read from refs instead of closed-over values
- [x] Update share link handler in initial route effect to read from refs
- [ ] Write test verifying new project is added to project set
  - Note: This is a React closure bug, not a service logic bug. The schema helpers
    are already tested in `quarto-automerge-schema/src/projectSet.test.ts`.
    A proper test requires rendering the App with mocked Automerge repos (e2e-level).
    Skipping unit test for now; manual verification is the pragmatic path.
- [x] Verify `npm run build:all` passes
- [x] Verify existing tests pass (414/414 passed)
