---
title: 'Project Selector Loading UX Improvement'
date: 2026-04-07
---

## Overview

When hub-client connects to the project set sync server on startup, the user currently sees a blank white page with "Connecting to project set..." text. This is jarring — the user should see the full ProjectSelector UI with the connecting state shown inline where the project list would be.

## Current Behavior

In `App.tsx` lines 537-548, when `projectSetState.status` is `'loading'` or `'connecting'`, an early return renders a blank page with centered text. This blocks the entire ProjectSelector from rendering.

## Proposed Change

Remove the early-return gate in `App.tsx` for `loading`/`connecting` states and instead pass the project set status through to `ProjectSelector`, which will render the connecting message in the projects list area.

## Work Items

- [x] Add a `projectSetStatus` prop to `ProjectSelector` (type: `ProjectSetStatus` from `useProjectSet`)
- [x] In `App.tsx`, remove the early return for `loading`/`connecting` states (lines 537-548)
- [x] In `App.tsx`, render `ProjectSelector` for `loading`/`connecting` states (pass `projectSetEntries` as undefined, `projectSetStatus` with the current status)
- [x] In `ProjectSelector`, when `projectSetStatus` is `'loading'` or `'connecting'`, show an inline message in the `.projects-list` area instead of projects (e.g., "Connecting to project set..." with appropriate styling)
- [x] While connecting, hide action buttons, divider, and forms (Create New / Connect to Project) since they require the project set to be connected
- [x] Verify with `npm run build:all` from hub-client/
- [x] All hub-client tests pass (52 passed)
- [ ] Manual testing: confirm the ProjectSelector chrome (header, sign out, theme toggle) is visible during connection

## Design Notes

- The connecting message should use the existing `.connecting` CSS class styling (teal border, semi-transparent background) that's already used for per-project connection banners
- The "Your Projects" heading should still be visible, with the connecting message below it
- Action buttons at the bottom should be hidden or disabled during connection — creating/connecting projects requires a connected project set
