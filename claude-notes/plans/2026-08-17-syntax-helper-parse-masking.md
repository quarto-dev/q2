# qmd-syntax-helper: AST-based rules report unparseable files as clean (bd-syntax-helper-parse-masking-w88mhedp)

**Date:** 2026-08-17
**Braid:** bd-syntax-helper-parse-masking-w88mhedp
**Checkout:** main checkout, branch `main` (investigation only — no implementation branch yet)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug is confirmed at HEAD with an in-tree repro, the
root cause is exactly where the strand says it is, and the affected-rule set is
now fully enumerated (it is slightly larger than the strand states: `q-2-30` is
masked the same way). The remaining work is a design decision about *how* the
"unanalyzable" state should surface in check output, summary counts, and
convert — questions below.

## Issue context

Filed today (2026-08-17) by Carlos, p2 bug, labels `diagnostics`,
`syntax-helper`. The AST-based rules (`reference-links`, `literal-brackets`,
anything on `bracket_analysis::analyze`) report a file that fails to qmd-parse
as **clean**: `check -r literal-brackets -r reference-links <file>` prints ✓,
counts it as a clean file, and folds it into "Success rate: 100.0%". The
operator of a scoped sweep gets an actively misleading all-clear — the rules
never saw an AST at all.

Real-world hit: during the Connect-docs port, a pre-migration
`admin/security/index.md` failed to parse (3× Q-2-10 stray apostrophes), so a
bracket sweep reported it clean while it contained four literal `[1]`–`[4]`
brackets — exactly the content `literal-brackets` exists to save. Origin
strand: `br-syntax-helper-parse-masking-fmwvbmq8` in the q2-connect-docs skein.

## Dependency graph

**Empty** — no edges in either direction in the q2 skein. The
`discovered-from` context lives in a *different* skein (q2-connect-docs,
`br-syntax-helper-parse-masking-fmwvbmq8`), summarized in the description.
No incoming pressure; the strand is self-contained and freshly filed, so no
staleness concerns.

## What the code looks like today

All paths from the description exist unchanged at HEAD (`0876e413`).

**Root cause, confirmed** — `crates/qmd-syntax-helper/src/conversions/bracket_analysis.rs:165`:

```rust
let (doc, _ctx, _diags) = match parsed {
    Ok(triple) => triple,
    Err(_) => return Ok(Analysis::default()),   // parse failure ⇒ "clean"
};
```

The doc comment on `analyze()` (lines 153–157) shows this was a *deliberate*
choice for **convert** safety ("the rules leave it alone rather than editing a
document whose structure we cannot trust. Parse errors are the `parse` rule's
business, not ours") — but the same empty `Analysis` also flows into **check**,
where "leave it alone" becomes "report clean".

**Full masked-rule inventory** (rules that need a successful AST and swallow
`Err`):

| Rule | Call site | Err handling |
|---|---|---|
| `reference-links` | `reference_links.rs:142` (check), `:209` (convert) | via `analyze()` → default = clean |
| `literal-brackets` | `literal_brackets.rs:82` (check), `:133` (convert) | via `analyze()` → default = clean |
| `q-2-30` | `diagnostics/q_2_30.rs:70` | `Err(_) => return Ok(Vec::new())` — same masking, independent of `analyze()` |

**Not affected:** the ~20 `q_2_*` conversion rules, `apostrophe-quotes`,
`attribute-ordering`, `div-whitespace` are *diagnostic-driven* — a parse `Err`
is their **input** (they scan the returned diagnostics for their code), so
`Ok(_) => clean` is correct for them. `parse` and `syntax` obviously report
failures. `grid-tables`/`definition-lists` are text-based.

**Reporting machinery** (`main.rs`): `CheckResult` (rule.rs:18) has only
`has_issue: bool`; `print_check_summary` (main.rs:425) derives
`files_clean = total_files − files_with_issues` and the success rate. There is
**no third state** — any fix that wants "unanalyzable ≠ clean ≠ has-findings"
in the summary needs either a new `CheckResult` flavor or a synthesized
parse-failure result.

**Convert-side wrinkles** (both matter for the design):

1. `main.rs:252-260` — a rule `convert()` returning `Err` **aborts the whole
   run** (`return Err(e)`), skipping all remaining files. So "make analyze
   return the error" naïvely turns one bad file into a dead sweep.
2. The convert iteration loop (main.rs:217-310) applies all requested rules
   repeatedly until convergence. Under `convert -r apostrophe-quotes -r
   literal-brackets`, iteration 1's apostrophe fix can *make the file
   parseable*, letting iteration 2's AST rule see it. A hard refusal on first
   parse failure would break this useful compounding; the failure only matters
   if the file **still** doesn't parse when the rule last ran.

### Repro (in-tree, confirmed at HEAD)

`claude-notes/plans/syntax-helper-parse-masking-investigation/` holds
`input.qmd` (Q-2-10 stray apostrophe + one literal `[1]` + one `[the
docs][gcc]` reference + definition) and `input-parseable.qmd` (identical,
apostrophe escaped). Observed with
`cargo run -p qmd-syntax-helper -- check -r literal-brackets -r reference-links <file>`:

- `input.qmd` → **0 issues, "Success rate: 100.0%"** (the bug)
- `input-parseable.qmd` → 2 findings (literal bracket + reference link)

(Repro transcript in `repro-transcript.txt` in the same directory.)

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Integration tests in
  `tests/integration/bracket_analysis_test.rs` (+ a new
  parse-masking test module): unparseable file under `check -r
  literal-brackets` / `-r reference-links` / `-r q-2-30` must NOT be reported
  clean; summary counts must not include it in the success rate; convert must
  not silently no-op; `convert -r all`-style iteration where an earlier rule
  repairs the parse must still apply the AST rule afterward.
- **Phase 1 — Make `analyze()` honest.** Return the parse failure (e.g.
  `Result<Analysis, AnalyzeError>` or `Ok(AnalysisOutcome::Unparseable(codes))`)
  instead of defaulting to empty; carry the Q-codes for the message.
- **Phase 2 — Check-side surfacing.** Per design answer: synthesized
  `CheckResult` ("file failed to parse; rule not applied") and/or a third
  summary bucket ("Unanalyzable files: N") excluded from clean count and
  success rate. Dedup when several AST rules run on the same file.
- **Phase 3 — Convert-side behavior.** Skip-with-warning per iteration, final
  post-convergence check: if the file still fails to parse and an AST rule was
  requested, report it (non-✓) without aborting the multi-file sweep.
- **Phase 4 — `q-2-30` alignment.** Apply the same treatment to its
  independent `Err(_) => clean` site.
- **Phase 5 — Docs.** Helper's user-facing docs / `--help` note: AST-based
  rules require a parsing file; run `parse` / fix parse errors first.

## Open design questions for the user

1. **Check-output shape.** When an AST rule can't run, should each requested
   AST rule emit its own `CheckResult` ("literal-brackets: file failed to
   parse; rule not applied"), or should the file get **one** synthesized
   parse-failure result regardless of how many AST rules were requested?
   (One-per-rule is simpler to wire through `Rule::check`; one-per-file reads
   better and doesn't inflate "Total issues".)
2. **Summary bucket.** Is "file failed to parse" just another *issue* (file
   counted in "Files with issues", success rate drops — minimal change), or a
   genuine third state ("Unanalyzable: N" line, excluded from both clean and
   issue counts)? The strand's option (b) implies the third state; it touches
   `CheckResult`'s serialized JSON shape, which external consumers (the
   connect-docs sweep scripts) may parse.
3. **Convert semantics.** For `convert -r literal-brackets` on an unparseable
   file: hard per-file refusal with a non-zero note in output (but *not*
   aborting the rest of the sweep), or current silent no-op plus a
   warning? And do you agree the check should be "still unparseable at
   convergence" rather than "unparseable at iteration 1" (to preserve the
   fix-then-apply compounding with e.g. `apostrophe-quotes`)?
4. **Scope: `q-2-30`.** The strand names only `analyze()`-based rules; q-2-30
   has the identical masking independently. Fold it into this fix (my
   recommendation) or file it separately?
5. **JSON stability.** `check --json` emits `CheckResult` lines consumed by
   scripts. May we add a new optional field (e.g. `"unanalyzable": true`) /
   new rule_name value, or must the wire shape stay byte-compatible?

## Risks / tradeoffs (draft)

- **Convert abort semantics** (main.rs:252) are a pre-existing sharp edge: any
  `Err` from convert kills the whole multi-file run. Phase 3 must avoid
  routing the new failure through that path, or deliberately change it —
  flagging because it's a behavior change beyond the strand's ask.
- **Success-rate consumers**: the connect-docs workflow reads the summary; any
  change to counting semantics should be announced in that repo's skein
  (origin strand `br-syntax-helper-parse-masking-fmwvbmq8`).
- The `analyze()` doc comment's convert-safety rationale is still valid — the
  fix must keep "never edit a file we can't parse" while removing "call it
  clean".
