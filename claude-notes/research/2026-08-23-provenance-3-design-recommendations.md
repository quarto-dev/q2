# Provenance Plan 3 — design recommendations

**Epic:** `bd-mxa44voa`. **Plan:** `claude-notes/plans/2026-08-20-provenance-3-audit-and-fix.md`.
**Findings:** `claude-notes/research/2026-08-21-provenance-audit-findings.md` (§§ 1–2 are the
accessor rule every recommendation below applies).
**Written:** 2026-08-23, on `provenance-3-design-questions` at `d6ee475be` (= the tip of
`feature/yaml-provenance`, which now *includes* Plan 2's final fix wave — so "what this branch
does not yet contain" in the dispatch prompt is moot; every item it listed has landed:
floors at 0.2.2/0.1.3, `website_post_render.rs:217`, `div_whitespace.rs` deleted, the
`SourceInfo: !Hash` clause).

**Recommendations only.** No production code changed. The single exception the brief allowed —
question 4's experiment — was run in a throwaway worktree and a local upstream checkout, both
restored (§ 4.6).

Three of the eight questions are mis-framed, and saying so is most of the value here:

- **Q1's proposed fix does not work.** `map_offset(0)/map_offset(length())` on `cb.source_info`
  is the *whole fenced block* hull, not the body. The right parent exists and the parser throws
  it away.
- **Q4's hypothesis is wrong, and so was the alarm that produced it.** Neither clamping nor
  "ariadne changed" explains Task F's observation. A *third* guard — `quarto-source-map`
  0.1.2's `offset_to_location` floor — sits upstream of the helper and makes the snap
  unreachable through `map_offset`. The crate documented this itself in commit `5e48166`, two
  days before Task F ran. The experiment confirms it: the snap half (3) **is** what saved the
  founding render at the version that crashed; clamping (1)+(2) did not; and today the
  upstream floor alone suffices with the helper reduced to a pass-through.
- **Q6's structural argument is stated backwards.** Printing runs *before* the exit-code gate,
  not after. The invariant that holds is immutability, not ordering.

---

## 1. `codeblock_shorthand.rs:486` — `find()` locates the body inside the fence line

### What is actually being asked

`body_source_for` (`crates/quarto-core/src/crossref/codeblock_shorthand.rs:470-490`) needs the
provenance of `cb.text`. `CodeBlock::source_info` spans the whole fenced block, so it
byte-searches the block's raw text for `cb.text` and takes the first hit. The question is
whether to fix the first-match hazard or file it.

But the plan's named remedy — "use the `map_offset(0)`/`map_offset(length())` pair instead of
`find()`" — is **mis-framed**. That pair is the hull of whatever span you call it on; called on
`cb.source_info` it returns the fenced block's extent, fence lines included. It cannot locate the
body because there is no body span to call it on. The real question is **where the body's
provenance should come from**.

### Evidence

- **Verified myself (probe, removed):** for `` ```{python}\npython\n``` ``, `body_source_for`
  returns `4..10` — inside `{python}` — where the truth is `12`. Output:
  `PROBE cb.text="python" block=Some((0, 0, 23)) body_span=4..10 slice="python" truth=12`.
  So the hazard is real, not contrived-only: any body that is a substring of its own fence line
  (`r` in `` ```{r} ``, `python`, a bare language name) mislocates.
- **The correct parent already exists and is discarded.** The grammar has a `code_fence_content`
  node; `process_code_fence_content`
  (`crates/pampa/src/pandoc/treesitter_utils/code_fence_content.rs:14-50`) computes its span
  (`node_source_info_with_context` + `source_info_to_qsm_range_or_fallback`) and returns
  `IntermediateBaseText(content, range)`. `process_fenced_code_block`
  (`fenced_code_block.rs:29-34`) destructures it as `IntermediateBaseText(text, _)` — the range
  is thrown away — and the `CodeBlock` is built with only the whole-node `location` (`:63`,
  `:80`). Note `content` there is *not* byte-identical to the node span when
  `block_continuation` markers (`> `, list indentation) are elided (`:27-45`), so a correct
  content provenance is a `Concat` of verbatim pieces with the markers as gaps — exactly what
  `cell_options` does for `#|` lines and exactly what `ProvenanceBuilder` models.
- **Adding a field to `CodeBlock` is wide.** `quarto-pandoc-types/src/block.rs:117-122` has
  `attr`, `text`, `source_info`, `attr_source`; `grep -c 'CodeBlock {'` across `crates/` finds
  74 construction sites in 45 files, plus the WASM/TS schema.
- **The two other body-locators are not affected.** `text_execute.rs:304-308` builds
  `body_source` as `substring(ctx.source_info, code_start, code_start + code.len())` over the
  engine's *writer* provenance (Phase 5's category — not this bug class), and
  `cell_options::partition_cell_options` (`mod.rs:163-200`) takes `body_source` as a parameter
  and never searches.

### Options

1. **Consumer-side, bounded search (cheap, now).** Keep `find`, but search only the region
   between the fence lines: start after the first `\n` of `block_text`, end before the closing
   fence line (the last line, when the block text ends with one — tree-sitter error recovery can
   omit it). Inside that region `cb.text` can match at only one place except when the body
   consists solely of fence characters (a `` ``` `` body under a `` ```` `` fence), which is the
   one hole and should be named in the comment. The blockquote/list fallback stays as it is
   (`text` lacks the continuation markers, so the contiguous search fails and we return the
   block — coarse, never wrong). ~10 lines; T7 from the seam spec binds it, with its revert hunk
   rewritten as "the bounded search → back to whole-block `find`".
2. **Producer-side, the principled fix.** Keep `code_fence_content`'s provenance: build it with
   `ProvenanceBuilder` (verbatim runs between `block_continuation` gaps — the deletion shape Plan
   1 made expressible) and carry it on `CodeBlock` as a `text_source: SourceInfo` (the
   `attr_source` precedent). Then `body_source_for` becomes `cb.text_source.clone()` and
   `text_execute.rs` can stop deriving the body from writer provenance. This is the
   "supply the right parent" fix the findings doc § 2 says this bug class wants. Cost: a
   `quarto-pandoc-types` field (74 construction sites, mostly tests and `..Default`-less literals),
   JSON writer/reader wire support, TS schema, snapshot churn.
3. **Do nothing, document it.** Real: the span is always *inside the block*, so a diagnostic is
   coarse-but-bounded, and the failing shape needs a body that equals a substring of the info
   string.

### Recommendation

**Do (1) in Plan 3 Phase 6 now, and draft (2) as a strand outside the epic.** (1) removes the
reachable mislocation at trivial cost and is what T7 already expects to bind. (2) is the right
long-term shape but is a `quarto-pandoc-types` type change with a wire-format consequence — the
epic's three plans are about *consumers* of provenance and have deliberately not touched AST
type shapes; opening that here would exit the epic with an unaudited new producer.

**Rewrite T7's revert hunk** in the seam spec: "the bounded search → back to whole-block
`find`"; the `map_offset` phrasing describes a change that cannot produce the body span.

**If wrong:** (1) leaves the fence-character-body hole; that yields a span a few bytes off but
still inside the block — the same "coarse, never wrong" class the function already accepts. If
(2) is never done, `text_execute.rs` keeps deriving body provenance from writer provenance, which
Phase 5 already classifies as out-of-epic.

> **Correction 2026-08-23 (measured in execution, Plan 3 Phase 6a).** Option 1's
> named hole is wrong in two independent ways, and the implementation's doc
> comment now carries the measured shape instead. Probed through
> `pampa::readers::qmd::read` + `body_source_for` on the landed bounded search:
>
> 1. **"A body consisting solely of fence characters" is not a hole** when the
>    closing fence is present. ```` ````{python}\n```\n```` ```` resolves to
>    `13..16`, the true body — because the region ends *at* the closing fence
>    line, so the body's ```` ``` ```` matches at region offset 0.
> 2. **The real hole is narrower, and its degradation is coarser than stated.**
>    It is a body whose *last line* is made only of fence characters in a block
>    that tree-sitter error recovery left **without** a closing fence
>    (```` ````{python}\n```\n ```` → `0..17`; ```` ````{python}\nx\n```\n ````
>    → `0..19`, truths `13..16` and `13..18`). The last-line test reads the
>    body's own final line as the closing fence, the contiguous search then
>    fails, and we take the **block-span fallback** — the whole block, not a span
>    "a few bytes off". Both stay inside the block, so option 1's "coarse, never
>    wrong" conclusion survives; only its shape and size were wrong.
>
> Also measured, because the doc comment drafted it as a *second* hole before
> retracting it: a list-indented body whose own text begins with spaces
> (`` - item\n\n  ```{python}\n    x\n  ```\n ``, `cb.text = "  x"` → `24..27`)
> and a blockquote body whose own text begins with `> `
> (`` > ```{python}\n> > x\n> ```\n ``, `cb.text = "> x"` → `16..19`) both
> resolve to the **true** offset. A run of *k* spaces or markers followed by
> content can only align one way, so the container prefix does not produce an
> early match in the shapes probed. That is not a proof of uniqueness, and the
> comment does not claim one. Note the blockquote row does **not** exercise the
> fence bound: `is_fence_line` trims only whitespace, so a marker-prefixed
> closing fence (`` > ``` ``) is not detected and the region runs to the end of
> the block — the earliest-match property alone carries that row. The
> list-indented row *does* exercise it (`trim` removes indentation).
>
> Also measured, and worth keeping because it bounds rows 3–4's alarm: an
> error-recovery block with an **ordinary** body still resolves exactly
> (```` ```{python}\nprint('hi')\n ```` → `12..23`). The fallback is specific to a
> fence-shaped final line, not to missing closing fences in general.
>
> CRLF was probed too (it was the change's one unmeasured claim): well-formed
> `\r\n` blocks resolve correctly — the parser keeps the `\r` in `cb.text`, and
> the region starts after the `\n` — and the hole above degrades identically.
> That CRLF behaviour is a **dated measurement, not pinned by a test**; the
> probe was removed and no committed artifact re-derives it.

**Owner:** Plan 3 Phase 6 (the two existing `:486` items collapse into one: comment *and* fix).
Strand draft for (2):

> **Title:** Carry `code_fence_content` provenance on `CodeBlock` instead of re-locating the body
> by search
> **Body:** `process_fenced_code_block` (pampa `fenced_code_block.rs:30`) discards the range
> `process_code_fence_content` computes for the body, so every consumer that needs the body's
> span re-derives it: `codeblock_shorthand.rs::body_source_for` byte-searches the block text (a
> body equal to a substring of its info string mislocates — fixed by a bounded search in Plan 3,
> but still a search), and `text_execute.rs:304` substrings the engine's writer provenance. Add
> `text_source: SourceInfo` to `CodeBlock` (precedent: `attr_source`), built with
> `ProvenanceBuilder` so elided `block_continuation` markers become gaps, and make both
> consumers read it. Wire/TS/snapshot impact: 74 construction sites, JSON `r`/`p` entries for the
> new field, `annotated-qmd` schema. Discovered-from `bd-mxa44voa`; outside its three plans
> because it changes an AST type, not a consumer.

---

## 2. `treesitter.rs:989` — the `shortcode_string` closure

### What is actually being asked

Whether to tighten a decoded-value/raw-span pairing whose range is provably dead. The findings
doc § 6 already settled scope (wrong-span, not drifting); the plan asks only "delete or file".

### Evidence (verified)

- `crates/pampa/src/pandoc/treesitter.rs:989-1007`: the closure strips quotes, unescapes
  `\"`/`\'`, computes `range` from the **whole node** (`:1002-1005`; `:1000-1001` are the tail of the live `text` binding — deleting them breaks the build) and returns
  `IntermediateBaseText(text, range)`.
- `treesitter_utils/shortcode.rs:31-46`: `process_shortcode_string` is the closure's only
  caller (`treesitter.rs:1008` is the only call; grep), destructures the range away at `:36`,
  and recomputes **the same whole-node range** at `:42-44`. So the closure's range is not merely
  dead — it is a duplicate of the one that survives.
- The closure's type is `&dyn Fn() -> PandocNativeIntermediate`; the `else` arm at `:37-40`
  calls it a second time just to format a panic. Nothing else needs the intermediate wrapper.

### Options

1. **Delete the dead computation and simplify the seam.** Change `process_shortcode_string`'s
   parameter to `&dyn Fn() -> String`, have the closure return the decoded string, drop the
   `let … else { panic!() }`, and add a two-line comment at the construction site: *the arg's
   range is the quote-inclusive node span paired with the decoded string; no consumer offsets
   into it (`shortcode_resolve.rs:135, :171, :837, :848, :2232, :2265` take the string only)*.
   ~15 lines, no behaviour change, no snapshot movement (the surviving range is unchanged).
2. **Tighten for real:** drive `ProvenanceBuilder` here so the arg carries content provenance
   like attribute values do after Plan 2 Phase 4. No consumer wants it (Plan 2's deferred-minor
   #5 says the same: "the only unescaper in the tree producing no provenance; if that bothers
   anyone, it is a Plan 3 strand").
3. File a strand and move on.

### Recommendation

**(1), in Plan 3 Phase 6.** It is the cheapest honest action and it removes the trap — a
reader who sees `IntermediateBaseText(decoded, raw_range)` built and returned will assume the
pairing is consumed. Do **not** do (2): it adds a producer with no consumer, which is the
"door in a field" Plan 3 Phase 3 already declined for `parse_with_parent`.

**If wrong:** if a consumer later needs sub-string positions in a quoted shortcode arg, the
whole-node span is a coarse caret, not a wrong byte; at that point (2) becomes the task and
Plan 2 Phase 4's attribute-value path is the template.

**Owner:** Plan 3 Phase 6. No strand. Plan 2 deferred-minor #5 closes with it.

---

## 3. A splice-safety guard for `q_2_28`'s `end_offset()` reader

### What is actually being asked

`q_2_28.rs:80` reads `location.end_offset()` from a diagnostic — on a `Concat`-rooted span that
is content length, silently wrong (findings § 1 table) — and uses it to find a splice point. The
plan asks whether a generic "refuse to splice a non-`Original` span" guard should exist.

### Evidence (verified)

- Reachability today: `Q-2-28` has no Rust emission site —
  `grep -rl 'Q-2-28' crates` hits only the corpus (`pampa/resources/error-corpus/Q-2-28.json`,
  `_autogen-table.json`), the catalog, and the conversion. Same for `Q-2-33`
  (`q_2_33.rs:74-75`, the sibling that reads **both** `start_offset()` and `end_offset()`).
- **The splice is already content-checked.** `find_violation_offsets` (`q_2_28.rs:97-135`)
  scans back from the offset to a `\n`, forward over whitespace, and **only returns a violation
  if the next four bytes are literally `>}}}`** (`:122-124`). A wrong offset therefore cannot
  splice the wrong bytes: it either finds nothing (skipped) or finds a *real* `\n…>}}}` shape
  somewhere else — which is itself a Q-2-28 violation and a correct edit. Its failure mode is
  **missed or duplicated** fixes, not corruption.
- The duplicate case is a pre-existing hazard independent of provenance: two diagnostics
  resolving to the same `newline_start` would `replace_range` the same region twice
  (`apply_fixes`, `:143-155`, sorts by `Reverse(newline_start)` but does not dedupe). Noted,
  not this epic's.

### Options

1. **Generic guard in `rule.rs`/`utils`** refusing any span whose `resolve_byte_range()` is
   `None`. Over-broad: it would also refuse `Substring{Original}` chains, which resolve
   correctly.
2. **Replace the accessor at the site.** Use `resolve_byte_range()` (honest `None` on `Concat`,
   correct on `Substring{Original}`) instead of `end_offset()`, and `continue` on `None`. Same
   in `q_2_33.rs:74-75`. This is the findings doc's rule applied literally: "never
   `start_offset`/`end_offset` on a possibly-`Concat` span".
3. **Do nothing, document.** Real: the content check is the splice guard, and both codes are
   corpus-only.

### Recommendation

**(2) plus a comment naming the content check as the splice guard — in Plan 3 Phase 6 as a
two-site accessor fix, no new abstraction.** It is the accessor rule, it costs four lines, and
it converts a *silent* wrong number into an honest skip. A generic guard is not warranted: the
conversion's own `== ">}}}"` check is the real safety property and should be named as such so
nobody removes it as "redundant".

**If wrong:** if a future emitter attaches a `Substring{parent: Concat}` location to Q-2-28,
(2) skips the fix (the user sees the diagnostic but `--fix` does nothing for it) instead of
mis-scanning from content-offset `0`; with the content check in place either outcome is safe.

**Owner:** Plan 3 Phase 6. No strand; the Plan 2 obligation-8 test
(`attr_provenance_splice_test.rs`) already pins the attribute channel.

---

## 4. What actually prevents the founding crash

### What is actually being asked — and why the framing is wrong

Task F (row 18) observed that with `snap_span_to_char_boundaries` reduced to a pass-through,
two tests that build a mid-character span **directly** still rendered without panicking, and
flagged the snap's panic-prevention role as unwitnessed. Plan 2's hand-off (e) escalated that
to "determine which renderer still aborts". The dispatch prompt then hypothesised that the
**clamping** half of the helper, not the snapping half, might be the load-bearing behaviour.

Both are mis-framed, because the mechanism was already on record in the upstream crate:

- `quarto-source-map` commit `8e07717` (2026-08-21, released in 0.1.2) makes
  `FileInformation::offset_to_location` return the **floored** offset
  (`src/file_info.rs:116-125`: `safe_offset` walks left to a char boundary and is returned as
  `Location.offset`). Every `map_offset` goes through it (`mapping.rs:38`).
- `quarto-error-reporting` commit `66d115c` took 0.1.3, and `5e48166` — *"Re-anchor the
  mid-character-span crash tests after the 0.1.3 floor"* — renamed the two tests and wrote in
  their doc comments that "by the time `map_offset` hands this test's span (21..28) to the
  renderer it has already become 19..28 — the snap in this crate is never exercised against a
  mid-character offset at this level, because one can no longer be constructed here" and that
  the unbinding is "accepted, not an oversight".
- Task F's **own row 17** reverted that floor and witnessed its effect one repo away (the
  zero-width-label tests went red), then row 18 observed the vacuity without connecting it to
  row 17. The doc comment row 18 quotes as "accepted-unbound" is the one that explains it.

So the question is not "which renderer still aborts" (ariadne 0.6.0 still aborts — see A, C, E
below; it has never changed), nor "is clamping what saved us" (it is not — C). The question is
**what the helper should claim about itself now that a guard upstream of it makes its snapping
half unreachable through the mapping path.**

### The experiment

**Instrument.** A commit state, not a reconstruction: q2 at `fdf55e777` (= `v0.24.0`, the
README's repro state, pre-Plan-2 mapping, lock pins `quarto-error-reporting` 0.2.1 and
`quarto-source-map` 0.1.0) in a throwaway worktree `.scratch/q2-v0240` with its own
`CARGO_TARGET_DIR=.scratch/target-v0240`, the fixture copied to `repro-fixture/`. Overrides
went in **that worktree's** `Cargo.toml` (merged into its existing `[patch.crates-io]` block —
a second block is a TOML duplicate-key error) and needed `cargo update -p <crate>` to take,
because the patch source's version (0.2.2) differs from the locked one (0.2.1) and cargo
otherwise reports `[patch.unused]`. The helper was varied in `~/src/quarto-error-reporting`
(clean at `922b09c6c`) and restored with `git checkout --` between configurations. Command in
every row: `cd repro-fixture && rm -rf _site && ../../target-v0240/debug/q2 render`.

| cfg | `quarto-error-reporting` | helper | `quarto-source-map` | result |
|---|---|---|---|---|
| A | 0.2.1 crates.io (pre-fix) | — (no helper; ariadne spans unclamped) | 0.1.0 | **abort 101** |
| B | path @ `922b09c` | full: (1)+(2)+(3) | 0.1.0 | clean 0 |
| C | path | (1)+(2) clamp kept, (3) snap removed | 0.1.0 | **abort 101** |
| D | path | (3) snap kept, (1)+(2) clamp removed | 0.1.0 | clean 0 |
| E | path | pass-through | 0.1.0 | **abort 101** |
| F | path | pass-through | path @ `09ec6d1` (0.1.3, floor) | clean 0 |
| G | current branch `d6ee475be`, stock lock (0.2.2 / 0.1.3), Plan 2 mapping fix | full | 0.1.3 | clean 0, carets **correct** (`:7:16`, `:7:37`) |

Verbatim observed lines (from `.scratch/run-{A..F}.log`; identical text in A, C, E):

```
thread 'main' (68082495) panicked at /Users/gordon/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ariadne-0.6.0/src/write.rs:84:59:
end byte index 37 is not a char boundary; it is inside '✨' (bytes 35..38 of string)
EXIT=101
```

```
Rendered 1 of 1 files to /Users/gordon/src/q2/.worktrees/workspace-4/.scratch/q2-v0240/repro-fixture/_site — 2 warnings
EXIT=0
```

B and F print byte-identical diagnostics (second caret at `_quarto.yml:7:36`, underline
`────┬───` over `✨</span`): for this repro the floor (start 37→35) and the snap (start
37→35, end already aligned) agree.

The helper variants, for the record (C and D; E is `let _ = content; start..end`):

```rust
// C — (1)+(2) kept, (3) removed            // D — (3) kept, (1)+(2) removed
let len = content.len();                    let len = content.len();
let s = start.min(len);                     let mut s = start; let mut e = end;
let e = end.min(len).max(s);                while s > 0 && s < len && !content.is_char_boundary(s) { s -= 1; }
s..e                                        while e < len && !content.is_char_boundary(e) { e += 1; }
                                            s..e
```

### What the outcomes mean

1. **The snap half (3) was the load-bearing fix at the version that crashed** (C aborts, D does
   not). The dispatch prompt's clamping hypothesis is refuted; the commit name was right. The
   founding mechanism is an *end* offset one byte left of a char boundary (`37` inside
   `35..38`) — consistent with the one-byte-left YAML mapping bug, not with the `length() - 1`
   fallback, which the prompt assumed.
2. **Today the in-crate snap is redundant for every offset that arrives via `map_offset`** (F
   is clean with the helper removed). All three call sites (`diagnostic.rs:878`, `:937`,
   `:1032`) feed it `map_offset` results, so on the shipped stack it is defense in depth, not the
   guard. Its widening behaviour (ceil the end) does differ from the floor's (truncate the end)
   for an end-only mid-char offset; on the repro they coincide.
3. **The doc comment is not wrong about the renderers** — both still panic on a mid-char byte
   (A/C/E prove ariadne; the annotate-snippets claim was tested at `4da3385` and nothing in this
   experiment contradicts it). It is **wrong about its own role**: "we normalize here rather
   than trusting the input" is no longer the reason the render survives. And the tests are not
   "testing the wrong property": `snap_span_widens_to_whole_characters` pins the helper's
   arithmetic, which is exactly what a redundant guard can be tested for; the two end-to-end
   tests are correctly labelled smoke checks.
4. **There is no test on the shipped stack that reddens when the *upstream* floor is lost**,
   except cross-repo ones in `quarto-error-reporting` (row 17's zero-width-label tests). q2
   itself has nothing: if `quarto-source-map` ever reverted the floor *and* the in-crate snap
   were removed, the abort would be back with no q2 test noticing. That is the real gap.

### Recommendation

- **Rewrite the helper's doc comment** (upstream, next `quarto-error-reporting` release; a
  doc-only change): keep the two-renderer panic claim, then state plainly: *since
  `quarto-source-map` 0.1.2 every offset reaching this helper through `map_offset` is already
  floored, so the snapping half is unreachable from the mapping path and is defense in depth
  against a mapping regression or a caller passing raw offsets; the clamp half still guards
  the `length() - 1` fallback at `:842-851` and inversion.* Point at commit `5e48166` and at
  the q2 pin below.
- **Add the pin in q2, not upstream — one end-to-end test** in
  `crates/quarto/tests/integration/` that drives the real binary over the README's case A
  fixture (`_quarto.yml` navbar `text: '<span id="x">Ask AI ✨</span>'`) and asserts exit 0
  plus both `Q-2-9` lines. Its revert hunk is **not in q2**: it reddens only if the mapping
  regresses *and* both guards are gone. Label it accordingly (the T6 convention: an
  upstream-behaviour pin). It is the cheapest witness of the founding bug that exists and it
  currently does not exist anywhere.
- **Do not** downgrade the snap to "widening only" or remove it. F shows it is not needed
  today; A/C/E show exactly what it costs to be wrong about that.

**If wrong:** if the floor is later removed upstream (it truncates a span's end, which someone
might "improve"), the snap carries the render and the rewritten comment says so. If the pin is
skipped, the next regression presents exactly as the founding one did: `_site/` written, exit
101, log truncated.

**Owner (decided 2026-08-23):** both the upstream doc-only PR and the pin are Plan 3 Phase 6
items; the pin also asserts the measured carets (`:7:16`, `:7:37`) so it binds to q2's mapping,
not only to the upstream guards. Item (e) is rewritten from "determine which renderer still
aborts" to "pin the founding repro end-to-end in q2".

### 4.6 Restoration statement

Verified after the last configuration:

- `~/src/quarto-error-reporting`: `git status --short` empty, `HEAD` =
  `922b09c6c8d4e68177e546c1d2c334f35fd5eda4`.
- `~/src/quarto-source-map`: `git status --short` empty, `HEAD` = `09ec6d1` (untouched; it was
  only *referenced* by config F's patch).
- Throwaway worktree removed (`git worktree remove --force .scratch/q2-v0240`;
  `git worktree list` shows none under `.scratch`), `.scratch/target-v0240` deleted.
- workspace-4: `git status --short` empty; neither manifest contains a
  `quarto-error-reporting = { path` override; lock SHAs unchanged —
  `ccc01dd2c8cc77a0d1199fe7efcace923f31e31c  Cargo.lock`,
  `d632527b1bf8c98fda4faa75330e2fb57bb0399e  crates/wasm-quarto-hub-client/Cargo.lock`.
- Logs kept under `.scratch/` (excluded from git): `run-{A..F}.log`, `build-*.log`,
  `probe-q1.log`.

---

## 5. Narrowing `is_gapless` (`span_assert.rs:234`)

### What is actually being asked

`resolve_span` (`crates/quarto-config/src/span_assert.rs:252-…`) refuses with
`SpanProblem::Concat` whenever the enclosing `Concat` has *any* gap, even if the queried
`Substring` lies inside one contiguous piece. A multi-line `#|` block is always gappy (each
line's `#| ` marker is a gap — `cell_options/mod.rs:247-263`), so a span inside one option line
is refused. The helper documents this as a "conservative over-approximation"
(`span_assert.rs:189-193`). Narrow, or leave documented?

### Evidence (verified)

- `concat_pieces_are_contiguous` (`:199-227`) walks every piece using each piece's own
  `map_offset(0)`/`map_offset(length())` — correctly avoiding the declared per-piece `length`
  for *positions*. `is_gapless` (`:234-240`) recurses through `Substring` to the parent and
  never looks at the sub-range.
- Blast radius is test-only: `span-assert` is a feature enabled only from `[dev-dependencies]`
  (Plan 2's final review verified this under `resolver = "2"`); 24 `resolve_span(` call sites,
  all in tests.
- One test already documents the refusal in a 20-line NOTE and proves its point via `map_offset`
  instead (`codeblock_shorthand.rs:1417-1440`). That test is a ready-made binding: replace the
  NOTE with `resolve_span(...).expect(...)` and the narrowing is the hunk whose revert reddens it.
- The narrowing *selects* pieces by content offset (which pieces does `[start, end)` of the
  `Substring` overlap — this is the one place the declared `length` field is the right number,
  because it is a content length being compared against content offsets), then checks
  contiguity of just those pieces via `map_offset`. No source position is ever derived from a
  content length, so the "content length is not source length" trap does not apply. Still, it
  is content-space arithmetic in the module whose job is to *check* spans, and Plan 2 was right
  that it belongs with a plan that can test it.

### Options

1. **Narrow** to the touched pieces (~25 lines in `is_gapless`/`concat_pieces_are_contiguous`,
   taking an optional content sub-range), bound by flipping the NOTE above into an assertion.
2. **Leave it**, documented as it already is. Real: nothing user-visible depends on it.

### Recommendation

**(1), Plan 3 Phase 6, low priority — after the correctness items.** The cost is small, the
binding test is already written, and the benefit is that every future test over a `#|` option
value can use `resolve_span` instead of re-deriving the `map_offset` pair by hand — which is
how `span_assert` earns its keep. If Phase 6 runs out of room, (2) is acceptable and needs no
strand: the helper's own comment is the record.

**If wrong:** a bug in the narrowing yields a wrong `Ok` in a *test helper*, which a test would
then assert against the wrong text — visible in the failing assertion, never in a rendered
caret.

**Owner:** Plan 3 Phase 6. No strand either way.

---

## 6. A test for a caught panic on an *error*-severity diagnostic

### What is actually being asked

Whether `render_diagnostic_guarded` (`crates/quarto/src/commands/render.rs:1276-1295`) can
ever rescue an exit code that must stay non-zero, and whether to pin that with a test.

### Evidence (verified) — the argument on record is stated backwards

The hand-off says "`diagnostic_counts()` runs before printing". It does not:
`print_render_diagnostics(&summary, …)` is at `render.rs:836` and `should_exit_nonzero(&summary,
…)` — which calls `diagnostic_counts()` — is at `:848`; the project path is the same shape
(`:1010` then `:1027`). The property that actually holds is **immutability**: both calls take
`&summary`, the guard's closures borrow `&DiagnosticMessage`/`&SourceContext` only (the
function requires `UnwindSafe` without `AssertUnwindSafe`, `:1264-1269`), so a swallowed
render panic cannot remove a diagnostic from the summary the gate counts. The conclusion is
right; the stated reason would mislead the next reader into thinking reordering is safe.

Existing coverage (`tests/integration/diagnostic_render_panic_boundary.rs`) asserts exit **0**
on a caught panic over *warnings* (`:86`, `:124`, `:158`). Nothing asserts exit **1** on a caught
panic over an *error*. `render_exit_codes.rs:28-69` already has the fixture: a duplicate
crossref id → exactly one `Q-15-1` error.

### Options

1. **Add the test** (~20 lines): `render_exit_codes`'s fixture +
   `QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER=0` → assert `!status.success()`, stderr contains
   `internal error rendering diagnostic Q-15-1`, and does **not** contain the `Q-15-1` text
   rendering (so the fault really hit that diagnostic).
2. Leave the structural argument.

### Recommendation

**(1), Plan 3 Phase 6, and fix the prose in Plan 2's hand-off (h) and in the guard's doc
comment (`:1256-1258` says "before `should_exit_nonzero`", which is right; the hand-off's
ordering claim is what is wrong).** Label the test honestly: its revert hunk is *not* a
mutation of the guard — no edit to `render_diagnostic_guarded` can change the count — it is
"compute exit status from what was *printed*", a refactor someone could plausibly make. It is an
invariant pin, like T6.

**If wrong:** nothing; the test is cheap and the failure it guards is the one the guard's own doc
calls its most dangerous.

**Owner:** Plan 3 Phase 6.

---

## 7. `render.rs:904` — the unguarded pre-render `to_text(None)`

### What is actually being asked

Whether the one per-diagnostic `to_text` call outside the guard needs it, and what would change
the answer.

### Evidence (verified)

- `render.rs:897-905`: loop over `underscore_typo_diagnostics` + `project_kind_diagnostics` +
  `config_diagnostics`, each `eprintln!("{}", diagnostic.to_text(None))`.
- `to_text_with_renderer` (`quarto-error-reporting/src/diagnostic.rs:461-481`): the excerpt
  renderer runs only under `(has_any_location, Some(ctx))`; with `ctx = None` it takes the
  structured-text branch, whose only location use is `loc.start_offset()` printed as
  `"  at offset {}"` (`:508-517`) — a number, no slicing. **Structurally unreachable** for byte
  slicing, as Plan 2's final review said. (Minor: on a `Concat` that prints content-offset `0`
  — a wrong-but-harmless number, the findings-doc accessor rule again.)

  > **Correction 2026-08-23 (measured in execution, Plan 3 Phase 6d).** Both line citations in
  > the bullet above are wrong; the bullet's *substance and conclusion* are confirmed exactly as
  > written, including the parenthetical minor at its end. Re-measured against the version
  > actually locked (`Cargo.lock`: `quarto-error-reporting 0.2.2`), at
  > `~/.cargo/registry/src/index.crates.io-*/quarto-error-reporting-0.2.2/src/diagnostic.rs`:
  >
  > | claimed | measured | what is there |
  > |---|---|---|
  > | — | `:442` | `pub fn to_text_with_renderer(` |
  > | `:461-481` | **`:460-481`** | `let has_source_render = if let (true, Some(ctx_val)) = (has_any_location, ctx) { … };` — the gate, one line earlier than claimed |
  > | — | **`:486`** | `if !has_source_render {` — the structured-text fallback branch |
  > | `:508-517` | **`:522`** | `writeln!(result, "  at offset {}", loc.start_offset())` — one line. `:508-517` is the **other** branch of the same `if let Some(ctx) = ctx`: the ctx-present arm, which prints `"  at {path}:{row}:{col}"` from `loc.map_offset(loc.start_offset(), ctx)` |
  >
  > So the first citation is one line narrow at the front, and the second points at the wrong
  > branch entirely — the `at offset` write it names is not inside that range at all. The
  > conclusion is untouched: with `ctx = None` the excerpt renderer is not entered, the
  > structured-text branch's only location use is a printed number, and the byte-slicing path is
  > structurally unreachable. The parenthetical minor is likewise unaffected — a `Concat`-rooted
  > location printing content-offset `0` through `start_offset()` is the accessor rule's shape
  > inside the crate that owns the renderer, and it lives at `:522`.
  >
  > The `render.rs` guard-wrap comment that Phase 6d landed cites the **measured** anchors
  > (`:442`, `:460-481`, `:486`), not the ones above. A code comment must not carry a citation
  > its author has measured to be wrong, however small the drift: the next reader lands a line
  > off — or, as with `:508-517`, in the wrong branch entirely — and cannot tell whose drift it
  > is.

- **What would change it is already on screen.** `config_sources` — the vector of
  `_quarto.yml`/profile/extension-manifest paths that `attach_config_source` uses to *give*
  config diagnostics a `SourceContext` at print time — is built at `:883-890`, fourteen lines
  above the loop. The natural next improvement ("why do config-parse diagnostics print without
  a source excerpt?") is to bind those sources and pass `Some(ctx)` here; the day that happens
  this becomes a ninth slicing site with no guard.
- Q4's result does not weaken the "no excerpt" protection: the panic needs a renderer to slice,
  and `None` never reaches a renderer. It *does* mean that once a ctx is passed, the site is
  protected by the upstream floor like the other eight — the guard is still the right boundary
  because a floor is not a panic boundary.

### Options

1. **Wrap it now** in `render_diagnostic_guarded(code, || diagnostic.to_text(None))` — uniform
   with the other eight, ~3 lines, and Plan 2's `grep -c = 8` evidence becomes 9.
2. **Comment only**: record why `None` is safe and that binding `config_sources` here requires
   the guard.
3. Nothing.

### Recommendation

**(1) + a one-line comment, Plan 3 Phase 6.** Wrapping costs nothing, removes a trap that sits
next to its own bait, and makes the "every diagnostic render is guarded" statement true without
a carve-out. Update Plan 2's Phase 5 evidence count in the same commit.

**If wrong:** none — the guard is a no-op on a path that cannot panic.

**Owner:** Plan 3 Phase 6.

---

## 8. `bd-g7qh1ltt` — `handleConcat` reconstructs the wrong string

### What is actually being asked

Whether the strand's self-scoping (outside all three plans) is right, and whose it is. The
strand offers two fixes: (a) ship decoded bytes on the wire; (b) fall back to the piece's full
source text.

### Evidence (verified)

- `ts-packages/annotated-qmd/src/source-map.ts:239-270`: for each piece
  `[pieceId, offset, length]`, `mappedSubstring(toMappedString(pieceId), 0, length)` — the first
  `length` characters of the piece's **source** text. Correct only when the piece is verbatim.
- Public callers of `toMappedString`: `block-converter.ts:331, :381, :412, :448, :479, :510`
  (table caption/head/body/foot/row/cell), `meta-converter.ts:197, :231`. Those spans are
  `combine()`-produced `Concat`s, which the findings doc § 3 proves byte-identical by
  construction — so no caller is hit today, as the strand says.
- **The Rust side has the same property.** `SourceInfo` carries no content bytes and no
  verbatim tag (findings § 3: "`SourceInfo` carries no verbatim tag — it lives in the builder…
  and does not survive into the emitted value"). Rust cannot reconstruct a decoded string from
  a `Concat` either; the content lives on the AST node (`Str.text`, the attribute value) and
  provenance is a *map* from content offsets to source. So this is not a "wire-format gap" —
  the wire faithfully transmits everything the Rust value holds.

### Options

(a) **Put decoded bytes on the wire** for replacement pieces: duplicates text that is already in
the JSON on the node, bloats the pool, and makes the pool the second place content lives.
(b) **Fall back to full source text**: produces a `MappedString` whose `value` has a different
*length* than the content, so every `mappedSubstring(…, contentOffset)` over it is off — worse
than today, and self-inconsistent with `resolveChain`.
(c) **Take the content from the consumer**: `toMappedString(id, content?: string)` — the AST
node's own text — and build the `MappedString` as (content, map-from-provenance). When
`content` is omitted, keep today's behaviour for verbatim-only concats and throw/flag through
`errorHandler` when a piece's source length ≠ its content length (the one case provenance alone
can detect; a 1→1 fold it cannot, which is the documented limit).

### Recommendation

**The boundary is right — keep it outside the epic — but the strand's root-cause line should be
rewritten so the fix is not (a).** Re-scope to (c): "`toMappedString` cannot derive content from
provenance because provenance is a map, not a store; callers that need the decoded string
already have it on the node." That is an `annotated-qmd` API decision and belongs to the TS
source-tracking line (`bd-1d6io`, branch `braid/bd-1d6io-annotated-qmd-source-tracking`),
whose owner is already changing `SourceInfoReconstructor`'s contract. Priority stays 2/latent.

Suggested replacement body for the strand:

> `toMappedString(id).value` is wrong for any `Concat` with a non-verbatim piece, and it cannot
> be made right from the wire: `SourceInfo` (Rust and TS alike) is a map from content offsets to
> source and carries neither decoded bytes nor a verbatim tag, so the decoded string is not
> reconstructible from provenance by design — it lives on the AST node. Fix by letting the
> caller supply the content (`toMappedString(id, content?)`) and building the `MappedString` as
> (content, provenance map); when omitted, detect the mismatched-length case via the piece's
> own source extent vs declared length and route it to `errorHandler`. Do **not** ship decoded
> bytes on the wire (duplicates node text into the pool) or fall back to source text (value
> length ≠ content length breaks every content-offset consumer). No current caller is affected
> (`combine()` concats are byte-identical by construction). Owner: `bd-1d6io`.

**If wrong:** if a caller someday needs a decoded string for a node whose text is not on the
node (there is none today), (a) becomes necessary and the pool gains a content field — a wire
bump, not a rewrite.

**Owner:** `bd-1d6io` (TS source tracking), not the provenance epic. **Done 2026-08-23:** the
strand body was re-scoped as above (comment `c-2edupaog`) and linked `related` → `bd-1d6io`.

---

## Sweep — what else is a question rather than a task

**Plan 3, gating table (§ Read this first).** Stale on three rows and should be rewritten, not
just checked: `quarto-source-map` 0.1.2 *and* 0.1.3 are released and in the lock (`Cargo.lock`:
0.1.3); `ProvenanceBuilder` shipped in 0.1.3 (`545f50d`); and "Plan 1's Phase 1 doc rewrite
(still open)" is closed — the `preimage_in` doc comment in 0.1.3 (`source_info.rs:416-441`)
carries the offset-claim-not-byte-identity wording Phase 1's last item wants to cite. Nothing in
Plan 3 is gated any more; the `[patch.crates-io]` instructions for consuming 0.1.2 early can be
deleted.

**Plan 3 Phase 6, the census cross-check.** Plan 2's final fix wave found and fixed a
*sixth* decoded/raw pairing at `website_post_render.rs:217` (FIX-2) that is not in the
findings-doc § 6 census table (six sites + the seventh shortcode closure). The cross-check item
should add it so the table and the code agree.

**Plan 3 Phase 1, "(a) state whether the fallback moves any snapshot".** A task, correctly
phrased; but note it can be answered now — 0.1.2 is in the lock and `cargo nextest run
--workspace` is green on this branch (Plan 2's final fix report), so the answer is "none moved"
unless Phase 1 finds otherwise. Record it rather than leaving it as a future measurement.

**Plan 3 Phase 2, `offset_to_location_bytes` routing.** Conditional ("a `quarto-yaml`-side
disagreement is out of scope — file a strand"). Fine as written; no decision needed until
measured.

**Plan 3 Phase 5, T4.** Conditional characterization probe; correctly labelled; nothing to
decide.

**Plan 2 deferred-minor list → strands?** None. #5 closes with Q2's recommendation; #8 is Q6;
#6 (append the document name to the `internal error rendering diagnostic` line) is a two-line
enhancement that can ride Q7's Phase 6 edit rather than a strand; #1, #4, #7, #9 stay "ride"
for the reasons the review gave.

**Not in the list, worth one line each:**

- `q_2_33.rs:74-75` reads `start_offset()` *and* `end_offset()` — Q3's sibling; same fix.
- `to_text_with_renderer`'s no-context branch prints `loc.start_offset()` as "at offset N"
  (`diagnostic.rs:516`) — content-offset `0` for any `Concat`-rooted location. Harmless, but it
  is the accessor rule's exact shape inside the crate that owns the renderer; `root_file_id` +
  `map_offset` need a ctx, so the honest fix is to omit the line when the span is not
  `Original`. Upstream, doc-level priority.
- `apply_fixes` in `q_2_28.rs` does not dedupe violations with equal `newline_start` (see Q3) —
  pre-existing, not provenance; a one-line `dedup_by_key` if anyone touches the file.

---

## Summary table

| # | question | recommendation | owner | confidence |
|---|---|---|---|---|
| 1 | `codeblock_shorthand.rs:486` `find()` | **Mis-framed** (the `map_offset` pair gives the block hull). Bounded between-fences search now; rewrite T7's revert hunk; strand for carrying `code_fence_content` provenance on `CodeBlock` | Plan 3 Phase 6 + strand (draft above) | high — mislocation reproduced (`4..10` vs `12`) |
| 2 | `shortcode_string` closure | Delete the dead range computation; narrow the seam to `Fn() -> String`; comment the pairing. No strand | Plan 3 Phase 6 | high |
| 3 | `q_2_28` splice guard | No generic guard. Replace `end_offset()` with `resolve_byte_range()` here and in `q_2_33`; comment the `== ">}}}"` check as the splice guard | Plan 3 Phase 6 | high |
| 4 | what prevents the founding crash | **Mis-framed twice.** Snap (3) was load-bearing at the crashing version (C aborts, D clean); today the upstream `offset_to_location` floor alone suffices (F clean). Rewrite the helper's doc to say it is defense in depth; add an end-to-end q2 pin over the README fixture; keep the snap; the pin asserts carets too | Plan 3 Phase 6 (both the upstream doc PR and the pin) | high — six-config experiment, restored |
| 5 | narrow `is_gapless` | Narrow to touched pieces, bound by the existing NOTE-test; low priority; leaving it is acceptable | Plan 3 Phase 6 | medium |
| 6 | caught panic on error-severity | Add the ~20-line pin using the `Q-15-1` fixture; **correct the ordering claim** (print precedes the gate; immutability is the invariant) | Plan 3 Phase 6 | high |
| 7 | `render.rs:904` unguarded `to_text(None)` | Wrap it now + one comment naming `config_sources` as what would change the calculus; evidence count 8 → 9 | Plan 3 Phase 6 | high |
| 8 | `bd-g7qh1ltt` boundary | Boundary right (outside the epic); root cause mis-stated — provenance is a map, not a store; re-scope to caller-supplied content; owner `bd-1d6io` | `bd-1d6io` (edit the strand) | medium-high |
| — | sweep | Plan 3's gating table is stale (all gates closed); add the sixth site to the Phase 8 census cross-check; no deferred-minor becomes a strand | Plan 3 | high |
