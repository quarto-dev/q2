# qmd-syntax-helper: AST-based rules report unparseable files as clean (bd-syntax-helper-parse-masking-w88mhedp)

**Date:** 2026-08-17
**Braid:** bd-syntax-helper-parse-masking-w88mhedp
**Checkout:** main checkout, branch `main` (investigation only — no implementation branch yet)
**Status:** Design aligned 2026-08-17 (see "Design decisions" below) — **pending user go-ahead to implement.**

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

## Design decisions (aligned with user, 2026-08-17)

1. **One synthesized per-file result** when requires-parse rules are skipped,
   not one per rule. (User: needed given #2; otherwise indifferent.)
2. **Third summary state, declared per rule.** "Unanalyzable" is tracked as a
   distinct state, but refusal is **opt-in per rule**: many rules *expect*
   unparseable input (grid-tables; every diagnostic-driven `q_2_*` rule reads
   the parse error stream as its input) and must keep running on unparseable
   files. Only rules that need a successful AST declare that they require one.
3. **Hard per-file refusal for requires-parse rules in convert**, never
   aborting the multi-file sweep; refusal is judged **at convergence** (an
   earlier rule in the same run may repair the parse, after which the AST rule
   runs normally — the compounding is preserved by re-probing each iteration).
4. **`q-2-30` folded in** — it becomes a requires-parse rule like the two
   bracket rules.
5. **No wire-shape compatibility requirement.** Verified: no JSON schema for
   `check --json` exists anywhere in the repo (the output is undocumented
   serde-serialized `CheckResult` lines), so there is nothing to keep fresh
   and we don't create one here. New optional fields are fine.

### Mechanism (assessed: moderate, fits existing architecture)

The `Rule` trait already has the "rule declares a property, driver interprets
it" precedent in `opt_in_only()`. The change follows the same shape:

- **`Rule::requires_parse(&self) -> bool { false }`** — overridden to `true`
  by `reference-links`, `literal-brackets`, `q-2-30`.
- **Shared parse probe.** Hoist `ParseChecker::check_parse` into a shared
  helper (it already returns the diagnostics). The check driver probes once
  per file *only when* at least one requested rule requires parse; the convert
  driver probes once per iteration under the same condition.
- **Check driver** (`main.rs`): on probe failure, skip requires-parse rules,
  run everything else normally, and synthesize one per-file `CheckResult`:
  `has_issue: false`, new fields `unanalyzable: true` +
  `skipped_rules: [names]`, `error_codes` from the probe, message
  "file failed to parse (Q-2-10); N rule(s) not applied: …".
- **Summary** (`print_check_summary`): new "Unanalyzable" bucket.
  Clean = no findings AND nothing skipped; success rate = clean/total. A file
  with both findings and skipped rules counts as with-issues *and* is listed
  in the unanalyzable line (the two lines answer different questions:
  "what needs fixing" vs "what did the sweep actually cover").
- **Convert driver**: skip requires-parse rules while the probe fails in the
  current iteration; after convergence, if any requested requires-parse rule
  was never applied because the file still doesn't parse, print a per-file
  `✗ … not applied: file does not parse (Q-2-10)` and continue the sweep.
- **`analyze()` hardening**: change `Err(_) => Ok(Analysis::default())` to
  propagate the parse diagnostics as an `Err` carrying the Q-codes, so no
  future caller can silently reintroduce the masking. In driver flow the
  probe runs first, so the rules only hit this path on direct library use.
- **Secondary fix (discovered):** the check loop's `Err` arm
  (`main.rs:116-125`) prints to stderr but still counts the file **clean** in
  the summary — same masking family. Fold in: rule-error files get their own
  small "Errors: N" accounting, excluded from clean.

## Phases

- **Phase 0 — Test plan (TDD, failing first).** In
  `crates/qmd-syntax-helper/tests/integration/` (new `parse_masking_test.rs`,
  registered in `main.rs`, alphabetized):
  1. `check -r literal-brackets` on unparseable fixture → not clean; one
     synthesized unanalyzable result with `skipped_rules` containing both the
     rule name and the probe's Q-codes; summary shows Unanalyzable: 1,
     success rate 0%.
  2. Same for `-r reference-links` and `-r q-2-30`.
  3. `check -r grid-tables` (non-requires-parse) on the same fixture → rule
     still runs; no unanalyzable synthesis when no requires-parse rule was
     requested.
  4. `check -r all` → parse rule reports the failure AND requires-parse rules
     are skipped (one synthesized result), diagnostic-driven rules still run.
  5. `--json` output includes `unanalyzable`/`skipped_rules` fields.
  6. Convert: `convert -r literal-brackets` on unparseable file → no edit,
     per-file refusal note, sweep continues to next file (multi-file test).
  7. Convert compounding: `convert -r apostrophe-quotes -r literal-brackets`
     on a fixture whose only parse error is the apostrophe → iteration 1
     fixes the parse, a later iteration applies literal-brackets; no refusal
     note.
  8. `analyze()` unit test: unparseable source → `Err` with Q-codes.
- **Phase 1 — `analyze()` + trait.** `requires_parse()` on `Rule`; `analyze()`
  returns `Err` with diagnostics; shared parse-probe helper; q-2-30 and the
  two bracket rules override `requires_parse()`.
- **Phase 2 — Check driver + summary.** Probe, skip, synthesize, new
  `CheckResult` fields, summary buckets incl. the rule-error accounting fix.
- **Phase 3 — Convert driver.** Per-iteration probe/skip + at-convergence
  refusal report; multi-file sweep never aborts on refusal.
- **Phase 4 — Docs.** `README.md` + `list-rules` note: requires-parse rules
  and the "fix parse errors first (e.g. `convert -r apostrophe-quotes`)"
  workflow, mirroring the connect-docs lesson.

## Work items

- [x] Phase 0: failing integration tests (`parse_masking_test.rs`) + `analyze()` unit test
      — 12 tests written first; verified red 8/12 pre-fix (4 green guards:
      control fixtures, no-probe case, compounding).
- [x] Phase 1: `Rule::requires_parse()`, `analyze()` → `Err`, shared parse probe
      (`utils/parse_probe.rs`), rule overrides (literal-brackets,
      reference-links, q-2-30; `parse` refactored onto the probe).
- [x] Phase 2: check driver skip/synthesize, `CheckResult.unanalyzable`/
      `skipped_rules` fields, summary buckets ("Unanalyzable files",
      "Files with errors") + rule-error accounting.
- [x] Phase 3: convert per-iteration probe + at-convergence refusal (stderr,
      per file, sweep continues).
- [x] Phase 4: docs (README "Rules that require a parsing file" section,
      `list-rules` † marker).
- [x] End-to-end verification through the binary: post-fix transcript at
      `syntax-helper-parse-masking-investigation/post-fix-transcript.txt`
      (unparseable → "Unanalyzable files: 1", success rate 0.0%, JSON record;
      control → both findings). Crate suite 176/176.
- [ ] Full workspace tests + `cargo xtask verify --skip-hub-build` before PR.

## Implementation notes

- The parse rule's JSON `error_codes` are now **deduplicated** (previously one
  entry per diagnostic, so 3× Q-2-10 appeared three times). Wire-shape change
  allowed per design decision 5.
- `q-2-30::convert` now propagates the parse failure instead of returning a
  0-fix "File does not parse" ConvertResult; in driver flow it is skipped
  before convert is called.
- A file whose *probe read* fails (unreadable) is accounted in
  "Files with errors" and its rules are not run.

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
