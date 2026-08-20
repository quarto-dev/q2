# Extension management commands (`remove` / `list` / `add` / `update`) — STUB plan

**Status:** STUB (2026-07-03) — placeholder in the **extensions epic**
(`2026-03-16-extensions-grand-plan.md` family), not yet scheduled or designed.
Created per Gordon's direction after the Q1 engine-CLI survey established that
`quarto remove` is generic extension management with **no** engine-specific
behavior (engines are just extensions that contribute engines).

**Sources:** `claude-notes/research/2026-07-03-q1-engine-cli-survey.md`
(per-command evidence + the remove-julia bug chain); strand **bd-5edooc78**
(the removal-guard requirement this plan MUST carry).

## Overview

q2's `add`, `update`, `remove`, `list` commands all exist in clap but are
`NotImplemented` stubs (`crates/quarto/src/commands/`). Q1's equivalents manage
extensions by id (org/name glob matching), with interactive pickers,
confirmation prompts, an `--embed` mode for authoring, and tool-vs-extension
target resolution.

## Non-negotiable requirement (from bd-5edooc78)

Q1 has a live bug: `quarto remove julia-engine` deletes the BUNDLED julia
engine, because its built-in guard only checks `organization === "quarto"`
while subtree extensions load with `organization: undefined`. q2's
implementation must NOT reproduce the org-only guard — protect by
resolved-path-under-install-dir, or refuse to remove `contributes.engines`
extensions without an explicit force flag (options analyzed in the survey doc).

## Work items (skeleton — to be designed when scheduled)

- [ ] Survey Q1's four commands end-to-end (id resolution, globbing, scopes,
      prompts, `--embed`, output) — Q1-parity spec, same discipline as Plan 9/10.
- [ ] Design the q2 extension-id/registry model these commands operate on
      (reconcile with the existing extension read/registry code).
- [ ] Implement with the removal guard from bd-5edooc78 baked in + a test that
      REDDENS if an engine-contributing/built-in extension becomes removable.
- [ ] Decide + (if Gordon approves) report the Q1 bug upstream to quarto-cli.
