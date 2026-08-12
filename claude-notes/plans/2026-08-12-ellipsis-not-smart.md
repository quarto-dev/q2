# Smart punctuation: `...` converts to an ellipsis only when preceded by a word character (bd-ellipsis-not-smart-48bv2pe6)

**Date:** 2026-08-12
**Braid:** `bd-ellipsis-not-smart-48bv2pe6` (bug, p3, label `markdown`)
**Branch:** `braid/ellipsis-not-smart-48bv2pe6` (off `main` @ `27f96dfb`)
**Status:** **Implemented.** All design questions answered; fix landed, tests green,
verified end-to-end. Pending full `cargo xtask verify` sign-off and user review.

## Triage verdict

**Ready to design** → **done.** The root cause was a one-line asymmetry in the
tree-sitter token-start character class, *not* the attribute-class routing the strand
hypothesized. All four design questions were answered by the user (see "Decisions"),
option B was implemented, and the parser-regeneration risk did not materialize.

## Decisions (user, 2026-08-12)

1. **Fix site: option B**, with the explicit instruction to *start* there and only
   revisit if parser regeneration caused an unforeseen, unfixable cascade. It did not —
   `_autogen-table.json` regenerated with a zero-byte diff and all 11,736 workspace
   tests pass.
2. **`....` → `….` is in scope.** Small user-visible differences are acceptable when
   they move q2 closer to Pandoc. Confirmed against `pandoc -f markdown -t native`,
   which emits `Str "\8230."`.
3. **Probe sweep** as proposed, plus `1.` vs `1...` ordered-list behavior.
4. **Test placement:** new `smart-typography-positions.qmd` fixture, not an extension
   of the existing one.

## Outcome

The fix is a single new alternative in `PANDOC_REGEX_STR`
(`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`), which makes a run of dots
lex as one `pandoc_str` token so `apply_smart_typography` sees the whole run:

```js
"[.]+",
"[>.,;!?]",
```

`apply_smart_typography` itself is unchanged, as predicted.

End-to-end through `cargo run --bin q2 -- render`:

```
<p>Click the <strong>…</strong> menu to open it.</p>
<p>Wait for it… then see (…) here.</p>
<p>Four dots …. and two .. dots.</p>
<p>Escaped a...b stays literal, code <code>...</code> untouched.</p>
```

Output inspected directly in the generated HTML. The first line is the exact Connect
docs construct from the strand.

### Probe-sweep results

Baseline captured in `ellipsis-not-smart-investigation/probe-sweep.qmd`. Every
at-risk construct behaves correctly after the fix:

| construct | after |
|---|---|
| `[x](../foo.html)` link destination | untouched |
| `../foo` in prose | literal `../foo` |
| `..` / `.` in prose | literal |
| `{.class}`, `[y]{.a .b}` | still parse as attribute classes |
| pipe-table cells | `.` and `..` literal, `...` → `…` |
| `1.` ordered list | still a list |
| `1...` | `1…`, not a list (unchanged — was already alnum-led) |
| `` `...` `` code span | untouched |
| `a\.\.\.b` | literal `a...b` |

### Answer to the `1.` vs `1...` question

`1.` still produces `OrderedList (1, Decimal, Period)`. `1...` was never a list and
still is not — it produces `Str "1…"`. Its behavior is *unchanged* by this fix,
because `1...` is alnum-led and so was already lexing as a single token.

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

## Phases

- [x] **Phase 0 — Tests first (TDD).** Added
      `crates/pampa/tests/integration/test_smart_typography_positions.rs` (8 tests) and
      `crates/tree-sitter-qmd/.../test/corpus/dot-runs.txt` (8 grammar cases).
      Verified failing: 4 of 8 Rust tests failed on the position/remainder assertions,
      4 controls (short runs, escaped runs, code spans, dashes) passed from the start —
      exactly the expected split.
- [x] **Phase 1 — Grammar fix (option B).** `tree-sitter generate` reported no
      conflicts, so the no-`conflicts:` design invariant at `grammar.js:136` holds.
      `tree-sitter test`: 590/590.
- [x] **Phase 2 — Regeneration fallout.** `./scripts/build_error_table.ts` produced a
      **zero-byte diff** in `_autogen-table.json`; the error-corpus tests pass. One
      pre-existing grammar test had to change (see Risks).
- [x] **Phase 3 — Full verification.** `cargo nextest run --workspace`: 11,736 passed.
      End-to-end `q2 render` output inspected (above). Full `cargo xtask verify`
      (all 14 steps, WASM leg and hub-client build included): passed.
      `cargo xtask lint`: 958 files, all checks passed.
- [ ] **Phase 4 — Docs.** Probably nothing to write: this restores documented Pandoc
      behavior rather than adding a feature. Flag for user judgment.

## Risks — how each resolved

- **Parser regeneration (the main risk): did not materialize.**
  `./scripts/build_error_table.ts` regenerated `_autogen-table.json` with **no diff at
  all**, so no LR state that the error catalog depends on moved. The error-corpus tests
  pass. This was the one thing that could have forced a retreat to option C; it didn't.
- **Grammar design invariant: held.** `tree-sitter generate` reported no conflicts, so
  the parser is still deterministic LR per `grammar.js:136`.
- **Snapshot churn: none, except one deliberate change.** No pre-existing `.snap` file
  changed — notably `smart-typography.snap` is byte-identical, confirming word-adjacent
  behavior is untouched. Exactly one new snapshot was added.
- **One pre-existing grammar test had to change.** `test/corpus/punctuation-vs-image.txt`
  case 5 ("multiple punctuation marks") feeds the literal input `...` and asserted
  **three** `pandoc_str` nodes — i.e. its expectation encoded the buggy tokenization
  directly. It now asserts one node. This is the only pre-existing test touched, and the
  change is forced by the fix rather than incidental. Flagged per the repo rule about not
  editing tests you did not write.

## Follow-up worth considering (not in scope here)

`## Heading with ... dots` produces the identifier `heading-with-...-dots` in q2, both
before and after this fix. Pandoc produces `heading-with-dots` — it strips the
punctuation. So q2's heading-id algorithm diverges from Pandoc on punctuation, which is
a *separate* pre-existing defect this investigation surfaced but did not touch. Note
that the fix does change the *text* of such a heading to use U+2026 while leaving the
id spelled with ASCII dots. Worth its own strand if the user agrees.
