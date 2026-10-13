---
title: '`list_projects`: remember a default project set in the OS keyring (bd-3h5hbfdv)'
date: 2026-10-13
description: 'Persist the project-set reference in secure storage on first use so `list_projects` works with no argument; new `set_default_project_set` tool for manual updates.'
status: in-progress
braid:
  strand: bd-3h5hbfdv
  priority: P2
---

## Overview

`list_projects` requires the project-set link or id on every call, which is
inconvenient: a user typically has one set ("My projects") and must paste its
`#/link-project-set/` URL each session. The MCP server already stores OAuth
credentials in the OS keyring (`@napi-rs/keyring`, service
`dev.quarto.hub-mcp`); the project-set reference belongs in the same secure
storage.

Semantics (from the user\'s request): store **the first time** — an explicit
`project_set` that reads successfully is persisted only when no default is
stored yet, so exploring someone else\'s set link does not silently switch
the default. Manual updates go through a dedicated tool.

## Checklist

- [x] `src/default-project-set.ts`: `DefaultProjectSetStore` (same service as
  `CredentialStore`, account `default-project-set`; read folds errors to
  `null`, write/clear throw `KeyringUnavailableError`; backend injectable)
- [x] Failing tests: store unit tests + in-memory tool tests (bare call with
  no default errors actionably; explicit arg persists on first success; bare
  call then uses the stored value; a different explicit set does not
  overwrite; `set_default_project_set` validates and overwrites; keyring
  write failure warns in `list_projects` but errors in the setter)
- [x] `ConnectionManager` gains optional `defaultProjectSetStore`
- [x] `tools.ts`: `project_set` optional on `list_projects` + default
  resolution/persist in `handleListProjects`; new `set_default_project_set`
  tool (read tools section — local state, not project content; available in
  `--read-only`)
- [x] `index.ts`: construct the store in main; instructions mention the
  remembered default
- [x] Tool-list/conformance tests updated for the 23rd tool
- [x] Full package suite + typecheck + build smoke; render this note strict

## Outcome

All green: 69 test files / 605 tests, `tsc --noEmit` clean, this note
renders strict. End-to-end verification (temporary vitest file, deleted
after the run): spawned the real `dist/index.js` over stdio against a test
hub with the **real macOS keychain** — explicit `#/link-project-set/` call
persisted the link verbatim, a bare `list_projects` read it back,
`set_default_project_set` overwrote it, and the entry was deleted
afterwards (verified absent). Caveat carried over from the design: if the
remembered set is deleted or belongs to another hub, the bare call\'s error
names the value and points at `set_default_project_set` to fix it.

## Details

- The stored value is the **original string as passed** (link or bare id),
  fed back through `parseProjectRef` on use — one parser, no new blob schema,
  and the `server=` of a link keeps routing the read.
- Persistence failure in `list_projects` degrades to a warning text block
  (the listing still returns); in `set_default_project_set` persistence is
  the point, so a keyring failure is an error result.
- The setter validates by reading the set document before storing, so a bad
  link fails fast instead of poisoning the default.
