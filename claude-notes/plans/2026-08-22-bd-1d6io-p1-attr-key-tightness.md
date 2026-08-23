# bd-1d6io — P1 tight ranges for attribute keys, and a CI guard for the annotated-qmd fixtures

**Strand:** bd-1d6io (`in_progress`). Absorbs **bd-49cbyqbt** (closed as a
duplicate of failure #2; see `braid show bd-1d6io` comment `c-qn11q3g6`).
**Branch:** `braid/bd-1d6io-p1-attr-key-tightness`, worktree
`.worktrees/workspace-2`, off `origin/main` at `e3b3d7d4a`.
**Predecessor:** `claude-notes/plans/2026-06-01-bd-1d6io-investigation.md`
(the bisect + root-cause record). **Read that first**, then read the
"Corrections" section below — two of its conclusions are superseded.
**Date:** 2026-08-22.

## Corrections to the June investigation

The June investigation is sound on *history* (the bisect, the dating, the
CI-blind-spot analysis) and superseded on *remedy*. Three corrections,
each verified live at `e3b3d7d4a`:

1. **Failure #1 needs no code fix.** Plan 7g Phase 3 (2026-06-03, three days
   after the investigation) converted `code_span_helpers.rs` to tight ranges.
   Live pampa on `links.qmd` gives `Space [125,126]` + `Code [126,133]`; the
   committed `links.json` still holds `[125,133]` for *both*. The test is red
   on a **stale fixture**, not on live code.

2. **The fix layer is the writer, not the scanner.** June concluded "both Rust
   writers are correct — the fix is scanner-side," with a warning about
   `s->indentation` blast radius. Plan 7g Phase 3 then fixed failure #1 at the
   writer layer and wrote the convention down as
   `claude-notes/designs/provenance-contract.md` **§P1 — Tight ranges**:

   > A node's `source_info` covers exactly the bytes that constitute it: its
   > own delimiters included (a code span includes its backticks), surrounding
   > whitespace excluded. **Implemented** (2026-06-03, Plan 7g Phase 3):
   > `code_span_helpers.rs`, `citation.rs`, `quote_helpers.rs`, `postprocess.rs`
   > math-with-attr Span. Use `tight_source_info_for_node(node, ctx)` and
   > `leading_whitespace_source_info(&whole, &tight)`.

   The attribute path is not in that list. Failure #2 is a **missing P1
   conversion**. **Do not touch the scanner.**

3. **June's code pointer is stale.** `key_value_specifier.rs` /
   `process_key_value_specifier` does not exist. The live seam is the
   `"key_value_key"` arm at `crates/pampa/src/pandoc/treesitter.rs:1205`, which
   has called `.trim()` on the key *text* while keeping the untrimmed
   `node_location(node)` since `6b0c51d26`. So "the writer faithfully reports
   the node range" is false there: the writer breaks its own text↔range
   lockstep. That is why a writer-layer fix is *correct*, not a workaround.

## Root cause (unchanged from June)

The single external scanner's leading-whitespace preamble
(`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:2614`) advances
over spaces/tabs with `advance()` → `lexer->advance(lexer, false)`, which is
token-*inclusive*, to accumulate `s->indentation`. Zero of the scanner's 66
advances use the skip flag. Tree-sitter fixes an external token's *start* at
scanner entry, and `mark_end` only moves the end — so any external token
entered at a whitespace position absorbs that whitespace. `KEY_SPECIFIER` for
the 2nd+ same-line kv pair is such a token.

We are **not** fixing that (P1 handles the consequence at the writer). It is
recorded here because it explains why the defect is per-token and why the
correct cases are correct by accident.

## Measured scope

Fixture: `2026-08-22-bd-1d6io-p1-attr-key-tightness-investigation/scope.qmd`,
read back through `pampa -t json` and indexed against the source bytes.

| form | recorded | status |
| --- | --- | --- |
| `#the-id`, `.one`, `.two` | `[5,12]`, `[13,17]`, `[18,22]` | correct |
| 1st key `a` | `[23,24]` | correct |
| 2nd/3rd keys `bb`, `ccc` | `[26,29]` `' bb'`, `[33,37]` `' ccc'` | **wrong** |
| values incl. quotes | `[30,33]` `'"2"'` | correct (P1: delimiters included) |
| trailing side of `ccc = '3'` | `[33,37]` stops before the space | correct |
| multi-line list keys `m1`, `m2` | `[115,117]`, `[124,126]` | correct — a soft
  line break is tokenized separately, so the scanner is never entered at
  whitespace |

**One seam, leading side only.** Shortcode kv pairs route through the same
`"key_value_key"` arm; an unexpanded shortcode contributes `[0,0]` pool
entries, so there is no observable symptom there, but the fix covers it.

Why no auditor caught it — **this paragraph was wrong when first written; see
Phase 6.** I recorded "P1 has no auditor, and the key range does not overlap a
sibling (`[251,262]` starts exactly where the previous value ends), so P4 was
satisfied while P1 was violated." The second clause is true and irrelevant. The
first is false: `check_tightness` implements P1/P3 and `audit_attr_source`
already applied it to every attr-key range. **The auditor would have named this
defect exactly.** Nothing ran it over real documents — that, not a missing
check, is why the defect survived. Corrected in Phase 6.

## Baseline at `e3b3d7d4a` (unmodified `origin/main`)

| leg | result |
| --- | --- |
| `cargo nextest run --workspace` | **12999 passed, 0 failed**, 198 skipped |
| `ts-packages/preview-renderer` integration (after `npm run build:wasm`) | 578 passed, **1 failed** — `Equation > appends \tag{N}`, pre-existing, tracked as **bd-s36g9dav** (katex 0.18.1 pin), unrelated |
| `ts-packages/annotated-qmd` (`npm test`) | 154 passed, **2 failed** — this strand's pair |

`cargo xtask verify --skip-hub-build` cannot reach the preview-renderer leg:
it skips the WASM build that `preview-runtime/src/wasmRenderer.ts` imports, so
seven suites fail to resolve. Run `cd hub-client && npm run build:wasm` first
when that leg matters.

`npm install` in a fresh worktree rewrites `package-lock.json`, deleting ~468
lines of other-platform optional `@esbuild`/rollup binaries. **Revert it** —
committing it breaks Linux/Windows CI.

## Phases

### Phase 1 — Tests first (TDD; must fail before Phase 2)

`crates/pampa/tests/integration/test_attr_source_parsing.rs` already has the
right harness: `parse_qmd`, `extract_offsets`, and
`assert_source_matches(input, source_info, expected)` — which *is* the P1
assertion. Its header comment claims it verifies "source locations for
key-value pairs," but **no test in it touches a kv key**. That is the coverage
gap that let this live for a year.

- [x] Add kv-key P1 tests to `test_attr_source_parsing.rs` (no new file, so no
      `main.rs` change): 1st key tight; 2nd and 3rd keys tight (the failing
      case); `k = v` spacing around `=`; multi-line attr list keys tight
      (guards the currently-correct path against regression); values keep
      their quotes; ids/classes unaffected.
      → 5 tests added, all in the new "Key-Value Attribute P1 Tightness"
      section, sharing a `kv_sources(attr_source, i)` accessor.
- [x] Run them and record the failures. Expect the 2nd/3rd-key cases to fail
      with a leading space and everything else to pass.
      → **42 tests run: 38 passed, 4 failed**, exactly as predicted:

      ```
      test_div_kv_keys_have_tight_source                'custom-key' vs ' custom-key' (27-38)
      test_span_kv_keys_have_tight_source               'k2'         vs ' k2'         (22-25)
      test_kv_key_with_spaces_around_equals_...         'bb'         vs ' bb'         (10-13)
      test_third_kv_key_has_tight_source                'bb'         vs ' bb'         ( 8-11)
      PASS test_multiline_attr_list_kv_keys_have_tight_source
      ```

### Phase 2 — The P1 conversion

- [x] Narrow the recorded range in the `"key_value_key"` arm
      (`treesitter.rs:1205`) to the trimmed key bytes, in lockstep with the
      `.trim()` already applied to the text. Per §P3 trim **both** ends.
      → Added `tight_node_location(node, input_bytes) -> Range` to
      `location.rs` (the `Range` counterpart of `tight_source_info_for_node`),
      plus a private `advance_location` that carries row/column across the
      trimmed prefix (tree-sitter's `Point::column` is a byte offset, and a
      trimmed `\n` must bump the row). It trims with `str::trim` semantics so
      it agrees exactly with the caller's `.trim()`. Falls back to the
      untrimmed range on invalid UTF-8 or an all-whitespace slice.
- [x] Phase 1 tests green. → **42/42**.
- [x] `cargo clippy -p pampa --all-targets -- -D warnings` (clean) +
      `cargo nextest run -p pampa` → **4516 passed, 0 failed**. No pampa
      insta snapshot needed updating, which independently confirms June's
      finding that no CI-resident snapshot covered a multi-kv attribute.
- [x] Add the attribute path to §P1's "Implemented" list in
      `provenance-contract.md`, with the new helper. (This item originally
      added a note claiming **P1 is not auditor-enforced**. That was wrong —
      see Phase 6. The contract now says the opposite; the line is corrected
      rather than deleted so the mistaken inference stays visible.)

### Phase 3 — Fixture regeneration

**Scope grew here, deliberately: all 20 fixtures needed regeneration, not 2.**
Every committed fixture differed from live output, including ones with no
attributes at all — the fixtures had drifted on three independent axes since
`896b017cc`. Regenerating only two would have left 18 stale and made Phase 4's
guard unwireable.

- [x] Regenerate **all 20** fixtures from the repo root (the fixtures embed the
      input path in `astContext.files[].name`, so the relative path matters):
      `for f in ts-packages/annotated-qmd/examples/*.qmd; do target/debug/pampa -t json -i "$f" > "${f%.qmd}.json"; done`
- [x] `cd ts-packages/annotated-qmd && npm test` → **156 passed, 0 failed**
      (was 154/2). bd-1d6io's original symptom is fully resolved.
- [x] Report the full fixture diff, per fixture and per category.

### Fixture diff, categorized

Method: resolve every AST node's *and* every `a.kvs` attr-sidecar's pool index
to an absolute byte range in both the old (`git show HEAD:`) and new fixture,
then compare per node. Script preserved at
`2026-08-22-bd-1d6io-p1-attr-key-tightness-investigation/categorize.py`.

| fixture | semantic Δ | category |
| --- | --: | --- |
| academic-paper | 14 | tightness, meta-truncation |
| tutorial | 15 | tightness, meta-truncation |
| inline-types | 7 | tightness |
| blog-post | 6 | tightness, meta-truncation |
| links | 2 | tightness |
| yaml-tags | 2 | provenance *gain* (mislabelled `meta-truncation` by the script) |
| empty-content | 2 | meta-truncation |
| **div-attrs** | **1** | **attr-key ← this change** |
| boundary-values, minimal-doc, minimal-figure, missing-fields, zero-width | 1 each | meta-truncation |
| definition-list, figure, horizontal-rule, ordered-list, raw-block, simple, table | 0 | pool re-packing only |

**Only `div-attrs` changed for a reason attributable to this branch, and it
changed in exactly one place** — the `custom-key` range, `[251,262]`
`' custom-key'` → `[252,262]` `'custom-key'`. Clean attribution.

The other three categories are pre-existing fixture staleness being flushed:

1. **tightness** (5 fixtures) — bd-1d6io **failure #1**, fixed by Plan 7g
   Phase 3 and never regenerated. The wide `Code`/`RawInline`/`Quoted`/`Cite`
   range splits into a tight node plus a 1-byte `Space`:
   `links`: `Space [125,133] ' \`x = 5\`'` + `Code [125,133] ' \`x = 5\`'`
   → `Space [125,126] ' '` + `Code [126,133] '\`x = 5\`'`.
   Also 6 sites in `tutorial`, 2 `RawInline` + 2 `Quoted` + 1 citation prefix
   in `inline-types`, 2 `Cite` in `academic-paper`, 1 `Code` in `blog-post`.

2. **meta-truncation** (10 fixtures) — committed meta scalar ranges were 2
   bytes short, dropping the closing delimiter; live includes it.
   `'"Jane Do'` → `'"Jane Doe"'`, `'[tutorial, quarto, publishing'` →
   `'[tutorial, quarto, publishing]'`. Strictly better.

   Two label caveats, from review. `yaml-tags`'s two entries are a provenance
   *gain* — `Span '' → 'x + 1'` and `'' → '2024-01-15'`, unresolvable becoming
   resolvable — which `categorize.py`'s `len(nt)>len(ot) and nt.startswith(ot)`
   branch files under this category. The direction is right; only the label is
   wrong. And the table folds the script's `OTHER:Space`/`OTHER:Str` buckets
   into prose, so it does not show that `academic-paper` carries the
   `OTHER:Str` item — that one is bd-mxa44voa, flagged below.

3. **pool re-packing** (all 20) — the committed fixtures carried a leading
   pool entry `{"d":{"by":{"kind":"user-edit"}},"r":[0,0],"t":4}` that live no
   longer emits, shifting every `s` index by one. Seven fixtures differ *only*
   by this, which is why the raw byte diff is 20/20 while the semantic diff is
   13/20.

**One change is an improvement that is still not correct, and it is not ours.**
`academic-paper`'s `meta.author` inline sub-nodes went from unresolvable
(`Str` → `'---\ntitl'`, i.e. document start) to resolvable but shifted
(`Str "Dr."` → `[71,80]` `'"Dr. Alic'`, should be `[72,75]`). This is
**bd-mxa44voa** — nested-parse drift where quarto-yaml's span is
quote-inclusive while the decoded scalar is what gets re-parsed; its
description predicts exactly this "off by one before any escape is involved"
shift. Pre-existing, separately tracked, untouched here. **Do not read the
regenerated `academic-paper.json` as asserting those ranges are right** — the
guard freezes current behavior, and bd-mxa44voa will move it again.

### Phase 4 — The CI guard (June's most durable item)

Both failures sat red for months because the annotated-qmd example JSONs are
static, hand-regenerated, and inert to `cargo nextest`. The guard converts
them into a CI-resident artifact.

- [x] Added `crates/pampa/tests/integration/annotated_qmd_fixture_guard.rs`
      (registered in `main.rs`), comparing live writer output to every
      committed fixture. It compares in memory and never writes the fixtures.
      Two tests:
      - `annotated_qmd_example_fixtures_match_live_writer` — invokes the real
        binary via `env!("CARGO_BIN_EXE_pampa")` with `current_dir(repo_root)`
        and the repo-relative path, because the fixtures embed that path in
        `astContext.files[].name`. Collects **all** mismatches before failing,
        names them, and prints the regeneration loop plus a warning to review
        the diff rather than rubber-stamp it. Also fails on a `.qmd` with no
        committed `.json`.
      - `annotated_qmd_examples_use_lf_line_endings` — see the cross-platform
        note below.
- [x] Home decided: **a `pampa` integration test**, not an xtask lane. It is
      CI-resident under plain `cargo nextest` with no JS toolchain, and
      driving the binary (rather than the library) guarantees it compares
      against exactly what the documented regeneration command produces.
- [x] Proved the guard bites. Reverted Phase 2's one expression, re-ran:
      the guard failed naming `div-attrs` (and the 4 Phase 1 tests failed);
      restored, all 44 green. The guard is bound to the fix, not decorative.
- [x] **Cross-platform**: these fixtures record byte offsets *and*
      `astContext.files[].line_breaks`, so a CRLF checkout shifts everything
      and the guard would fail as ~20 opaque mismatches on Windows. Pinned the
      sources with `ts-packages/annotated-qmd/examples/*.qmd text eol=lf` in
      `.gitattributes`, and added the LF assertion above so the pin failing is
      reported as itself rather than as fixture drift.

### Phase 5 — Wrap up

- [x] `cargo nextest run --workspace`. **Superseded twice — the figure below is
      the live one at the branch tip, not a number copied forward.** The
      first-commit reading (13006, +7) and the second (13014, +15) are kept in
      the table so the arithmetic is checkable.

      | at | runs | skipped | delta vs 12999 baseline |
      | --- | --: | --: | --: |
      | commit 1 `541e3838e` | 13006 | 198 | +7 |
      | commit 2 `aac9174a6` | 13014 | 198 | +15 |
      | **branch tip (review fixes)** | **13028** | **198** | **+29** |

      Full accounting of +29, **+0 failures and +0 skips** throughout:

      | source | test fns | runs |
      | --- | --: | --: |
      | kv-key P1 tests, `test_attr_source_parsing.rs` | 5 | 5 |
      | `annotated_qmd_fixture_guard.rs` | 2 | 2 |
      | `tiling_corpus_tests.rs` | 1 | 1 |
      | abbreviation test, `tiling_phase3_tests.rs` | 1 | 1 |
      | `tiling_auditor_tests` in `incremental.rs` | 3 | **6** |
      | `advance_location` + `trim_whitespace_range` in `location.rs` | 7 | **14** |
      | | 19 | **29** |

      **The ×2 on in-module tests is real and worth knowing:** pampa's
      `#[cfg(test)]` tests are enumerated under both the `pampa` lib target and
      `pampa::bin/pampa`, so every in-module test costs two runs. Verified with
      `cargo nextest list -p pampa`, not assumed. Pre-existing behavior, not
      introduced here — but it means an in-module test's contribution to the
      workspace count is double its function count.
- [x] `cargo xtask lint` → **all checks passed (1037 files checked)**.
- [x] `cd ts-packages/annotated-qmd && npm test` → **156/156**.
- [x] `cargo xtask verify` (full, WASM rebuilt, so the preview-renderer leg
      actually runs) → exits 1 on **exactly** the known bd-s36g9dav failure
      and nothing else:

      | leg | result |
      | --- | --- |
      | `cargo nextest run --workspace` | 13006 passed, 198 skipped |
      | ts-packages builds + MCP smoke | pass |
      | hub-client build + tests | 131 passed (22 files) |
      | trace-viewer | 10 passed |
      | preview-runtime | 549 passed, 36 skipped |
      | preview-renderer integration | 578 passed, **1 failed** — `Equation > appends \tag{N}` |

      Byte-identical to the baseline for that leg (578/1 before and after), so
      the delta introduced by this branch is **zero new failures**.
- [x] Reconcile this checklist against reality, commit, then ask before pushing.

### Phase 6 — The corpus auditor (added after review; commit 2)

Gordon asked whether Plan 7g's exhaustive tiling/coverage approach could be
applied to the coverage gap in Phase 4. It can, and the answer reframes Phase 4:

**The rigorous check already existed and already covered this exact defect.**
`check_tightness` in `audit_source_range_tiling` implements P1/P3, and
`audit_attr_source` applies it to every attr-key range. Pointed at the bug it
says, precisely:

```
TightnessViolation: `attr-key` [251..262] has leading space/tab byte (' ')
```

The defect lived a year because **nothing drove the auditor over real
documents** — its only caller was 11 hand-written snippets in
`tiling_phase3_tests.rs`, none with a multi-kv attribute. The assertion was one
function call away.

- [x] Added `tests/integration/tiling_corpus_tests.rs`: runs the auditor over
      ~170 documents (annotated-qmd examples, pandoc-match-corpus, smoke,
      writers, claude-examples), asserting zero findings. Guards against a
      vacuous pass (corpus size and parsed-count floors). `KNOWN` list requires
      a strand per entry; it has exactly **one**: an `AttrAlignmentSkipped`
      census finding on an autolink's synthesized class (bd-3aolj / bd-1e6a5).
- [x] Proved it binds: reverting the Phase 2 fix makes it fail naming **4**
      documents — `div-attrs.qmd` plus three in `tests/writers/ansi/` that the
      Phase 4 fixture guard could not see. Strictly better coverage.
- [x] Refined `check_tightness` with a `retained: Option<&str>` parameter. The
      corpus probe surfaced one false positive: `e.g. \`code\`` gives
      `Str [0..5]` a trailing source space, because the abbreviation handler
      substitutes NBSP and **keeps it in the node's text** — those 5 bytes are
      exactly what produced the node. Attr keys/values pass `None` and stay
      strict, since weakening them is precisely what bd-1d6io must not do.
      **This edits the auditor Plan 7g shipped — flagged for review.**
      **Correction (review):** I described this as *generalizing* the existing
      `Space`/`SoftBreak`/`LineBreak` exclusion. It does not — that wholesale
      type skip is still at `incremental.rs:1367`, so there are now **two**
      mechanisms, applying in different places. The seam leaves a pre-existing
      gap (a `Space` in a single-`Inline` CustomNode slot is still flagged: that
      arm has no type skip and `inline_retained_text` returns `None` for
      `Space`). Unifying them is **bd-89jcn0uv**, and it is not mechanical —
      the wholesale skip is deliberately blind to a `Space` claiming
      *non*-whitespace bytes.
- [x] Three in-module unit tests pin the exclusion to the node's *text*, not its
      type, so it cannot degrade into "`Str` is exempt" and blind the auditor to
      that whole family. Plus one end-to-end test in `tiling_phase3_tests.rs`.
- [x] Demoted the Phase 4 fixture guard in its own docs. It compares against a
      snapshot, so regeneration launders a violation past it; the corpus auditor
      asserts a property and cannot be laundered. The fixture guard is kept for
      what the auditor cannot see — wire-format changes (pool packing, key
      renames) that preserve the invariants but still want human eyes.

**A correction to my own earlier proposal, recorded because it was nearly
built.** I had offered an exhaustive `source[range] == text` assertion. That is
*wrong*: text legitimately diverges from source bytes by design — abbreviation
NBSP substitution, unescaped attribute values, decoded YAML scalars. It would
have fired on all three and grown an allowlist to hide it. P1's structural
formulation (boundary bytes, containment, disjointness) is robust precisely
because it never requires text equality. **A second wrong inference of mine, caught in review.** I wrote that the
auditor is "correctly silent on bd-mxa44voa's shifted `academic-paper` ranges
because a shifted range is not a tightness violation." That reasoning is false.
`audit_source_range_tiling` (`incremental.rs:1080`) calls only
`audit_block_siblings(&ast.blocks, …)` — **it never visits `ast.meta` at all.**
It is silent because it does not look, not because it looked and approved. If it
did look, `meta.author.c[2]` is `Str [81,86] ' Smit'` with text `"Smith"` — a
leading space the text does not retain, i.e. a textbook `TightnessViolation` it
would name exactly.

This matters for the branch's central claim: the corpus auditor asserts a
property that cannot be laundered by regenerating a fixture — **for block
content only.** Metadata provenance is unguarded, which is precisely where the
regenerated `academic-paper.json` freezes a known-bad range. Recorded in the
corpus test's doc comment, and tracked as **bd-riu2dvcf** (walking `ast.meta`
would catch bd-mxa44voa for free).

### Phase 7 — Review fixes (commit 4)

Findings from the code review, all verified before acting. The reviewer
confirmed test binding empirically by reverting each change in isolation, and
independently re-derived the fixture categorization.

- [x] **Windows breakage in the corpus test (blocking).** `is_known` matched
      `KNOWN`'s forward-slashed suffixes against a `Path::display()` string,
      which is backslashed on Windows — so the one legitimate finding would be
      reported as a violation and the test was red for Windows developers only
      (CI is ubuntu + macOS, so CI would never have caught it). Normalized to
      forward slashes. The *fixture* guard is Windows-safe by construction:
      `astContext.files[].name` stores the argument verbatim and Windows accepts
      forward slashes, so the round-trip matches.
- [x] **Vacuity guards were looser than they looked.** Replaced the `> 100`
      floors with (a) a per-root non-emptiness assertion — losing the
      annotated-qmd root entirely still left ~155 files, comfortably over any
      total floor, and `collect_qmd` skips an unreadable directory silently —
      and (b) an exact `EXPECTED_UNPARSEABLE` set instead of a count, so a new
      unparseable or panicking document names itself.
      **This immediately earned its keep:** the reviewer's list of 7 unparseable
      files was wrong — `smoke/011.qmd` parses fine. The assertion caught it on
      first run and printed the real set of 6.
- [x] **Corrected the bd-mxa44voa claim** and recorded the `ast.meta` scope gap
      in the corpus test's doc comment. Filed **bd-riu2dvcf**.
- [x] **Corrected the "generalizes" claim.** Filed **bd-89jcn0uv** for unifying
      the two exclusion mechanisms, with the caution that it is not mechanical.
- [x] **All-whitespace fallback (#5).** Extracted `trim_whitespace_range` from
      `tight_node_location` so it is directly testable, and collapsed the
      all-whitespace case to a zero-length range at the start. Previously it
      returned the *untrimmed* range, contradicting the helper's own documented
      invariant (the caller records `""`) and inviting the auditor to report a
      bogus boundary-space finding on a node with no content. Now consistent
      with the sibling `utils::trim_source_location::trim_whitespace`. Answering
      the "should it fail louder?" question: no — "louder" here means a false
      auditor finding. Unreachable at the current call site; this is about not
      seeding drift between two helpers that must agree.
- [x] **`advance_location` coverage (#6).** Its newline and multi-byte branches
      were never exercised — every in-tree path trims a single ASCII space. Four
      unit tests now pin ASCII, `\n`, `\r\n`, and byte-vs-char column counting.
      The function is `pub` and documented as general, so the next caller may
      well hit a newline prefix.
- [x] **`.gitattributes` widened** from `examples/*.qmd` to `examples/*`. The
      JSON fixtures contain no newlines today, so `core.autocrlf` cannot corrupt
      them — but the guard byte-compares them, so a future multi-line writer
      would break on Windows in exactly the way the pin exists to prevent.
- [x] Fixed the stale Phase 2 checklist note (#9) and the `yaml-tags` category
      label (#10) — a provenance *gain*, not `meta-truncation`.

**Accepted as follow-ups, not fixed here:** bd-89jcn0uv (unify the exclusions),
bd-riu2dvcf (walk `ast.meta`), and the reviewer's #8 — `check_tightness` tests
*presence* of boundary whitespace, not *amount*, so a `Str` whose text ends in
one NBSP could absorb an unbounded run of trailing source spaces. Sibling
disjointness defends that in depth, so it is not a live hole; it is folded into
bd-89jcn0uv.

**Reviewer's verdict:** merge with fixes. The blocking item (Windows) and both
wrong claims are fixed above.

### Phase 8 — Tighten the tightness exclusion to the actual producer behavior

Gordon asked why the exclusion wasn't keyed to the *specific* thing pampa does,
since the NBSP is deliberate typography. It should have been — I wrote a general
rule without reading the producer first.

`postprocess.rs` (the `ends_with_abbreviation` branch) absorbs the `Space` after
an abbreviation into the preceding `Str` as U+00A0, Pandoc-parity, so `e.g.`
cannot be separated from its referent. `048.qmd` hits the specific sub-case
where the abbreviation is followed by a `Code` rather than a `Str`, so the Space
is swallowed with no word to join: `Str("e.g.\u{a0}")` over the five source
bytes `"e.g. "`. The `\<space>` escape in `text_helpers.rs` does the same
substitution.

- [x] Re-keyed the exclusion from "the text has boundary whitespace" to
      **exactly the NBSP substitution**: the text's boundary characters must be
      U+00A0, **in matching quantity** with the source space/tab bytes.
      `retained` → `own_text`, `inline_retained_text` → `inline_own_text`.
- [x] **This also closes reviewer finding #8** (presence vs amount) rather than
      deferring it: a range absorbing two trailing spaces against one NBSP is
      now reported, where the loose rule forgave it. So the tighter exclusion
      *removes* a filed follow-up instead of adding one — dropped from
      bd-89jcn0uv's scope.
- [x] Three new tests, two of which were **red under the loose rule**: a `Str`
      retaining a *plain* space is still flagged (no producer does that); a
      range absorbing 2 spaces against 1 NBSP is still flagged; a source *tab*
      is accepted, since a Pandoc `Space` can come from either.
- [x] Checked for other producers first: the only other whitespace-into-text
      path is `code_span_helpers.rs`, which writes `Code`'s text, and `Code`
      passes `None`. Both NBSP producers are covered; nothing retains a plain
      boundary space.
- [x] Binding re-verified after tightening: reverting the attr-key fix still
      fails the corpus test naming the same 4 documents.

Why this is better than the general rule: it is a claim a reader can check
against `postprocess.rs` in one hop, and a *different* producer that starts
retaining some other character now fails loudly and names its document instead
of being silently forgiven.

## Result

bd-1d6io's reported symptom is gone: `ts-packages/annotated-qmd` goes from
154/156 to **156/156**, and the two failures resolved by different means —
failure #1 by regenerating a fixture that had been stale since Plan 7g Phase 3,
failure #2 by the P1 conversion this branch adds. The guard means neither can
silently return.

Verification of the fix through the real binary, not just the library:

```
$ target/debug/pampa -t json -i ts-packages/annotated-qmd/examples/div-attrs.qmd
  ... pool entry for the `custom-key` attribute key:
  before:  [251,262]  ' custom-key'
  after:   [252,262]  'custom-key'
```

Source line: `::: {.panel data-value="42" custom-key="test"}` with `custom-key`
at byte 252. Output inspected by resolving the pool index back to source bytes,
not inferred from exit status.

## Explicit non-goals

- **The scanner.** Not touched. If a future consumer needs a correct *CST*
  node range (LSP, tree-sitter queries), that is a separate strand with the
  regression sweep June's plan describes.
- **P2 whitespace ownership for attribute separators.** Attribute lists have
  no `Space` inlines to re-attribute the separator bytes to, so they stay
  unowned. §P4's scope boundary explicitly tolerates this ("blank lines,
  `> ` gutters, and list indentation are legitimately unowned"), and the
  `key_value_specifier` parent still spans them.
- **Attribute *value* ranges including their quotes.** P1 says delimiters are
  included, and the annotated-qmd test asserts it deliberately ("values
  include quotes in source"). See bd-bhxeoqoj for the stale comment about it
  in `theorem.rs`.
- **bd-s36g9dav** (katex `\tag{N}`), **bd-3aolj** / **bd-1e6a5**
  (`AttrSourceInfo` positional-alignment), **bd-mxa44voa** (nested-parse
  rerooting). All pre-existing and separately tracked.
