---
title: 'Error-docs tooling: `cargo xtask error-docs`'
date: 2026-05-22
description: 'Specifies a `cargo xtask error-docs` subcommand that audits error documentation pages against the error catalog for missing, stale, mismatched, or misplaced pages, plus a health rollup and stub generation.'
status: draft  # drafting — pending user review
braid:
  strand: bd-8otua  # child of bd-94x8a
  priority: P2
  labels: [documentation, error-reporting, tooling]
---

**Blocked by:** bd-nvlxn (foundation — schema + directory layout must exist)

## Goal

A small, focused xtask subcommand that answers, on demand:

- Which catalog entries don't have a page yet?
- Which pages exist for codes no longer in the catalog?
- Which pages drift from the catalog (mismatched title, subsystem,
  since-version)?
- What's the documentation health, broken down by subsystem and
  status?

And, as a convenience for content authors:

- Generate a stub qmd from a catalog entry.

The tool is **read-only** for the catalog. The catalog (`crates/quarto-error-reporting/error_catalog.json`)
is the source of truth for error codes; the docs are the source of
truth for prose. The tool reconciles the two without overwriting
either.

## Command shape

```
cargo xtask error-docs                  # = `audit`
cargo xtask error-docs audit [--fail-on missing|stale|mismatch|any|none]
cargo xtask error-docs health
cargo xtask error-docs new <Q-X-Y> [--force]
cargo xtask error-docs validate         # (stretch)
```

### `audit` (default)

Diff the catalog against `docs/errors/`. Reports four problem
classes:

- **Missing.** Catalog has the code; no page exists at the
  expected path `docs/errors/<subsystem>/<code>.qmd`.
- **Stale.** Page exists; catalog no longer has the code. Defensive
  check only — the catalog is append-only by design (see epic plan),
  so a stale page almost always indicates a typo in a filename or
  an accidental catalog removal, not legitimate retirement. The
  routine fix is to **restore the catalog entry**, not delete the
  page. Reported as a warning, never as a "delete this file"
  suggestion.
- **Mismatch.** Page exists; front-matter `title`, `subsystem`, or
  `since` doesn't match the catalog. The audit reports each
  drift specifically.
- **Misplaced.** Page exists at the wrong subdirectory (front-matter
  `subsystem` doesn't match the parent directory name, or the
  parent directory doesn't match the catalog's `subsystem` for
  that code). Subdirectory layout is load-bearing for URL stability.

Plus a catalog-internal consistency check:

- **`docs_url` drift.** Each catalog entry's `docs_url` should
  follow `https://quarto.org/docs/errors/<subsystem>/<code>`.
  Mismatches here aren't about docs/ at all — they indicate the
  catalog itself drifted from the layout convention.

Note: **`description` vs `message_template` drift is NOT flagged.**
The two fields are intentionally independent (catalog message is
terminal-output text; page description is listing-summary text).
Confirmed 2026-05-22.

Output: a structured report, terminal-readable by default, with
`--format json` for CI consumption.

Default exit codes (overridable via `--fail-on`):

- `0` — no problems, or only problems below the chosen threshold.
- non-zero — at least one problem at or above the threshold.

Default `--fail-on` is `none` initially (audit is informational);
once content lands we can flip the default to `missing` so new
catalog entries can't merge without at least a stub. **Decision
deferred** to when bd-an6z4 is mostly complete.

### `health`

Human-readable rollup, e.g.:

```
Error documentation health
==========================
Total catalog entries: 133
Pages present:         42  (31.6%)

By subsystem:
  yaml         8/21   (38.1%)   complete:  3   stub:  5
  markdown     5/36   (13.9%)   complete:  1   stub:  4
  ...

By status (pages only):
  complete    11
  stub        28
  draft        3
  deprecated   0
```

No exit-code semantics; this is for humans.

### `new <Q-X-Y>`

Generate `docs/errors/<subsystem>/<Q-X-Y>.qmd` from the catalog
entry (subdirectory derived from `error_catalog.json[<code>].subsystem`,
parent directory created if needed):

- Front-matter fully populated from the catalog (`title`,
  `subsystem`, `code`, `since`, `categories: [<subsystem>]`).
- `status: draft`. (Promoted to `stub` by hand once a human
  reviews the skeleton; confirmed 2026-05-22.)
- `description:` left empty with a comment placeholder.
- Body uses the template from [foundation plan](2026-05-22-error-docs-foundation.md#page-template),
  with each section containing a single `<!-- TODO: ... -->` line.

Errors if the page already exists, unless `--force`. Errors if the
code is not in the catalog (typo guard).

### Front-matter schema validation (folded into `audit`)

Schema-check existing pages: required front-matter fields present,
`status` is a valid enum value, `code` matches the filename.
**Decision 2026-05-22:** folded into `audit` rather than a
separate subcommand — fewer moving parts for v1. Schema violations
appear as their own problem class in the audit report ("Invalid
front-matter"). If the audit grows unwieldy later, we can split.

## Implementation sketch

Lives under `crates/xtask/src/error_docs/` (or similar):

```
crates/xtask/src/error_docs/
  mod.rs           ← argument parsing, dispatch
  audit.rs         ← catalog ↔ docs diff
  health.rs        ← rollup report
  new.rs           ← stub generation
  schema.rs        ← front-matter parsing + validation
  render.rs        ← terminal + JSON output
```

Dependencies (most already in the xtask):

- **Catalog parsing.** Read `crates/quarto-error-reporting/error_catalog.json`
  directly with `serde_json`. (Could go through `quarto_error_reporting::catalog::ERROR_CATALOG`,
  but that pulls a heavy dep into xtask; raw JSON read is fine and
  keeps the xtask light.)
- **Front-matter parsing.** YAML frontmatter from each `Q-*.qmd`.
  `serde_yaml` (already in the workspace).
- **File enumeration.** `walkdir` or plain `std::fs` over
  `docs/errors/`.

No new heavy deps expected.

## CI integration

**Deferred entirely for v1** (decision 2026-05-22). We want to
get the basic workflow up and running and learn how it feels to
use it before wiring it into CI. The audit is callable on demand
by humans and agents; CI integration becomes a separate beads
issue once we've used the tool for a while.

Sketch (kept as a future reference, not part of this work):

- Phase 1 — `audit --fail-on mismatch` runs in CI; mismatches
  fail, missing pages don't.
- Phase 2 — `audit --fail-on missing` once content is widely
  populated; new catalog entries can't merge without at least a
  stub.

## Work items

(Started after bd-nvlxn lands.)

- [ ] Scaffold `crates/xtask/src/error_docs/{mod,audit,health,new,
      schema,render}.rs`
- [ ] Hook the new subcommand into `crates/xtask/src/main.rs`
- [ ] Write tests for `audit` first (synthetic catalog + tempdir
      tree) covering all problem classes — TDD per CLAUDE.md
- [ ] Implement `audit` (text + flat JSON output)
- [ ] Write tests for `health` (snapshot a synthetic rollup)
- [ ] Implement `health`
- [ ] Write tests for `new` (stub generation, guards)
- [ ] Implement `new`
- [ ] End-to-end sanity: `cargo run -p xtask -- error-docs audit`
      against the real tree exits 0 with default flags
- [ ] End-to-end sanity: `cargo run -p xtask -- error-docs health`
      prints a reasonable rollup
- [ ] `cargo xtask verify --skip-hub-build` clean

## Test strategy (TDD)

Rust tests live alongside the xtask. Per CLAUDE.md, write tests
*before* implementation:

1. **`audit`**: unit tests with synthetic catalog + docs trees in
   `tempdir`. Cover each problem class (missing, stale, mismatch).
   Cover the all-clean case. Snapshot the rendered terminal output
   (text mode) and the JSON output via `insta`.

2. **`health`**: same approach. Snapshot of the rollup with a fixed
   synthetic tree.

3. **`new`**: write a stub from a synthetic catalog entry, parse
   the resulting front-matter back, assert all fields match.
   Test the "already exists" guard and the "code not in catalog"
   guard.

4. **End-to-end sanity check** (per CLAUDE.md):
   `cargo run -p xtask -- error-docs audit` against the real
   `error_catalog.json` and `docs/errors/` exits successfully
   when the seed page from bd-nvlxn is the only one present (it
   should report \~132 missing, exit 0 because default fail-on is
   `none`).

## Out of scope

- Writing prose. `new` produces a stub; humans fill it in.
- Updating the catalog from docs (we never want docs to be the
  source of truth for which codes exist).
- Cross-validating prose quality (LLM review, readability score,
  etc.). Could be a future tool; not this one.
- Linking checking (do `[Q-X-Y'](Q-X-Y'.qmd)` references resolve).
  Quarto's own build will surface broken links.

## Decisions (resolved 2026-05-22)

1. **Subcommand naming** = `cargo xtask error-docs`. Caveat
   logged by user: the wider xtask naming convention will need a
   pass later (mixed verb-first vs noun-first today).
2. **JSON output format** = **flat list** of problems. Optimize
   for machine consumption — this output will mostly be consumed
   by agents and scripts, not humans reading raw JSON. Each entry
   carries its own `kind` field (e.g. `"kind": "missing"`,
   `"kind": "mismatch"`, etc.), so `jq 'map(select(.kind ==
   "missing"))'` is the canonical filter idiom.
3. **`validate` command** folded into `audit` (see above).
4. **CI workflow** deferred (see above).
