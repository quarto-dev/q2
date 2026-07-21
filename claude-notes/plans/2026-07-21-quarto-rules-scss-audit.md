# Audit `_quarto-rules.scss`: categorized selector inventory (bd-eias3e39)

**Date:** 2026-07-21
**Braid:** bd-eias3e39 (child of epic bd-4doe9lvt)
**Checkout:** invoked in the main-branch checkout at `~/rooms/room-2/q2` (no new worktree/branch created)
**Status:** Investigation — pending design alignment with user. **Do not start the audit sweep until the user answers the design questions below.**

## Triage verdict

**Ready to design.** The strand is one day old, the epic plan
(`claude-notes/plans/2026-07-21-quarto-rules-scss-parity-epic.md`) already
specifies the methodology, all referenced files exist at HEAD, and spot-checks
confirm the port-now/blocked split is real. The open questions are about
inventory granularity, output-layer strategy, and how much per-row verification
is required — not about whether the work makes sense.

## Issue context

Created 2026-07-21 by Carlos (so no staleness risk). Task, P2, labels
`css`/`parity`. Deliverable: for each top-level selector in TS Quarto's
`src/resources/formats/html/_quarto-rules.scss`, determine (1) whether the rule
already exists in Q2's SCSS, (2) whether Q2's HTML writer emits the DOM it
targets, and (3) categorize as `PORT-NOW` / `BLOCKED-ON-EMITTER` /
`ALREADY-PRESENT` / `INTENTIONALLY-DROPPED`. Output: an inventory table in
`claude-notes/research/2026-07-DD-quarto-rules-scss-inventory.md` plus themed
child strands under the epic (port-now groups) and blocked strands with
`blocks` deps on emitter features.

A strand comment says "Epic plan updated with code pointers for the audit
handoff (bundle.rs / resources.rs / writers / compile-test / baseline)" — but
the committed epic plan does **not** contain those pointers; they live in the
sibling plan `claude-notes/plans/2026-07-21-title-block-bottom-margin-parity.md`
(the bd-btjkyylx worked example). The pointers are reproduced under "What the
code looks like today" below, so nothing is lost.

## Dependency graph

- **parent-child:** bd-4doe9lvt (epic, open) — this audit is the epic's first
  and currently only child; every Phase-2/3 strand of the epic is created *by*
  this audit. The epic is entirely gated on this strand.
- **discovered-from (via the epic):** bd-btjkyylx — title-block bottom-margin
  parity (PR #406, merged as `c7523c2b`; strand still `in_progress`, likely
  just needs closing). It is the worked example the epic wants each port-now
  strand to follow: TDD compile-output assertion in
  `crates/quarto-sass/src/compile.rs` → port rule into the right layer →
  re-capture `phase5-single-doc-baseline/expected_hashes.txt` with a dated
  comment → end-to-end render check.
- No `blocks` edges in either direction.

## What the code looks like today

All referenced paths exist at HEAD (`main` @ `c7523c2b`, in sync with origin):

- **Source of truth:**
  `external-sources/quarto-cli/src/resources/formats/html/_quarto-rules.scss`
  — 774 lines, **144 depth-0 rule blocks** (extracted list committed at
  `claude-notes/plans/quarto-rules-scss-audit-investigation/top-level-selectors.tsv`).
  The strand's "~80" is the *family-grouped* count: the ANSI-color block is ~36
  selectors, `table.gt_table` is 7, layout-panel/cell is ~19, etc.
- **Q2 SCSS layers to grep for "already present":**
  `resources/scss/bootstrap/_bootstrap-rules.scss` (+ `_bootstrap-variables.scss`),
  `resources/scss/html/templates/{title-block,copy-code,highlight,embed-example}.scss`.
  Bundle assembly: `crates/quarto-sass/src/bundle.rs`; embedded resources:
  `crates/quarto-sass/src/resources.rs`; compile-output tests:
  `crates/quarto-sass/src/compile.rs` (`test_compile_default_css`).
- **Emitter side to grep:** `crates/pampa/src/writers/`,
  `crates/quarto-core/src/transforms/`, `crates/quarto-core/src/revealjs/`,
  plus engine resources (`crates/quarto-core/src/engine/knitr/resources/`).
- **Baseline to re-capture on CSS shifts:**
  `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`.

Spot-checks (2026-07-21, confirming the split is real and non-obvious):

| token | in Q2 SCSS? | Q2 emits DOM? | provisional read |
| --- | --- | --- | --- |
| `footnote-back` | no | **yes** (`quarto-core/src/transforms/footnotes.rs`, `revealjs/footnotes.rs`) | PORT-NOW |
| `code-annotation` | **yes** (`_bootstrap-rules.scss`) | no emitter found | audit: maybe dead CSS already ported — inverse case |
| `code-overflow` | no | only via knitr `hooks.R` | needs audit |
| `quarto-figure`, `quarto-layout-panel` | no | no | BLOCKED-ON-EMITTER |
| `quarto-cover-image`, `gt_table`, `ansi-*-fg` | no | no (pampa `writers/ansi.rs` is an ANSI *output* writer, not HTML span emission) | likely BLOCKED |
| mermaid theming vars (`$mermaid-*`, line 730) | — | mermaid landed 2026-07 (`5d6fe82b`, bd-5m4ga0s1) | re-check: may have just become PORT-NOW |

Note the `code-annotation` inverse case: the audit will also surface rules Q2
*has* but whose DOM it doesn't emit — worth a column, not just the four
categories.

## Proposed phases (draft)

- **Phase 0 — Row extraction.** Turn `top-level-selectors.tsv` into the
  inventory table skeleton, grouped into families (~25–30 rows at family
  granularity; see Q1). Include TS-Quarto line ranges per row.
- **Phase 1 — SCSS-presence sweep.** For each row, grep Q2's SCSS layers;
  record present/partial/absent with file:line.
- **Phase 2 — Emitter sweep.** For each row, grep writers/transforms for the
  class/id tokens; for ambiguous rows, render a small fixture
  (`cargo run --bin q2 -- render …`) and inspect the HTML. Fixtures live in
  `claude-notes/plans/quarto-rules-scss-audit-investigation/`.
- **Phase 3 — Categorize + write inventory** to
  `claude-notes/research/2026-07-21-quarto-rules-scss-inventory.md` (or the
  date the sweep completes).
- **Phase 4 — File strands.** Themed PORT-NOW children under bd-4doe9lvt
  (candidate groups from the epic: figures/floats, code-overflow, footnotes,
  layout panels, misc utilities) and BLOCKED strands with `blocks` deps on
  emitter-feature strands (create those emitter strands where missing).
- **Phase 5 — Close the audit strand**, leaving the epic's Phase 2/3 to the
  new children.

No TDD phase: the audit itself changes no code — the bd-btjkyylx TDD template
applies to each *port* strand it spawns, not to the inventory.

## Open design questions for the user

1. **Row granularity.** Inventory rows per depth-0 selector (144 rows) or per
   family (~25–30 rows, one verdict each, with the member selectors listed)?
   Family granularity matches how the port strands will be cut; I'd default to
   that, keeping the full 144-selector TSV as the appendix.
2. **Destination layer for PORT-NOW rules.** Continue the piecemeal approach
   (each rule into the thematically-right existing layer, per bd-btjkyylx), or
   create a Q2 `_quarto-rules.scss` counterpart layer so future diffs against
   TS Quarto are mechanical? This decides what the port strands say, so it's
   cheaper to settle before filing them.
3. **Verification depth for BLOCKED rows.** Is "no token hit in
   writers/transforms + no hit in a rendered kitchen-sink fixture" sufficient
   evidence that DOM isn't emitted, or do you want a rendered fixture attempt
   per feature (e.g. actually trying a layout panel, a `gt` table)? Grep-only
   is much cheaper; fixture-per-row is stronger.
4. **Engine-output families.** The ANSI-color block (~36 selectors),
   `widget-subarea`, `knitsql-table`, and `gt_table` are all execution-engine
   *output* styling. File them as one "engine output styling" blocked strand,
   or one per engine surface?
5. **Mermaid rows.** Mermaid just landed (bd-5m4ga0s1). Should the audit treat
   the mermaid-adjacent rules (`$mermaid-*` theming vars, the
   `.quarto-figure-* > figure > div` diagram-centering rules) as in-scope
   port-now candidates, or hand them to the mermaid feature line as a
   `related` strand?
6. **bd-btjkyylx cleanup.** PR #406 is merged but the strand is still
   `in_progress` — close it as part of this session's bookkeeping?

## Risks / tradeoffs (draft)

- **Q1-behavior drift:** some `_quarto-rules.scss` rules exist to patch Q1
  runtime-JS behavior (anchorjs, tippy, utterances, code-annotation JS) that Q2
  may implement differently or not at all — categorization needs care not to
  mark "Q2 has no anchorjs" rules as PORT-NOW just because a class token grepped.
- **Selector-extraction artifacts:** the TSV has 3 rows (89, 133, 141) where
  block comments smeared into the selector text; line numbers are correct,
  clean up during Phase 0.
- The audit itself is read-only, so risk is mostly *mis*-categorization; the
  per-strand TDD template on the port side is the safety net.
