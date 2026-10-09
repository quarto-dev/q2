---
title: 'Vendored / non-cargo dependency audit — plan'
date: 2026-05-04
description: 'Sets up an inventory and audit procedure for vendored non-Cargo assets such as Bootstrap SCSS and CSL styles, so upstream updates are checked on the same cadence as Cargo dependencies.'
braid:
  strand: bd-xm7l
  priority: P2
  labels: [deps, vendored]
---

**Related skill:** `.claude/skills/upgrade-cargo-deps/SKILL.md`
**Inventory doc:** `claude-notes/research/vendored-dependencies-inventory.md`

## Overview

The existing `upgrade-cargo-deps` skill audits Cargo dependencies but
ignores everything *else* the repo vendors — Bootstrap SCSS, Bootstrap
Icons, the chicago-author-date CSL style, tree-sitter highlight queries,
quarto-cli's built-in extensions, knitr R scripts, etc. These assets
are bundled at compile time via `include_dir!` / `include_str!` /
`include_bytes!` (or copied into static-asset roots) and have no
mechanism notifying us when upstream releases a new version.

Goal of this work: give a future agent enough information to
**audit, refresh, and verify** the vendored set on the same cadence as
the Cargo audit, without re-discovering everything from scratch every
time.

This plan does *not* refresh any vendored asset. It only sets up the
machinery (inventory + audit procedure) that subsequent runs use.

## Outcome of this session

This session produces three artifacts; nothing else changes in the
repo:

1. **Inventory doc** at `claude-notes/research/vendored-dependencies-inventory.md`
   cataloguing every vendored asset with its upstream, current
   version, bundling mechanism, update procedure, and verification
   steps.
2. **This plan doc** describing the discovery strategies and the
   future skill expansion.
3. **A beads issue** linking the two and tracking the next concrete
   steps (skill expansion + filling inventory gaps).

## Discovery strategies (summarized)

The full strategy list with grep recipes lives in the inventory doc
under § *Discovery strategies*. The five top-level approaches:

1. **Sweep `include_dir!` / `include_str!` / `include_bytes!`** in
   `*.rs` files; flag any path pointing outside the crate's own
   `src/`.
2. **Walk the repo-root `resources/` tree.** Every immediate
   subdirectory must be in the inventory or explicitly excluded.
3. **Walk per-crate `resources/` and `test-data/` directories.**
4. **Grep for provenance headers** (`Source:`, `Vendored:`, `Copied
   from:`) in `*.scm`, `*.lua`, `*.css`, `*.scss`, `*.R`, `*.html`.
5. **Inspect static-asset roots** (`hub-client/public/`,
   `trace-viewer/public/`).
6. **Find sub-package `package.json`s** outside `hub-client/` —
   they often produce JS bundles `include_str!`\'d into Rust.
7. **Note tree-sitter parser grammar forks** (those are massive;
   they appear in the inventory only to silence repeated discovery,
   not as audit candidates).

## Categorization

After discovery, each candidate falls into one of:

- **Tracked, version known** — appears in inventory with a current
  version/SHA. Audit just re-checks upstream.
- **Tracked, version unknown** — in the inventory but missing a
  recorded version (an *inventory gap*). Audit records "unknown" and
  files a beads issue if not already filed.
- **Repo-native** — not vendored; once verified, added to inventory's
  *not vendored* section so future audits skip it.
- **New** — not in inventory. Audit adds an entry, flags it in the
  TL;DR, and (if no clear update mechanism exists) files a beads
  issue.

## Future skill expansion

The `upgrade-cargo-deps` skill should grow a sibling phase — call it
**vendored audit** — that runs on the same bi-weekly cadence:

- **Cadence:** same survey run; one extra phase.
- **Read** the inventory doc.
- **For each entry**, perform a lightweight upstream check:
  - For HTTP-fetchable refs (Bootstrap, Bootstrap Icons, CSL styles):
    fetch a tag/commit listing and compare with the recorded
    version.
  - For tree-sitter grammars: fetch the upstream commit listing.
  - For npm-driven bundles (`quarto-system-runtime/js/`): run
    `npm outdated --prefix crates/quarto-system-runtime/js` and
    capture the result.
  - For quarto-cli-derived assets (extensions, knitr R, Pandoc HTML
    template): compare against `external-sources/quarto-cli`\'s
    current SHA *if* it's checked out; otherwise note as "no
    upstream check available this run".
- **Update** the `Last reviewed` date on each entry in the inventory
  for entries the audit successfully verified, even if no upgrade
  is needed.
- **For each entry where upstream is ahead**, file a `chore` beads
  issue (`deps,vendored`, priority `3`) with a one-line "current →
  available" description and a pointer to the entry's update
  procedure.
- **Do not perform the upgrade.** Vendored upgrades typically
  require manual verification (themes, parity tests, citeproc
  fixtures) that doesn't fit the "apply patch/minor automatically"
  pattern of the cargo side.
- **Inventory hygiene:** the audit also runs the discovery sweeps
  and reports any candidate that doesn't appear in the inventory.

This is intentionally separate from the cargo phase — it has a
different failure mode (upstream-fetch failures shouldn't abort the
cargo survey) and produces a different deliverable per entry
(beads-only, no lockfile commit).

## Work items

### Phase 1 — this session

- [x] Survey the workspace for vendored / non-cargo dependencies.
- [x] Identify discovery strategies that future agents can re-run.
- [x] Author `claude-notes/research/vendored-dependencies-inventory.md`
      with entries for the assets found (Bootstrap SCSS, Bootstrap
      Icons, Quarto extensions, knitr R, Pandoc HTML, chicago
      CSL, tree-sitter highlights, tree-sitter grammars,
      quarto-system-runtime JS, reveal.js-menu CSS, CSL test
      fixtures, Lua filters).
- [x] Author this plan doc.
- [x] File the tracking beads issue and link it from this plan.
- [x] Verify `tree-sitter-qmd` and `tree-sitter-doctemplate` are
      repo-native (correcting an earlier misclassification as
      vendored). Filed bd-7co9 (`discovered-from:bd-xm7l`) for the
      doc cleanup in `tree-sitter-qmd`.

### Phase 2 — next session(s)

- [ ] Fill inventory gaps (record upstream SHAs for
      knitr R scripts, Pandoc HTML template, version-less
      extensions). Each gap is its own beads sub-issue under the
      epic, label `deps,vendored,inventory-gap`.
- [ ] Extend `upgrade-cargo-deps/SKILL.md` (or split into a
      separate `audit-vendored-deps/SKILL.md` and have one wrapper
      run both) with the vendored-audit phase described above.
- [ ] Decide and implement the upstream-check mechanism per entry
      (HTTP-fetch, `npm outdated`, `git ls-remote`, etc.). Some
      will require WebFetch; some are local.
- [ ] First real-run of the expanded skill; refine based on
      experience.

### Phase 3 — optional / future

- [ ] Consider scripting the discovery sweeps as `cargo xtask
      audit-vendored` so the skill calls a single command.
- [ ] Add a CI lint that flags any new `include_dir!`/`include_str!`
      pointing into `resources/` *without* a corresponding inventory
      entry. (The existing `cargo xtask lint` infrastructure under
      `crates/xtask/src/lint/` is a natural home.)

## Notes / decisions

- **Inventory location** is `claude-notes/research/` (per
  `CLAUDE.md` § *Where information lives*: "research/ for findings,
  audits, reference material"). It is *not* in
  `.claude/skills/upgrade-cargo-deps/` because the inventory is
  general-purpose reference material, not skill-internal pin data
  (cf. `PINS.md`, which is skill-specific).
- **Inventory is data, not prose.** Each entry is structured so the
  skill can parse it without much language understanding. Future
  refactor: convert to YAML/TOML with a schema if the skill grows
  more complex.
- **Tree-sitter parser grammar forks** are explicitly out of scope
  for any automated audit — they're rebased manually on a long
  cadence. The inventory entry is purely to suppress repeated
  discovery.
- **Repo-native Lua filters** are listed under entry L for the same
  reason.
