# Smart punctuation: `...` converts to an ellipsis only when preceded by a word character (bd-ellipsis-not-smart-48bv2pe6)

**Date:** 2026-08-12
**Braid:** `bd-ellipsis-not-smart-48bv2pe6` (bug, p3, label `markdown`)
**Checkout:** invoked on `main` @ `7bcddf61` — **no branch was created.** See "Before implementation" below.
**Status:** Investigation complete — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The root cause is identified and confirmed by three independent
probes; it is a one-line asymmetry in the tree-sitter token-start character class,
*not* the attribute-class routing the strand hypothesized. The remaining decisions
are which of three fix sites to use and how much regeneration fallout to absorb.

## Issue context

Filed 2026-08-12 by "Claude (q2-connect-docs)", still `open`, priority 3, type `bug`.
Very fresh — no staleness concerns, and the description's evidence table reproduces
exactly at HEAD.

Pandoc's `smart` extension rewrites every run of three dots to U+2026. q2 does it only
when the dot run follows a word character. Curly quotes, en dash and em dash are correct
in every position. Real-world hit: the Connect docs write "the **...** menu" on
`user/api-keys` and `user/plumber`, which Quarto 1 renders with a real ellipsis.

## Dependency graph

**Empty.** `braid dep tree` and `braid dep list` both return no edges. The origin
strand `br-ksfjyxaa` lives in a *different* skein (q2-connect-docs) and is not
reachable from this one, so the usual `discovered-from` context has to come from the
description — which is unusually thorough, so little is lost.

No incoming `blocks` edges means nothing is waiting on this. The urgency is entirely
"prose-common construct renders wrong", which the p3 reflects.

## What the code looks like today

Everything the description points at still exists with the same shape. Full evidence
in `ellipsis-not-smart-investigation/findings.md`; summary:

The description's root-cause *area* is right — the defect is upstream of
`apply_smart_typography` (`crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:161`),
which is correct as written and should not change. But the description's stated
hypothesis is wrong, and it matters because it points at the wrong file.

**The hypothesis (attribute-class routing) is ruled out.** `apply_smart_typography` *is*
reached in the failing positions. The problem is that it never sees more than one dot at
a time: tree-sitter emits ` ... ` as **three separate single-character `pandoc_str`
nodes**, conversion runs per node (a run of 1 correctly stays literal), and `merge_strs`
concatenates them into `...` *afterwards*.

Root cause is `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:99`:

```js
const startStrRegex = regexOr(
    "[" + PANDOC_NON_ASCII_WHITESPACE + PANDOC_ALPHA_NUM + PANDOC_SMART_QUOTES + "-]");
```

`-` is in the token-*start* class; `.` is not. The *continuation* class (line 130)
contains both. So a token starting with `-` swallows the whole run (`--` is one node,
which is why the dash control in the strand passes), a token starting with alnum
swallows `a...b`, but a dot can only match the single-char alternative `"[>.,;!?]"`
(line 127) — one node per dot.

Three probes confirm it (`ellipsis-not-smart-investigation/positions.qmd`):

| input | output | |
|---|---|---|
| `x -... y` | `-…` | hyphen starts the token, dots ride along |
| `x ‘... y` | `‘…` | smart quote starts the token |
| `x a... y` | `a…` | alnum starts the token |
| `x ... y` | `...` | `.` cannot start a token — **the bug** |
| `x .... y` | `....` | should be `….` per Pandoc |

`x -... y` → `-…` is decisive: prefixing a hyphen *repairs* the ellipsis. No
attribute-routing story explains that; the token-start class does.

**Test-coverage gap** (the strand guessed this correctly). `tests/snapshots/native/smart-typography.qmd`
only has word-adjacent dot runs (`ellipsis...`). `tests/roundtrip_tests/qmd-json-qmd/dashes_spaced.qmd`
*looks* like it covers the space-preceded case but its source already contains U+2026 —
it tests round-tripping, not conversion.

Pre-flight `cargo xtask verify --skip-hub-build` passed at `7bcddf61`.

## Proposed phases (draft)

- **Phase 0 — Tests first (TDD).** Add the position dimension: a tree-sitter grammar
  test asserting a dot run lexes as one `pandoc_str` node at token start; extend
  `smart-typography.qmd` (or add a sibling fixture) with space-preceded, paren-preceded,
  block-start and `**...**` cases; keep the escaped-run rows. Verify they fail.
- **Phase 1 — Grammar fix.** One of the three options below, then
  `tree-sitter generate; tree-sitter build; tree-sitter test` in
  `crates/tree-sitter-qmd/tree-sitter-markdown`.
- **Phase 2 — Regeneration fallout.** Re-run `./scripts/build_error_table.ts` if parser
  states shifted (see Risks); review every changed snapshot per the CLAUDE.md
  snapshot-reporting rule.
- **Phase 3 — Full verification.** `cargo nextest run --workspace`, then
  `cargo xtask verify` (WASM leg included — pampa feeds `wasm-qmd-parser`), plus the
  end-to-end check: `cargo run --bin q2 -- render` on a fixture containing "the ... menu"
  and inspect the HTML.

## Open design questions for the user

1. **Which fix site?** Three options, in my order of preference:

   - **(B) — recommended.** Add a dedicated dot-run alternative to `PANDOC_REGEX_STR`
     (e.g. `"[.]+"` ahead of the `"[>.,;!?]"` alternative at line 127). Surgical: only
     consecutive dots group. `.class` still lexes as `.` + `class`, so nothing near the
     attribute grammar moves. The escape invariant survives untouched because each `\.`
     node holds exactly one dot.
   - **(A)** Add `.` to `startStrRegex`. One character, but a dot-led token would then
     absorb alnum and dashes — `.class` becomes a single token, which is adjacent to the
     `{.class}` / `[x]{.class}` attribute rules. Bigger blast radius for no extra benefit.
   - **(C)** Leave the grammar alone; coalesce adjacent single-dot nodes in the reader
     before conversion. Fights the deliberate per-node design documented at
     `text_helpers.rs:156`, and has to re-derive the escaped/unescaped distinction that
     the grammar already encodes.

   Do you want B, or do you see a reason to prefer another?

2. **Is `....` → `….` in scope?** Pandoc's rule (three at a time, remainder literal)
   gives `….` for four dots. It falls out of B for free and matches
   `apply_smart_typography` as already written, so I'd include it — but it is a second
   user-visible behavior change beyond what the strand reports. Confirm?

3. **How far should the probe sweep go before committing to B?** Grouping dot runs into
   one token touches any construct where consecutive dots appear: relative paths
   (`../foo`) inside link destinations, `..` in prose, a line *starting* with dots, dots
   inside pipe-table cells and attribute braces. I'd probe each before implementing.
   Anywhere else you want covered?

4. **Test placement.** Extend the existing `smart-typography.qmd` snapshot (one fixture,
   bigger diff) or add a sibling `smart-typography-positions.qmd` (isolates the new
   dimension, cleaner review)? And do you want a `qmd-json-qmd` roundtrip row starting
   from the *unconverted* `...` to close the gap `dashes_spaced.qmd` left?

## Risks / tradeoffs (draft)

- **Parser regeneration is the main risk, not the fix.** Changing `grammar.js` shifts
  the generated LR table, and the error-message infrastructure
  (`resources/error-corpus/_autogen-table.json`) maps *integer parse states* to
  diagnostics. State renumbering can silently redirect error messages. Phase 2 must
  re-run `./scripts/build_error_table.ts` and diff the result, and Phase 0's tests
  cannot catch a regression here — the error-corpus tests can.
- **Snapshot churn.** Any fixture with a space-preceded `...` in prose will change
  output. Expected and desirable, but per CLAUDE.md every changed `.snap` must be
  counted, summarized, and surprises flagged before the commit lands.
- **Grammar design invariant.** `grammar.js:136` declares "no `conflicts:`" — the
  grammar must stay deterministic LR. Option B adds an alternative to an existing
  token regex, resolved by longest-match in the lexer, so it should not introduce a
  conflict; `tree-sitter generate` will complain loudly if it does. Worth watching for.
- **WASM leg.** `pampa` feeds `wasm-qmd-parser`, so the full `cargo xtask verify`
  (not `--skip-hub-build`) is required before this can be considered done.

## Before implementation

This investigation ran on `main`. The plan-skeleton commit lands there; **implementation
should move to its own branch** — either `cargo xtask create-worktree bd-ellipsis-not-smart-48bv2pe6`
for an isolated worktree, or a plain topic branch. Your call, not something this
investigation should decide.
