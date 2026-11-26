---
title: 'hub-client: presence heartbeat keeps idle collaborators visible'
date: 2026-11-25
description: 'Presence was broadcast only on cursor or selection changes, while receivers drop peers they have not heard from in 5 s. Re-broadcast presence on a timer while a file is open.'
status: in-progress
---

**Braid:** bd-861i4fmk
**Issue:** <https://github.com/quarto-dev/q2/issues/1262>

## Overview

Current behaviour: `hub-client/src/services/presenceService.ts` broadcasts a
presence message only when `usePresence` reports a cursor or selection
change. Every receiver drops a peer it has not heard from in
`staleThresholdMs` (5 s), checked every `cleanupIntervalMs` (2 s). A
collaborator who stops moving the cursor disappears for everyone else
within 5 to 7 s.

New behaviour: a new `heartbeatIntervalMs` setting (default 2 s, below the
stale threshold) drives a `setInterval` that calls `broadcastPresence()`
again. `broadcastPresence` already does nothing without an open file, so
the heartbeat only sends while a file is open. The timer starts in
`initPresence` and is cleared in `cleanupPresence`. The stale threshold is
unchanged.

## Work items

- [x] Fake-timer tests in `presenceService.test.ts` (heartbeat describe block)
- [x] `heartbeatIntervalMs` config, `heartbeatInterval` state, start/stop
- [x] vitest, typecheck, ESLint
