---
title: 'Error-docs content authoring (umbrella)'
date: 2026-05-22
description: 'Tracks hand-written `docs/errors/` pages for all catalogued Quarto error codes, one subsystem at a time, with each page meeting a minimum stub-quality bar before its subsystem closes.'
status: draft  # drafting — pending user review
braid:
  strand: bd-an6z4  # child of bd-94x8a
  priority: P3
  labels: [content, documentation, error-reporting]
---

**Blocked by:** bd-nvlxn (foundation — template + schema must exist)

## Goal

Author `docs/errors/<subsystem>/Q-X-Y.qmd` for each entry in
`error_catalog.json`. 133 pages today, organized into per-subsystem
subdirectories per the foundation plan. Each page hand-written;
generation produces *only* skeletons.

This issue is an umbrella, not a single unit of work — opening
all 133 pages in one session would produce uniformly shallow
content. Instead, we open one sub-issue per subsystem when ready
to work on it, with a defined quality bar.

## Work items

(Umbrella — items are sub-issues opened on demand.)

- [ ] Sub-issue: `yaml` (21 codes)
- [ ] Sub-issue: `markdown` (36 codes)
- [ ] Sub-issue: `theme` (2 codes)
- [ ] Sub-issue: `writer` (21 codes)
- [ ] Sub-issue: `listing` (16 codes)
- [ ] Sub-issue: `navigation` (7 codes)
- [ ] Sub-issue: `project` (3 codes)
- [ ] Sub-issue: `template` (7 codes)
- [ ] Sub-issue: `xml` (16 codes)
- [ ] Sub-issue: `internal` (2 codes)
- [ ] Sub-issue: `cli` (1 code)
- [ ] Sub-issue: `lua` (1 code)
- [ ] After 3–5 stubs land, revisit the stub-quality floor and lock
      a final definition (see Decisions §2)
- [ ] Audit reports zero missing, zero mismatched, all pages at
      `stub` or better → umbrella closes

## Per-subsystem sub-issues

Subsystems and code counts (snapshot 2026-05-22):

| Subsystem  | Codes |
| ---------- | ----- |
| markdown   | 36    |
| yaml       | 21    |
| writer     | 21    |
| xml        | 16    |
| listing    | 16    |
| template   |  7    |
| navigation |  7    |
| project    |  3    |
| theme      |  2    |
| internal   |  2    |
| lua        |  1    |
| cli        |  1    |

Each subsystem gets its own beads sub-issue under this umbrella,
**opened on demand**, not all upfront. Closing a sub-issue requires:

- Every page in that subsystem present in `docs/errors/`.
- Every page at `status: stub` or better (the foundation plan
  defines the stub quality bar).
- `cargo xtask error-docs audit` reports zero mismatches and zero
  missing pages for that subsystem.

`status: complete` promotion is *not* a closing requirement.
Stubs are the floor; complete is a separate, ongoing pass driven
by user feedback or by us hitting the errors in real work.

## Suggested ordering

Highest-impact first, in rough order:

1. **yaml** (21) — every Quarto user touches YAML; many of these
   fire on basic config mistakes.
2. **markdown** (36) — largest single subsystem; many fire on
   common content patterns.
3. **theme** (2) — small, recently structured (bd-1pwy8 /
   bd-pgczr), fresh in our minds.
4. **writer** (21) — output-format failures; users hit these when
   rendering.
5. **listing** (16), **navigation** (7), **project** (3),
   **template** (7) — project-level features; less common but
   important for website projects.
6. **xml** (16) — narrower audience.
7. **internal** (2), **cli** (1), **lua** (1) — small, finish them
   off opportunistically.

Order is suggestive, not load-bearing. Open whichever sub-issue
matches the work in front of us.

## Per-page authoring workflow

1. **Generate the stub** with `cargo xtask error-docs new Q-X-Y`
   (from bd-8otua). If tooling isn't ready yet, copy the template
   from `docs/errors/README.md`.
2. **Find a real example.** Grep the codebase for `Q-X-Y` in
   source. Look at the call site to understand under what input
   the error fires. If we have snapshot tests that exercise the
   error, those are gold.
3. **Author the body.** Aim for stub quality on first pass:
   - **What this means**: rewrite `message_template` in plain
     English, expanding any jargon.
   - **Why this happens**: list 1–3 concrete user scenarios.
   - **How to fix**: at least one concrete remediation.
4. **Update front-matter `status`.** `draft` → `stub` when the
   sections above are filled in.
5. **Render check.** `cd docs && quarto preview`; verify the
   page renders and the listing picks it up.

Promotion from `stub` → `complete` happens later: when a user
reports the error and we improve the page based on their context,
or when we hit the error in our own work and add a richer example.

## Quality guardrails

Things to **avoid** when authoring:

- **Re-stating `message_template` and stopping.** The page exists
  precisely to say more than the terminal message could.
- **Generic "check your input" advice.** Every "how to fix"
  section should mention something specific to *this* error code.
  If we can't say anything specific, the page is at `status: draft`
  and we say so honestly.
- **Speculating about causes we haven't seen.** If we don't know
  *why* this fires in practice, list the one cause we know and
  mark `status: stub`.
- **Drift from the catalog.** `title`, `subsystem`, `since` must
  match `error_catalog.json`. Audit tool enforces this.

## When the catalog changes mid-flight

If a new code lands in `error_catalog.json` while this umbrella is
open:

- The audit tool (bd-8otua) will report it as missing on next run.
- The author of the catalog entry is expected to also generate a
  stub page (`xtask error-docs new`) in the same PR. Once Phase 2
  CI lands (audit `--fail-on missing`), this becomes enforced.
- The new page joins whichever subsystem sub-issue covers it; if
  no sub-issue is currently open for that subsystem, open one for
  the new code, even if it's a one-page sub-issue.

Catalog entries are append-only by design (see epic plan), so
codes effectively never get *removed*. If a future Quarto version
stops emitting a code, the page transitions to `status: deprecated`
but the catalog entry — and the page — stay live indefinitely so
users on older Quarto versions can still look the code up.

## Definition of done (for the umbrella)

The umbrella closes when:

- Zero missing pages reported by `cargo xtask error-docs audit`.
- Zero mismatches reported.
- All pages at `status: stub` or better.

Promoting everything to `status: complete` is **not** a closing
condition for the umbrella. That's a long-running quality
initiative that outlives the initial population.

## Out of scope

- Localization. English-only.
- Tutorials or how-to pages indirectly related to the errors
  (e.g. a generic "YAML basics" page). Pages here are about
  specific error codes.
- "Common pitfalls" cross-cutting articles. Could be a future
  enhancement linked from individual error pages.

## Decisions (resolved 2026-05-22)

1. **Sub-issue cadence** = on-demand. Open one sub-issue per
   subsystem when we're ready to start on it; don't pre-create 12
   idle issues.
2. **Stub-quality floor** = TBD after a few real stubs. We'll
   author a handful and look at them before locking the bar.
   Until then the plan's working definition stands ("What / Why /
   How" present with non-trivial content), but it's explicitly
   subject to revision.
3. **`status: complete` quality bar** = defer to a later pass, as
   originally drafted.
