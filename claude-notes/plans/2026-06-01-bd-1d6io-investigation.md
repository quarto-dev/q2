# bd-1d6io — annotated-qmd source-tracking off-by-one: investigation

**Status:** investigation complete — triage verdict below. No fix committed yet.
**Worktree:** `.worktrees/bd-1d6io-annotated-qmd-source-tracking` (branch `beads/bd-1d6io-annotated-qmd-source-tracking`, off `main`).
**Date:** 2026-06-01.

## TL;DR

Two TS tests in `ts-packages/annotated-qmd` fail after regenerating the example
JSON fixtures with current pampa. Both are "the recorded source range for a
token includes one byte of preceding whitespace," **but they are two distinct
bugs in two distinct code paths with two distinct histories**:

| | Failure #1 (inline code) | Failure #2 (div attr key) |
|---|---|---|
| Test | `substring-invariant.test.ts` "links.qmd: inline code" | `block-types.test.ts` "div-attrs … conversion" |
| Symptom | Code range starts on the sentence space before `` ` `` | 2nd+ attr-key range starts on the inter-pair space |
| Code path | code-span external scanner (`CODE_SPAN_START` / opening `code_span_delimiter`) | attribute external scanner (`KEY_SPECIFIER` / `key_value_key`) |
| Origin | **regression** in the 2025-10-30/31 inline-parser rewrite | **original defect**, present since ≥ 2025-08-06 |
| Deliberate? | No — accidental whitespace absorption | No — accidental whitespace absorption |
| Fix side | tree-sitter scanner/grammar | tree-sitter scanner/grammar |
| Writer side | `code_span_helpers.rs` is correct | `key_value_specifier.rs` is correct |

The issue's suspect commit **38e889ad (May 2026) is refuted** for failure #1:
its parent (`5d35218d`) is already bad. The hypothesis anchor
`code_span_helpers.rs:171` is *not* where the bug lives — that code faithfully
reports the tree-sitter node range; the node range is wrong because the
external scanner token is wrong.

## Reproduction (end-to-end, real TS harness)

```bash
# build pampa, regenerate the two fixtures, run the node test runner
cargo build --bin pampa
target/debug/pampa -t json -i ts-packages/annotated-qmd/examples/links.qmd \
    > ts-packages/annotated-qmd/examples/links.json
target/debug/pampa -t json -i ts-packages/annotated-qmd/examples/div-attrs.qmd \
    > ts-packages/annotated-qmd/examples/div-attrs.json
cd ts-packages/annotated-qmd && npm test     # node --import tsx --test test/*.test.ts
```

Observed at HEAD (`1ccda5a7`):
- `Inline code with backticks: substring(125, 133) should extract "`x = 5`", got " `x = 5`"`
- `Key source text " custom-key" should be a valid attribute key`

(NB: the suite uses node's built-in test runner via `tsx`, **not** vitest, and
needs `ts-packages/pandoc-types` built first: `cd ts-packages/pandoc-types &&
npm run build`.)

## Root-cause evidence

### Failure #1 — code-span scanner absorbs preceding whitespace

Concrete syntax tree for `a \`x = 5\` b` at HEAD (`pampa -v`):

```
pandoc_code_span      (0,1)-(0,9)
  code_span_delimiter (0,1)-(0,3)   <- opening "`" is at col 2; token starts at col 1 (space)
  content             (0,3)-(0,8)
  code_span_delimiter (0,8)-(0,9)
```

Control cases prove it is whitespace-absorption, not a fixed offset:

| input | opening delimiter | Code range | verdict |
|---|---|---|---|
| `` `x = 5` b `` (col 0) | (0,0)-(0,1) | `[0,7]` | correct |
| `a \`x = 5\` b` (1 space) | (0,1)-(0,3) | `[1,9]` | absorbs 1 space |
| `a  \`x = 5\` b` (2 spaces) | (0,1)-(0,4) | `[1,10]` | absorbs **both** spaces |

No Pandoc semantic wants this (the Pandoc one-space-strip rule concerns spaces
*inside* the backticks). `advance()` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:523` calls
`lexer->advance(lexer, false)` (token-inclusive); the leading inline whitespace
is advanced over before `CODE_SPAN_START` is emitted
(`parse_fenced_code_block`, scanner.c ≈ 743-746), so the token begins at the
whitespace. The Rust consumer
`code_span_helpers.rs:process_pandoc_code_span` then takes
`node_source_info_with_context(node, context)` over the whole (already-too-wide)
`pandoc_code_span` node — correct code, wrong input range.

### Failure #2 — attribute scanner absorbs the inter-pair space

CST for `::: {.panel data-value="42" custom-key="test"}` at HEAD:

```
key_value_specifier (0,12)-(0,27)
  key_value_key     (0,12)-(0,22)   data-value — correct (1st kv)
  key_value_value   (0,23)-(0,27)
key_value_specifier (0,27)-(0,45)
  key_value_key     (0,27)-(0,38)   custom-key — starts at col 27 (the space); "c" is at col 28
  key_value_value   (0,39)-(0,45)
```

Only the **2nd+** key is affected, which is why `data-value` passes and
`custom-key` fails. `key_value_key` is `alias($._key_specifier_token,
$.key_value_key)` (an external `KEY_SPECIFIER` token, scanner.c ≈ 1958/1969).
The grammar places inter-pair whitespace *outside* the key
(`_commonmark_specifier_start_with_kv`: `repeat(seq(optional($._attr_ws),
key_value_specifier))`), yet the node range includes it — the external token
boundary wins over the `optional($._attr_ws)`. Same whitespace-absorption
family as #1, different scanner path. The Rust consumer
`key_value_specifier.rs:process_key_value_specifier` faithfully records the
`key_value_key` node range — correct code, wrong input range.

## How each was pinned (bisect)

Good baseline for #1: `2b2337be` (2025-10-24, the commit that generated the
committed `links.json`) → correct `r:[126,133]` / minimal `r:[2,9]`.

The mechanical bisect over `2b2337be..HEAD` (1469 commits) is **muddied by two
structural facts** and cannot return a single commit:
1. The `quarto-markdown-pandoc` → `pampa` crate rename (`d86b8ecf`, 2025-12-06)
   — handled by a binary-agnostic oracle that tries both bin names.
2. A late-Oct/early-Nov 2025 inline-parser rewrite landed as a long run of
   **non-compiling WIP commits** ("welp", "partial work", "merge partial work",
   "fix bad arrays") — `git bisect` skips them, leaving an ambiguous block.

**Failure #1** narrowed to the 2025-10-30/31 window (33 commits, 27 of which
don't build). Direct probes:
- `2b2337be` (Oct 24): `r:[2,9]` **good**
- `5cc1a849` ("code spans", Oct 31, last buildable in block): `r:[1,9]` **bad**
→ Regression introduced by the inline-parser rewrite that re-implemented
code spans (commit "code spans" `5cc1a849` is the first confirmed-buildable
bad). The exact WIP commit is unrecoverable because the intermediates don't
compile.

**Failure #2** is older than every fixture. Probes with the minimal div
fixture: bad (`2nd key [27,38]`) at `2b2337be~300` (2025-08-06), `~250`, `~200`,
`~150`, `~100`, `~50`, `~20`, and at `2b2337be` itself — i.e. present since
multi-kv attribute parsing was first written. **Not a regression; an original
defect.**

Oracles + logs live in `/tmp/bd-1d6io/` (oracle_code2.py, oracle_attr2.py,
oracle_code_min.py, bisect-*.log).

## Why CI stayed green for ~7 months (answers the "snapshots" question)

Three artifacts could in principle have caught this; each had a blind spot.

1. **pampa insta `.snap` JSON snapshots** (`crates/pampa/snapshots/json/*.snap`)
   — CI-resident (run on every `cargo nextest`) **and** they record byte
   offsets. They *would* have caught the drift at the introducing commit —
   but **none contains an inline `Code` node**, and none exercises a token
   preceded by prose whitespace, nor a multi-kv attribute. The regressed
   boundaries were never represented in this family. This is the
   "never snapshotted" gap.

2. **`pandoc-match-corpus`** — asserts equality against Pandoc's `markdown`
   reader AST, which carries **no source ranges**. Constitutionally blind to a
   one-byte left-widening (text + structure are unchanged). The May-2026
   code-span rework (38e889ad) added all its new fixtures here, so even that
   work added zero source-range coverage.

3. **annotated-qmd example JSON** (`ts-packages/annotated-qmd/examples/*.json`)
   — the *only* artifacts that record full source ranges **and** assert the
   substring invariant. But they are **static, hand-regenerated** fixtures,
   inert to `cargo nextest`. The TS suite validates the *frozen file's*
   internal consistency, not live pampa. So:
   - `links.json` froze a **correct** value (Oct 24, just before the Oct 30/31
     regression) and stayed correct → test kept passing while pampa drifted.
     Phase 5's forced regen rewrote it from live (buggy) pampa → test failed.
     This is the "we updated the snapshots and discovered they were wrong" case
     — the *old* snapshot was right; the *regenerated* one encodes the real
     bug.
   - `div-attrs.json` is sharper still: its committed `custom-key` value
     `[252,262]` (correct) **never matched live pampa**, not even at the commit
     that introduced the fixture (`d6230301`, where live already produced
     `[251,262]`). The fixture's "correct" value was never CI-reproducible —
     it was generated from an uncommitted local state or hand-corrected to the
     intended value. The defect underneath it predates the fixture by months.

**Net:** the boundaries *were* covered by a snapshot (artifact 3) — that's how
the bug finally surfaced — but only a static, manually-regenerated one, so it
caught the drift retroactively at regen time, not at the introducing commit.
The CI-resident family (artifact 1) that runs every build had no inline-code /
multi-kv case.

## Fix direction — and an explicit "this is NOT a mechanical fix" warning

Both defects are **scanner/grammar-side**; both Rust writers
(`code_span_helpers.rs`, `key_value_specifier.rs`) are already correct — they
faithfully report a tree-sitter node range that the external scanner widened.

**Shared root cause.** This grammar has a *single combined* external scanner
(`scan()` at `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:2279`;
there is no separate inline scanner). Every external token flows through one
leading-whitespace preamble:

```c
// scanner.c:2371 — runs for EVERY external token
for (;;) {
    if (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        s->indentation += advance(s, lexer);   // advance() => lexer->advance(lexer, false): token-INCLUSIVE
    } else { break; }
}
```

When the scanner is *entered at* a whitespace position (true for
`CODE_SPAN_START` and for the 2nd+ `KEY_SPECIFIER`), this preamble folds the
leading whitespace into the emitted token. The defect even corrupts the
*adjacent* `Space` node: for `` a `x=5` b `` both the Space and the Code node
get range `[1,9]` — range assembly in the neighborhood is entangled, not a
clean one-byte shift.

**Why there is no obvious fix (evaluated against this codebase, not in the
abstract):**

1. **No idiom to copy.** The tree-sitter mechanism that *properly* excludes
   leading whitespace from a token range is `lexer->advance(lexer, true)` (the
   skip flag). It is used **0 times** in this scanner; all **66** advances are
   `false`. The canonical tool for this exact job has never been adopted here.
2. **The correct cases are correct by accident.** `data-value` (1st key) and
   the leading `Str`/`Space` are right only because grammar regex tokens happen
   to consume their leading whitespace *before* the external scanner is
   entered. There is no deliberate guard to point at and replicate.
3. **`mark_end` cannot help.** It controls a token's *end*; the *start* is
   fixed at scanner entry. The whitespace can only be excluded by consuming it
   as a separate token/skip *before* entry — a grammar/precedence change.
4. **Large, load-bearing blast radius.** The same preamble feeds
   `s->indentation`, which drives block-structure decisions (indented-code
   detection, list/blockquote continuation). Flipping it to skip=true changes
   token-start semantics for *every* external token at once.

**Two candidate approaches, both requiring grammar fluency + a regression
sweep (NOT mechanical):**
- (A) Adopt the skip flag in the preamble for the inline-token path — broad
  blast radius; must prove indentation tracking and block tokens are
  unaffected.
- (B) Restructure the grammar so separator whitespace is always tokenized
  before the code-span / key external tokens (so the scanner is never entered
  at the whitespace) — requires reasoning about tree-sitter conflict/precedence
  resolution to prove no regression in other attribute/code contexts.

**Recommendation:** do **not** attempt either under a "brain-dead obvious" bar.
This is a separate task for someone comfortable with the external scanner +
grammar precedence, done TDD: (a) add byte-offset regression tests
(inline-`Code`-in-prose, multi-kv attr) to the CI-resident
`crates/pampa/snapshots/json/` family *first*; (b) run a deliberate regression
sweep, because a shared-preamble change can shift many other tokens' ranges
simultaneously and current coverage is too thin to catch that automatically.

Escalation note: failure #1 lands in the same inline-parser rewrite that was
"verified" by pandoc-equality corpus tests — justified work whose verification
method could not see source-range drift. The widening is accidental (so a code
fix is warranted), but the structural gap is the source-range-blind
verification — see CI guard below.

## Recommended CI guard

Add a `cargo xtask verify` lane (or a nextest test in `pampa`) that runs the
live writer over `ts-packages/annotated-qmd/examples/*.qmd` and diffs against
the committed `*.json`, failing on drift. This converts artifact 3 from a
manually-refreshed fixture into a CI-resident one, so the next forced regen
can't silently encode a writer regression — and so any future source-range
drift fails at the PR that introduces it. (Implementation should regenerate to
a temp path and `diff`, not overwrite the committed fixtures.)

## Work items

- [x] Reproduce both failures end-to-end through the real TS harness at HEAD.
- [x] Refute the 38e889ad hypothesis for failure #1 (parent already bad).
- [x] Pin failure #1 to the 2025-10-30/31 inline-parser rewrite (regression).
- [x] Pin failure #2 as an original defect (≥ 2025-08-06).
- [x] Identify root cause + fix side for each (scanner; writers are correct).
- [x] Explain the snapshot/CI-coverage gap.
- [ ] (fix, separate work) Add CI-resident byte-offset regression tests
      (inline-code-in-prose, multi-kv attr) — TDD, before the scanner fixes.
- [ ] (fix, separate work) Scanner fix #1: code-span token starts at backtick.
- [ ] (fix, separate work) Scanner fix #2: key token starts at key char.
- [ ] (fix, separate work) CI guard: diff live writer vs committed
      annotated-qmd example JSON in `cargo xtask verify`.
