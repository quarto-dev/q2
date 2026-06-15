# Em-dash / en-dash parsing + canonicalization

**Strand:** bd-k2h1x7bu
**Date:** 2026-06-15
**Status:** PLAN — awaiting go-ahead before implementation

## Overview

Quarto 2's treatment of smart dashes is broken in two distinct ways, both
reproducible on `~/Desktop/daily-log/2026/06/15/test.qmd`:

1. **Parsing (read side):** `---` / `--` are only converted to em-/en-dashes
   when they appear as a *standalone whitespace-delimited token*. Mid-word
   runs (`un---spaced`, `en--dash`, `em-dashes---like`) are left as literal
   ASCII and never become `—` / `–`. This diverges from Pandoc's `smart`
   extension, which converts dash runs anywhere.

2. **Writing (round-trip side):** a unicode em-dash `—` (U+2014) or en-dash
   `–` (U+2013) in the AST is emitted verbatim by the qmd writer instead of
   being canonicalized back to `---` / `--`. We already canonicalize smart
   quotes back to ASCII on the write side (`’` → `'`); dashes should follow
   the same rule, for the same reason — keep `.qmd` source ASCII-clean and
   diff-friendly, never store smart typography literally in source.

The desired end state: em-dashes canonicalize to `---` and en-dashes to `--`
in `.qmd` source, while still rendering as proper unicode dashes in output
formats (HTML, etc.). Round-tripping qmd → AST → qmd must be idempotent.

## Reproduction

Input (`test.qmd`, hex-verified):

- Line 1 uses ASCII `---` mid-word: `em-dashes---like this one---are fine. (others, though)---like this one---aren't.`
- Line 3 uses literal unicode em-dashes (`U+2014`) with surrounding spaces.

```
$ cargo run --bin pampa -- test.qmd            # native AST
# line 1: Str "em-dashes---like", Str "one---are", ... ---  NOT converted
# line 3: ... Space, Str "—", Space, ...                ---  unicode passed through

$ cargo run --bin pampa -- -t markdown test.qmd  # round-trip
# line 1: ASCII --- preserved literally (never became a dash, never canonicalized)
# line 3: unicode — preserved literally (should have become ---)
```

Minimal synthetic confirmation:

```
$ printf 'spaced --- dash and en -- dash here\n\nun---spaced and en--dash\n' \
    | cargo run --bin pampa -- -
# [ Para [Str "spaced", Space, Str "—", ... Str "–" ...],   <- spaced: converted
#   Para [Str "un---spaced", Space, Str "and", Space, Str "en--dash"] ]  <- unspaced: NOT

$ ... | cargo run --bin pampa -- -t markdown -
# spaced — dash and en – dash here         <- unicode NOT canonicalized to ---/--
# un---spaced and en--dash                  <- still literal
```

## Root cause (file:line)

### Bug 1 — read side, whole-token gating

`merge_strs` runs over inline runs after the tree-sitter parse and applies
`as_smart_str` to each `Str`:

- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1680` —
  `as_smart_str(s: String)` converts only when the **entire string equals**
  `"--"`, `"---"`, or `"..."`:

  ```rust
  fn as_smart_str(s: String) -> String {
      if s == "..." { "…".into() }
      else if s == "--" { "–".into() }
      else if s == "---" { "—".into() }
      else { s }
  }
  ```

- Called at `postprocess.rs:1705` inside `merge_strs`, on each `Str` token
  *before* merging. Tree-sitter emits `un---spaced` as a single `pandoc_str`
  node (verified with `-v`), so the whole-token equality never matches and the
  dash run survives. A space-delimited `---` *is* its own `pandoc_str` node, so
  it matches — hence the asymmetry.

Contrast with smart quotes, which are NOT whole-token gated:

- `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:147` —
  `apply_smart_quotes` does `text.replace('\'', "\u{2019}")` over the whole
  text, so `don't` → `don’t` works mid-word. Dashes should use the same
  substring-scanning approach.

  `apply_smart_quotes` is applied at the base-text construction site
  (`treesitter.rs:566`, `:740`, `:774`, `editorial_marks.rs:62`).

### Bug 2 — write side, no dash canonicalization

The qmd writer reverses smart quotes but not dashes:

- `crates/pampa/src/writers/qmd.rs:1382` — `reverse_smart_quotes(text)` does
  `text.replace('\u{2019}', "'")` only.
- `crates/pampa/src/writers/qmd.rs:1460` — `write_str` calls
  `reverse_smart_quotes` then `escape_markdown`. Em/en dashes are not in the
  escape set (`qmd.rs:1409`–`1458`), so they fall through verbatim
  (`_ => result.push(ch)`).

### Not a gate: the `smart` extension is parsed but unused

`crates/pampa/src/options.rs` parses `+smart` / `-smart`, but neither
`merge_strs`/`as_smart_str` nor `apply_smart_quotes` consult it — qmd treats
smart typography as **always on**. This plan preserves that behavior (does not
wire up `-smart`); see Open Questions.

## Design decisions

> All three open questions are now RESOLVED (see end of doc). Decisions are
> baked into D1–D5 below.

### D1. Where to convert dash runs (read side) — MUST be per-node, pre-merge

Move dash + ellipsis conversion from whole-token `as_smart_str` to a substring
scan, folded together with quotes into a single
`apply_smart_typography(text)` in `text_helpers.rs` (handles quotes, dashes,
ellipsis uniformly). Apply it at exactly the four sites that call
`apply_smart_quotes` today:

- `treesitter.rs:566` (IntermediateBaseText → Str)
- `treesitter.rs:740` and `:774` (pandoc_str)
- `editorial_marks.rs:62`

and **drop** the `--`/`---`/`...` branches from `as_smart_str` in `merge_strs`
(turning that back into a plain concatenation/merge).

**CRITICAL — why per-node and not post-merge.** Escaped hyphens are *never*
inside a multi-char prose node. Tree-sitter emits each `\-` escape as its own
2-byte `pandoc_str` node (verified with `-v`: `a \-\-\- b` →
`str "a"`, `space`, `str "\-"`, `str "\-"`, `str "\-"`, `space`, `str "b"`).
A regular multi-char prose node therefore contains no backslashes at all. This
is exactly why the conversion is safe per-node and **unsafe post-merge**:

- `un---spaced` is one node `un---spaced` → run of 3 → `un—spaced`. ✓
- `a\-\-b` arrives as nodes `a`,`\-`,`\-`,`b` → each becomes `a`,`-`,`-`,`b`,
  none a run ≥2 within a node → merge to literal `a--b`. ✓ (escape preserved)
- If we instead scanned the *merged* string `a--b`, we'd wrongly produce
  `a–b`. (Confirmed today's output is `a--b`; this must not regress.)

The single-hyphen rule (`-` never converts) falls out for free: a `\-` node is
a length-1 run after backslash processing.

**Code/verbatim safe for free.** Those four sites are the prose-str surface
only; inline `Code` / code blocks / attrs / ids are different node types and
never reach them. Verified: `` `a---b` `` and `` `it's` `` are untouched today.
Mirroring the sites keeps dashes equally scoped. (Test it anyway.)

### D2. Dash-run algorithm (read side) — Pandoc study + chosen rule

Studied `external-sources/pandoc/src/Text/Pandoc/Parsing/Smart.hs:167` (the
`dash` parser, default mode — `Ext_old_dashes` off, which is our case):

```haskell
dash = try $ do
  string "--"                       -- require ≥2 hyphens
  (char '-' >> return (str "\8212")) -- a 3rd hyphen present → EM DASH (—)
    <|> return (str "\8211")         -- otherwise → EN DASH (–)
```

`dash` is one of the inline smart parsers tried at every position, so a run of
hyphens is consumed left-to-right, greedily taking **3 (em)** whenever ≥3
remain, else **2 (en)** when exactly 2 remain, else a lone **1** stays a
literal hyphen. Worked examples (N = run length):

| N | Pandoc output | note |
|---|---------------|------|
| 1 | `-`           | literal hyphen |
| 2 | `–`           | en |
| 3 | `—`           | em |
| 4 | `—-`          | em + leftover hyphen |
| 5 | `—–`          | em + en |
| 6 | `——`          | em + em |
| 7 | `——-`         | em + em + hyphen |

(`Ext_old_dashes`: `--`→em, single `-`→en only before a digit. We do **not**
implement old-dashes.)

**Chosen rule (simple, faithful):** scan each prose node for maximal runs of
`-`; consume the run greedily 3-at-a-time as em-dash while ≥3 remain, then a
trailing 2 as en-dash, leaving a lone 1 as hyphen. This is ~10 lines and
matches the table exactly. Per the user, exact Pandoc parity is low priority;
this rule happens to *be* Pandoc's, so we take it. `...` → `…` (U+2026) by the
same scan.

### D3. Write-side canonicalization

Extend the write path to map smart chars back to ASCII, mirroring
`reverse_smart_quotes`. Rename/extend it to `reverse_smart_typography` (or add
a sibling) in `qmd.rs`, applied in `write_str` before `escape_markdown`:

- `—` (U+2014) → `---`
- `–` (U+2013) → `--`
- `…` (U+2026) → `...`   *(ellipsis IN SCOPE — Q1 resolved yes)*

Round-trip is a fixed point: writing `——` (from N=6) → `------` → reparse →
em+em → `——`; `—–` (N=5) → `-----` → em+en → `—–`. Stable.

### D4. Round-trip hazard — em-dash on an all-dash line — resolved via `\—`

Canonicalizing `—` → `---` is unsafe **only** when the emitted dashes form a
line whose entire content is `-` characters totalling ≥3 — that re-parses as a
thematic break (HorizontalRule). Empirically scoped (all verified with the
binary):

- Lone `---` paragraph → `HorizontalRule`. **← the one real hazard.**
- Lone `--` paragraph → `Para [Str "–"]` (2 hyphens < thematic-break minimum;
  **safe**). En-dash never needs escaping on a line by itself.
- `…`/`...` is never block-significant. **safe.**
- qmd has **no** setext headings: `text\n===` → one para w/ softbreak;
  `text\n---` → para + HorizontalRule (the `---`-as-HR case again, not setext);
  `text\n-` → para + empty bullet list. So the only thing to guard is an
  all-dash line of ≥3 hyphens.

**Resolution (Q3):** in that position the writer emits **`\—`** — a backslash
followed by the *unicode* em-dash. A line starting with `\` is not a thematic
break, and the reader strips the backslash (D5) to recover a single `—`. This
keeps the em-dash *semantics* (an all-dash ASCII line cannot both render as a
dash and be block-safe — `---` is the only ASCII spelling and it *is* the
dangerous string; `\-\-\-` would instead suppress conversion and render literal
`---`). The lone-`—` round-trip is then a fixed point:
`Para[Str "—"]` → `\—` → reparse → `Para[Str "—"]`.

Writer rule: when the dashes being emitted would constitute an entire block
line of ≥3 hyphens, escape by emitting `\` + the unicode dash char for the
leading dash (simplest: detect "this Str is the sole/leading content of a
paragraph and reduces to an all-hyphen line"). This is the fiddliest part;
the idempotency round-trip tests (esp. `dashes_lone_emdash.qmd`) are the guard.

### D5. Reader: extend backslash-escape set to smart-typography chars

Today `process_backslash_escapes` (`text_helpers.rs:160`) only strips a
backslash before **ASCII punctuation** (and `\<space>`→nbsp); a backslash
before any other char is kept literally — so `\—` currently parses as the
two-char `Str "\—"` (verified). Per the user, **extend the escapable set to at
least em-dash (U+2014), en-dash (U+2013), and ellipsis (U+2026)** so that `\—`
→ `—`, `\–` → `–`, `\…` → `…`. This is what makes D4's `\—` round-trip work,
and it generalizes the "backslash forces the next char to appear literally"
intent to the smart-typography output chars.

- Tokenization is already fine: tree-sitter's `\\.` escape regex matches
  `\—` as a single 2-char node (verified), so only `process_backslash_escapes`
  needs the new branch.
- Strictly, only `\—` is *required* for the D4 hazard; en-dash and ellipsis are
  never block-significant. We add all three for a consistent rule (user
  direction: "extend … to em-dash, en-dash and ellipsis at least").
- **Smart *quote* chars (`’ ‘ “ ”`): explicitly SKIPPED** (user decision,
  2026-06-15) — backslash-escaping smart quotes can interact with apostrophe
  and `Quoted` parsing in ways we don't want to take on now. The writer already
  reverses `’`→`'`, so quotes never need escaping on output regardless.
- Note a minor, deliberate divergence from Pandoc, which keeps `\—` literal.
  We control the qmd dialect; this is acceptable.

## Test plan (TDD — write tests first, watch them fail)

Per repo policy, every test below is added and confirmed RED before any fix.

### Read-side (parsing)

1. **Unit tests** for `apply_smart_typography` in `text_helpers.rs`: table of
   inputs → expected, covering mid-word (`un---spaced`→`un—spaced`), the N=1..7
   run table from D2, `...`→`…`, and a single intra-word hyphen left intact.
2. **Unit tests** for the extended `process_backslash_escapes` (D5):
   `\—`→`—`, `\–`→`–`, `\…`→`…`, and that existing ASCII-punct escapes still
   work.
3. **Parse fixtures** — small `.qmd` files asserting `un---spaced` →
   `Str "un—spaced"`, `en--dash` → `Str "en–dash"`, **`a\-\-b` stays `a--b`**
   (escape preservation — the per-node regression guard from D1), and
   `` `a---b` `` (inline code) stays literal.

### Write-side (canonicalization)

4. **Unit tests** for `reverse_smart_typography`: `—`→`---`, `–`→`--`,
   `…`→`...`.
5. **Writer fixture** under `crates/pampa/tests/writers/markdown/`: an AST
   containing unicode dashes writes ASCII `---`/`--`, and an AST that is
   `Para[Str "—"]` writes `\—` (the D4 escape).

### Round-trip (idempotency)

6. **`qmd-json-qmd` fixtures** — drop `dashes_unspaced.qmd`,
   `dashes_spaced.qmd`, `dashes_lone_emdash.qmd` (the D4 hazard, expect `\—`),
   `dashes_escaped.qmd` (`a\-\-b` stays literal), and `dashes_runs.qmd` (N=4..7)
   into `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`. The existing
   `test_qmd_roundtrip_consistency` harness (`tests/integration/test.rs:726`)
   asserts JSON1 == JSON3 (idempotency), which directly catches the D4 hazard
   and any non-stable conversion.

### End-to-end

6. Exercise through the binary and inspect output, per CLAUDE.md E2E policy:
   - `cargo run --bin pampa -- test.qmd` shows `—` in all four positions.
   - `cargo run --bin pampa -- -t markdown test.qmd` shows `---`/`--`
     everywhere (no unicode dashes), and is byte-stable when fed back in.
   - Confirm HTML output (`-t html`) renders proper `—`/`–`.

7. **Full workspace regression:** `cargo nextest run --workspace` — the smart
   transform touches a hot path; existing snapshot/round-trip suites will flag
   collateral changes. Expect some snapshot churn (documents that previously
   kept literal `---` mid-word will now show `—`); review every changed `.snap`
   per the snapshot-change policy in CLAUDE.md and report counts.

## Implementation checklist

- [x] Phase 0 — Pandoc hyphen-run rule studied; N=1..7 table recorded (D2).
- [x] Phase 1a (read unit tests RED→GREEN) — `smart_typography_tests` in
      `text_helpers.rs`: dash-run table N=1..7, mid-word, ellipsis runs,
      apostrophe, and `\—`/`\–`/`\…` escapes. 14 tests.
- [x] Phase 2 (read fix) — implemented `apply_smart_typography` in
      `text_helpers.rs` (quotes + greedy dash runs + ellipsis); wired into the
      four prose-str sites (`treesitter.rs:566,740,774`, `editorial_marks.rs:62`);
      removed the dash/ellipsis branch from `as_smart_str` (function deleted) and
      removed now-dead `apply_smart_quotes`. **Per-node, never post-merge** (D1).
      Verified end-to-end on the binary: mid-word, N=4/5/7 runs, `a\-\-b`
      literal, code untouched, `dots…`, `\-\-\-`→`---`.
- [x] Phase 3 (reader escapes) — extended `process_backslash_escapes` +
      `is_escapable_smart_char` to strip a backslash before em/en-dash/ellipsis
      (D5).
- [x] Phase 1b (write/round-trip tests) — `smart_typography_writer_tests` in
      `qmd.rs` (escape_markdown canonicalization + literal-run escaping + hazard
      detection), `smart-typography.qmd` native snapshot, and five
      `qmd-json-qmd/dashes_*.qmd` idempotency fixtures.
- [x] Phase 4 (write fix) — folded smart-typography canonicalization +
      literal-run escaping into `escape_markdown` (operates on original text so
      it distinguishes a canonicalized em dash `—`→bare `---` from literal
      hyphens `--`→`\-\-`); removed `reverse_smart_quotes`. Added
      `line_is_dash_only_hazard` + `write_prose_inlines`/`write_prose_line` +
      `suppress_dash_canonicalization` ctx flag for the D4 all-dash-line escape
      (`\—`, `\-\-\-`). **Discovered & fixed a latent gap the read change
      exposed:** literal hyphen runs (≥2) and dot runs (≥3) in a Str must be
      escaped or the reader re-smart-converts them (`a\-\-b` → `a–b`).
- [x] Phase 5 — write-side + round-trip tests GREEN. 12-case binary round-trip
      sweep all stable (md idempotent AND AST stable), incl. lone em dash→`\—`,
      escaped literals, blockquote, en-dash-safe, ellipsis.
- [x] Phase 6 — full workspace `cargo nextest run --workspace`: **10065
      passed**, 197 skipped. Snapshot churn: **2 native snapshots** (012, 014),
      both the intended mid-token conversion (`1--30`→`1–30`,
      `Hello---maybe---world...`→`Hello—maybe—world…`,
      `wait---really---did`→`wait—really—did`); regeneration also corrected a
      stale `source:` path in both. No other snapshot in the tree changed.
- [x] Phase 7 — E2E verification on `~/Desktop/daily-log/2026/06/15/test.qmd`,
      output inspected:
      - `pampa test.qmd` (native): all four unspaced `---` → `—`
        (`em-dashes—like`, `one—are`, `though)—like`, `one—aren’t`); spaced
        em dashes also `—`.
      - `pampa -t markdown test.qmd`: every dash canonicalized to ASCII `---`,
        apostrophe back to `'`; byte-stable when re-fed (idempotent).
      - `pampa -t html test.qmd`: renders `—` / `–` (real Unicode dashes).
      - 12-case round-trip sweep (lone em dash→`\—`, escaped literals,
        blockquote, en-dash-safe, ellipsis, runs N=4..7) all md-idempotent AND
        AST-stable.
- [x] Phase 8 — `cargo xtask verify --skip-hub-build`: **✓ All verification
      steps passed!** (includes the `-D warnings` clippy gate and the
      q2-preview-spa build, which rebuilt `wasm_quarto_hub_client` — so the WASM
      leg compiles with these `pampa` changes). Ready to request push approval.

## Open questions — RESOLVED

- **Q1 — ellipsis:** ✅ YES — canonicalize `…` ↔ `...` too. (D3/D5)
- **Q2 — `-smart` extension:** ✅ Leave unconditional / out of scope. The flag
  is parsed but unused; this work does not wire it up.
- **Q3 — D4 escape style:** ✅ Use **`\—`** (backslash + unicode dash), which
  requires extending the reader's backslash-escape set (D5). Chosen over
  `\-\-\-` (which would render literal `---`, not a dash) and over bare unicode
  `—` (works with no parser change, but the user prefers the explicit escaped
  form). (D4)

## Verified findings backing the decisions (binary-observed)

- `un---spaced` / `en--dash` mid-word: NOT converted today (whole-token gating).
- `a\-\-b` → `a--b` literal today; must stay literal (per-node guard).
- `` `a---b` ``, `` `it's` `` inside code: untouched today (different node type).
- Lone `---` para → `HorizontalRule`; lone `--` → en-dash para (safe); qmd has
  no setext headings.
- `\—` today → `Str "\—"` (backslash kept) — the gap D5 closes.
- Pandoc `dash` parser: `external-sources/pandoc/.../Parsing/Smart.hs:167`.
