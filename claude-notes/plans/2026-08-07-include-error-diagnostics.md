# Include failure diagnostics: surface inner errors, kill spurious "Unknown shortcode"

**Strand:** bd-qpvoamvu (discovered: bd-1fz3vh99 — nested-container includes)
**Status:** approved 2026-08-07 — implementation in progress on
branch `braid/bd-qpvoamvu-include-failure-diagnostics-surface`

## Overview

While porting the Posit Connect docs to Quarto 2, a failing
`{{< include "../include/_common.qmd" >}}` produced this pair of
diagnostics:

```
Error: [Q-5-3] Include file parse error
  … Failed to parse included file '…/_common.qmd': 1 error(s)

Warning: [Q-16-3] Unknown shortcode
  … Shortcode `include` is not recognized
```

Two distinct defects:

1. **The inner parse error is swallowed.** The user is told "1
   error(s)" but never *which* error or *where* in the included file.
   (In the Connect case the real problem is a Q-2-10 smart-quote error
   at `_common.qmd:383:31` — `the groups' Unique IDs` — which the user
   can only discover by rendering the included file standalone, which
   the project's underscore convention actively prevents.)
2. **A spurious, contradictory Q-16-3 follows.** The include shortcode
   *is* recognized — it just failed — yet we then warn "Shortcode
   `include` is not recognized. Check the shortcode name for typos."

## Minimal reproduction

Two files (kept in scratchpad during investigation; becomes the
regression test fixture):

`index.qmd`:
```markdown
---
title: Include error repro
---

Before the include.

{{< include "_bad.qmd" >}}

After the include.
```

`_bad.qmd`:
```markdown
Some text before the error.

This line mentions the groups' Unique IDs instead of their names.
```

`q2 render index.qmd --to html` reproduces both diagnostics exactly
(verified 2026-08-07 on `main` @ 9249c43d). Rendering `_bad.qmd`
standalone shows the swallowed inner error: Q-2-10 "Closed Quote
Without Matching Open Quote" with a two-label ariadne snippet.

## Diagnosis

### Bug 1: inner diagnostics discarded

`crates/quarto-core/src/stage/stages/include_expansion.rs:141-159`.
When `pampa::readers::qmd::read` on the included file returns
`Err(diagnostics)` (a `Vec<DiagnosticMessage>`), the stage throws the
vector away and emits a single Q-5-3 whose problem text contains only
`diagnostics.len()`. The inner diagnostics carry precise
`SourceInfo` locations (top-level `location` plus per-`DetailItem`
locations) — everything needed for a proper ariadne snippet — but:

- their `FileId`s reference the *included file's own* parse context
  (`FileId(0)` = the included file), not the parent document's
  `SourceContext`;
- on the error path the stage `continue`s *before* the code that
  registers the included file's content in the parent's two source
  contexts (lines 162-190), so the parent `SourceContext` has no entry
  to render a snippet from anyway.

### Bug 2: spurious Q-16-3

On **all three** include-failure paths — circular include (Q-5-1, line
97), file-not-found (Q-5-2, line 114), parse error (Q-5-3, line 141) —
the stage does `i += 1; continue`, leaving the
`Paragraph[Shortcode(include)]` block in the AST. The include
shortcode is expanded by `IncludeExpansionStage`, *not* by the
`ShortcodeResolve` transform; `shortcode_resolve.rs` has no `include`
handler. So the leftover shortcode falls through every dispatch step
and hits the unknown-shortcode fallback
(`crates/quarto-core/src/transforms/shortcode_resolve.rs:600-610`),
producing the contradictory warning.

### Discovered (separate strand): includes nested in blocks are silently dropped

`expand_includes_in_blocks` only walks the **top-level** block list —
it never descends into `Div`/`BlockQuote`/list containers. An include
inside a `::: {.callout-note}` div is never expanded: the content is
silently missing from the output and the only signal is the same
misleading Q-16-3. Verified with a fixture on 2026-08-07. Q1 supports
includes inside divs, so this is a real porting hazard, but it is a
feature gap with its own design surface (recursion + FileId
bookkeeping through nested containers) — filed as its own strand
rather than folded in here.

### Discovered during implementation (2026-08-07): include codes collide with the project subsystem

The include stage's `Q-5-1`/`Q-5-2`/`Q-5-3` are **not in the catalog
as include errors**. Subsystem 5 is `project`: the catalog defines
Q-5-1 "Resource Path Resolves Outside the Project Root", Q-5-2
"Invalid Glob Pattern in `resources:`", Q-5-3 "Failed Walking Glob
Matches for `resources:`" — all legitimately emitted by
`project_resources.rs`. `include_expansion.rs` squats on the same
numbers with unrelated meanings, so a user who looks up "Q-5-3
Include file parse error" lands on glob-walking docs. No other code
site and no test depends on the include-stage spellings.

**Resolution folded into this strand:** mint a new subsystem `17`
(`include`) in `quarto-error-catalog/error_catalog.json` — subsystem
numbers 4/6/8 are gaps we leave alone; 17 is the next unallocated
number after `extension` (16):

- `Q-17-1` — Circular Include (was squatting on Q-5-1)
- `Q-17-2` — Include File Not Found (was Q-5-2)
- `Q-17-3` — Include File Parse Error (was Q-5-3)
- `Q-17-4` — Include Not Expanded Here (the new Phase-4 code)

This is user-visible renumbering, but the old codes never had correct
catalog entries for the include meanings, so nothing correct is
broken. The plan text below uses the new codes.

## Fix plan (TDD)

### Phase 1 — tests first

New integration test file
`crates/quarto-core/tests/integration/include_expansion_diagnostics.rs`
(registered alphabetically in `tests/integration/main.rs`), driving
the real HTML pipeline over temp-dir fixtures (same pattern as
`include_resolve_pipeline.rs`). Tests, all written and verified
failing before any fix:

- [x] T1: parse-error include → diagnostics contain the Q-17-3 wrapper
      **and** the inner parse diagnostic (assert its code, e.g.
      Q-2-10, and that its location resolves to the *included* file's
      path/line via the returned `SourceContext`).
- [x] T2: parse-error include → **no** Q-16-3 anywhere in the
      collected diagnostics (regression for the spurious warning).
      Must run the pipeline through the shortcode-resolve transform,
      not just the include stage.
- [x] T3: file-not-found include → Q-17-2 present, no Q-16-3.
- [x] T4: circular include → Q-17-1 present, no Q-16-3.
- [x] T5 (unit, in `include_expansion.rs` tests): failed include block
      is removed from the AST; surrounding blocks intact.
- [x] End-to-end check per CLAUDE.md: run the real binary on the repro
      fixture and inspect stderr — see "End-to-end verification record"
      below.

### Phase 2 — surface the inner diagnostics (bug 1)

In the `Err(diagnostics)` arm of `include_expansion.rs`:

- [x] Register the included file's content in **both**
      `doc.ast_context.source_context` and `doc.source_context`,
      exactly as the success path does (the two contexts must grow in
      lockstep — the success path `debug_assert_eq!`s their new
      `FileId`s, and skipping registration on the error path would
      desynchronize any *later* successful include in the same
      document).
- [x] Remap each inner diagnostic's `location` and every
      `details[i].location` from `FileId(0)` to the newly registered
      id via `SourceInfo::remap_file_ids` (public method on
      `quarto_source_map::SourceInfo`; already used by
      `quarto-ast-reconcile/src/remap.rs:55`).
- [x] Push the Q-17-3 wrapper (still anchored at the include site — it
      correctly answers "why is my content missing here?") followed by
      the remapped inner diagnostics into `ctx.diagnostics`. Reword
      the wrapper problem from `… : N error(s)` to something like
      `Included file '…' has N parse error(s), reported below` so the
      wrapper and the inner reports read as one story.

### Phase 3 — remove the failed include block (bug 2)

- [x] On all three failure paths (Q-17-1 / Q-17-2 / Q-17-3), replace
      `i += 1; continue` with `doc.ast.blocks.remove(i); continue`
      (no increment). The diagnostic has already been emitted; the
      shortcode must not leak into transforms that will misreport it.
      Content loss is not a concern — the include already contributed
      nothing, and for Q-5-3 the render fails anyway.

### Phase 4 — accurate message for leftover `include` shortcodes

Any `include` shortcode that *still* reaches `ShortcodeResolve` after
Phase 3 is one the expansion stage never considered: today that's an
inline include (`text {{< include f.qmd >}} text`) or one nested
inside a container (until the discovered strand fixes that). The
"unknown shortcode / check for typos" message is wrong for these.

- [x] Special-case `shortcode.name == "include"` at the
      unknown-shortcode fallback in `shortcode_resolve.rs`: emit a
      dedicated warning under a **new `Q-17-4` catalog entry** (see the
      subsystem-17 note above) saying the shortcode
      *is* known but was not expanded, e.g. "The `include` shortcode
      is only expanded when it is the only content of a top-level
      paragraph", with a hint about moving it to its own line. Exact
      wording to be settled during implementation review.
- [x] Test: inline include produces the dedicated message, not
      "Shortcode `include` is not recognized".

### Phase 5 — verification & bookkeeping

- [x] `cargo nextest run --workspace` (11005 passed) and full
      `cargo xtask verify` including the WASM/hub-client legs — all
      green (2026-08-07).
- [x] Re-render the Connect docs page
      (`admin/authentication/oauth2-openid-based/entra-id-openid-connect/index.qmd`)
      and confirm the output now names `_common.qmd:383` and emits no
      Q-16-3 (codes read Q-17-3 + inner Q-2-10) — done; see the
      verification record above.
- [ ] Close strand; update this plan's checklist.

## End-to-end verification record (2026-08-07)

All output below observed from the real binary (`target/debug/q2`,
built from this branch), ANSI stripped; output was inspected, not
inferred from exit codes.

**Repro fixture** (`q2 render index.qmd --to html` in the 2-file
scratchpad project):

```
Error: [Q-17-3] Include file parse error
 7 │ {{< include "_bad.qmd" >}}
   │              ╰─── Included file '…/_bad.qmd' has 1 parse error(s), reported below

Error: [Q-2-10] Closed Quote Without Matching Open Quote
 3 │ This line mentions the groups' Unique IDs instead of their names.
   │                              ╰─── This is the opening quote. …

2 errors
```

No Q-16-3 anywhere.

**Inline include** (`q2 render inline.qmd --to html`,
`text {{< include "_good.qmd" >}} more`):

```
Warning: [Q-17-4] Include not expanded
 5 │ text {{< include "_good.qmd" >}} more
   │                   ╰─── This `include` shortcode is in a position where includes are not expanded
ℹ Put the include shortcode in its own paragraph, surrounded by blank lines
```

**The originating Connect docs page**
(`q2 render admin/authentication/oauth2-openid-based/entra-id-openid-connect/index.qmd`):
now reports the Q-17-3 wrapper at `index.qmd:123` followed by
`[Q-2-10]` with a full two-label snippet at `_common.qmd:383:31`
(`should be based on the groups' Unique IDs …`); no Q-16-3 for the
include. The remaining warnings on that page (Q-13-x navigation etc.)
are the unrelated porting issues being handled separately.

**Docs site**: `q2 render docs/` — 186/186 files rendered; the
errors listing page picks up all four new include codes; the only
new-page warning (a link to the not-yet-written extension/Q-16-3
page) was fixed by de-linking.

## Decisions (reviewed 2026-08-07)

1. **Wrapper vs. details → separate diagnostics.** Inner diagnostics
   go in as separate top-level diagnostics after the Q-17-3 wrapper.
   Bonus rationale from review: the diagnostic-coalescing machinery
   then deduplicates the inner reports when the same broken file is
   included from many locations (a plausible scenario in the Connect
   docs), which `DetailItem` nesting would defeat.
2. **Phase 4 gets a new Q-5-x code** (next free code in the include
   family) rather than reusing Q-16-3. Project stance: not fussy about
   minting new codes; searchability/honesty outweigh catalog size.
3. **Q-5-1 / Q-5-2 stay warnings.** No severity change. The
   requirement is that a missing/circular include reports *its own*
   warning and never the "unrecognized shortcode" warning — which is
   exactly what Phase 3's block removal + tests T3/T4 pin down.

## Related

- Discovered strand: **bd-1fz3vh99** — includes nested inside
  container blocks are silently dropped (linked
  `discovered-from:bd-qpvoamvu`).
- Q-2-10 (the Connect docs' actual inner error) is itself arguably too
  strict for prose apostrophes after plural nouns (`groups' Unique
  IDs` is correct English). Not in scope; mentioned for context.
