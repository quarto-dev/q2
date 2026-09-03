# Make q2 self-documenting again: don't execute a cell that is being displayed

**Strand:** bd-knitr-executes-nested-display-fence-atbtktdj (epic: bd-98m98wg8)
**Branch:** `braid/bd-knitr-executes-nested-display-fence-atbtktdj-mask` off
`origin/main` @ `85e98fb02`.

The **minimal, forward-compatible slice** of a larger epic. The epic plan, the
full design-decision table, the open questions and the
engine-hand-off-assembler design live on
`braid/bd-98m98wg8-self-documenting-cells-epic` and are reviewed separately.
Nothing here has to be undone by any of it.

## The bug

A `{r}` fence *displayed* inside another code block is executed by knitr, and
the author's example is replaced by knitr's own cell scaffolding.

Measured at `eac0c7acf` and **independently reproduced at `85e98fb02`** on
`.scratch/nested/r1.qmd` — one real cell plus two display blocks:

| marker | occurrences | meaning |
| --- | --- | --- |
| `REAL-CELL-RAN` | 2 | correct: source echo + output |
| `DISPLAY-BLOCK-RAN` | 2 | **wrong** — the displayed example ran |
| `DISPLAY-OPTS-RAN` | 1 | **wrong** — with `echo=FALSE` the author's source vanished entirely, leaving only output |

The reader is shown knitr's intermediate markup instead of the example:

    ::: {.cell}
    ...{.r .cell-code}
    ::: {.cell-output .cell-output-stdout}

### Two symptoms worse than "wrong output"

**A displayed example in a language you don't have installed kills the whole
render.** Verified at `85e98fb02`: a document with one live `{r}` cell and a
` ````markdown ` block displaying a `{python}` example exits with

    reticulate (local) python_not_found("Installation of Python not found…")
    Quitting from pyfail.rmarkdown:131-133 [unnamed-chunk-2]
    Execution halted
    error: while rendering pyfail.qmd

knitr hands the *displayed* example to reticulate. This is the most
user-visible face of the defect and the one a documentation page hits first.

**A blockquoted display block loses its contents entirely.** The cell escapes
the blockquote — two `.cell` divs are emitted and the display block renders
**empty**.

### Why this matters beyond the bug

Q-2-50's own hint tells authors:

> To display the cell, wrap it in a `markdown` code block and write single
> braces; fenced code blocks are displayed verbatim.

That is the construct this bug breaks. The documented escape hatch and the
defect are the same thing, so **today there is no way to write Quarto
documentation about the execution feature.** This plan makes the hint true.

Reachable only when the engine is already live: AST-based resolution
(`computational_languages`, `resolution.rs:249`) correctly declines to start
knitr for a document whose only `{r}` fence is displayed. One real cell
anywhere turns every displayed example into a live one.

## Approach

One seam — `crates/quarto-core/src/stage/stages/engine_execution.rs`, **inside
the per-engine loop**:

```
mask(ast clone) -> serialize_ast_to_qmd -> engine.execute -> unmask -> capture -> reparse -> reconcile
```

Mask needs the AST (only it knows what is inside a code block). Unmask does
not — the marker is self-identifying — so it runs textually on the engine's
returned markdown.

**The mask must be applied inside the loop, per iteration.** `to_run` is
iterated with `ast` re-serialized each time and left *unmasked* between
iterations; masking once before the loop would silently leave the second
engine unprotected.

The `qmd_source_info` passed to `.with_source_info(...)` is the one produced by
serializing the **masked** clone — not a separate unmasked serialization.

Because every engine goes through this one `serialize_ast_to_qmd` call, this
covers knitr, q2's jupyter text engine, and TS extensions with no per-engine
work.

### The transform

Line-wise on the block's text:

```
```{r}              ->  ```{.r q2-nested-executable}
```{r, echo=FALSE}  ->  ```{.r q2-nested-executable, echo=FALSE}
```

The leading `.` is what does the work. All three partitioners require an
alphanumeric first character after `{`:

| scanner | pattern fragment | source |
| --- | --- | --- |
| knitr `all_patterns$md$chunk.begin` | `\{([a-zA-Z0-9_]+( *[ ,].*)?)\}` | knitr 1.50, read live from R |
| q2 jupyter `parse_code_blocks` | `\{(\w+)\}` | `engine/jupyter/text_execute.rs:128` |
| TS `breakQuartoMd` `startCodeCellRegEx` | `\{([=A-Za-z][=A-Za-z0-9._]*)…\}` | `ts-packages/quarto-api/src/markdownRegex/index.ts:779` |

The marker exists only so the unmask knows which openers were ours, and never
rewrites an author's own `{.r}`.

**Prefix handling is asymmetric, and the asymmetry is the point.** Mask
operates on a `CodeBlock`'s `text`, from which the reader has *already
stripped* any blockquote `> ` — the writer re-adds it on serialize. Only
**unmask**, which runs textually over engine output, ever sees a `> `. So the
mask side needs no prefix handling; the unmask side must not be `^`-anchored,
or it will miss every blockquoted fence (knitr's own pattern is `^[\t >]*`,
so those fences really do execute).

**Byte-exactness is a requirement, not an aspiration** — see "Reconcile"
below. The unmask must restore leading whitespace, inter-token spacing
(` ``` {r} `), trailing whitespace, and fences of any backtick width exactly.

### Scope

**In:** executable fence openers inside a *markdown-displaying* `CodeBlock` —
one whose attr classes are **empty** (no info string) or `["markdown"]`. Use
`engine_cell_lang` (`engine/capture_splice.rs:86`) to skip blocks that are
themselves executable cells.

**Correction (found during task 2, bd-rbpkzqjo):** the empty-classes case
does **not** include a 4-space indented code block — qmd has no
indented-code-block grammar production at all. `grammar.js`'s
`_indented_code_block_error` (~1223-1231) documents this as a deliberate,
blanket known limitation, and the scanner emits
`INDENTED_CODE_BLOCK_DISALLOWED` (`scanner.c:2664`) rather than an
indented-code-block node — verified to fire in every context (top-level and
inside a list item alike), never producing a `CodeBlock` at all. So a
4-space-indented display block is not a second way to reach the
empty-classes AST shape; it cannot reach the AST at all. The only way to
construct an empty-classes `CodeBlock` is a bare fenced block with no info
string.

**Language predicate — every braced opener, engine-agnostic.** Mask any
`{word}` opener regardless of language, not only languages the current engine
claims. This is what fixes the fatal `{python}`-in-an-R-document case above:
knitr hands unclaimed languages to reticulate rather than ignoring them. The
seam is engine-agnostic, so the predicate should be too.

**Display classes — empty or `markdown` only.** An author writing ` ````qmd `
gets nothing. Deliberate: Q-2-50's hint names `markdown`. Widening is a
separate decision.

**Out, deliberately:**

- **`RawBlock`.** A spike measured that masking a `{=markdown}` RawBlock
  converts "wrongly executed" into *silently mangled*: the writer emits such
  blocks **unfenced** (verified — `pampa -t qmd` drops the wrapper), so the
  inner cell becomes a genuine top-level cell in the engine input. Masking it
  would free a never-executed cell that renders with classes
  `.r q2-nested-executable` — unhighlighted and undiagnosed (bd-4gzwls31). A
  downgrade, not a fix. The assembler owns RawBlock.
- **Doubled-brace `{{lang}}`** — no scanner executes them (verified untouched
  at baseline); that is a diagnostic problem
  (bd-q250-nested-fence-blind-spot-t68z1lsw).
- **`{=html}`-style openers** — only `breakQuartoMd` matches `=`, and it
  mis-*partitions* rather than executes.
- **A cell nested inside another executable cell** — rewriting there would
  corrupt code that is about to run.

### Provenance

Masked text is longer than its source. The writer's map is *output*-indexed —
`pieces.push((block.source_info().clone(), buf.len() - start))`
(`pampa/src/writers/qmd.rs:3104`) — so a longer block shifts nothing for any
*other* block. But *within* a lengthened block the mapping goes silently
wrong: `Concat::map_offset` finds the piece, then the `Original` arm computes
`start_offset + offset` with **no clamp to `end_offset`**
(quarto-source-map 0.1.3, `mapping.rs:29`), so an offset near the block's end
resolves past it into the next block's source text.

On the masked clone only, give each changed block `Generated` source_info with
`by.kind = "nested-cell-mask"` (pinned here, normatively — the mask's own
kebab-case kind, not left to whatever string the implementation happens to
pick) and an `Other("nested-cell-mask/origin")` anchor. `map_offset` on
`Generated` returns `None` (`mapping.rs:74`) — *location unknown* rather than
a confident wrong answer — and `build_source_map` already tolerates that
(`.and_then`, emits `source: None`). `Other` rather than `Invocation` is
deliberate:
`preimage_in`'s `Generated` arm walks only `Invocation`
(`source_info.rs:500`), so the anchor is provably inert to any byte-copying
writer. That inertness means no test can observe the anchor *choice* except by
inspecting the `SourceInfo` directly.

The writer's piece loop is `for block in &pandoc.blocks` — top-level only — so
a masked block nested in a list or div must also mark its **top-level
ancestor**, or the drift returns. Cost, accepted: that container reads as
location-unknown, including unrelated prose in it. Unknown beats wrong.

The marking lives only on the short-lived clone; the reparsed AST gets fresh
source_info, so nothing `Generated` persists into the document.

### Reconcile — why byte-exactness is load-bearing

The seam ends `… → reparse → reconcile(ast, executed_ast)`, with `ast` the
**unmasked** original. That only works if unmask restores byte-identically. If
it drifts by one byte, reconcile treats the display block as *changed* and
**replaces** it, so it inherits the `.rmarkdown` intermediate's source_info —
quietly undoing the provenance work above and attributing the author's example
to a temp file. Phase 4 asserts the block lands in `blocks_kept` and keeps its
original source_info, not merely that its text looks right.

### The engine capture — asymmetric, by necessity

The capture at `engine_execution.rs:534` records `input_qmd` (the masked
`qmd`) and the engine result. `ReplayEngine` (`replay.rs:129`) byte-compares
its **live** input against `input_qmd`, and the live input is masked — so
`input_qmd` must stay **masked**. `CaptureSpliceStage` splices
`result.markdown` into a live AST for `q2 preview`, so that must be
**unmasked**, or preview renders the marker.

So: unmask `result.markdown` before the capture emit; leave `input_qmd`
masked.

**`compute_input_qmd` (`preview_record.rs:211`) has three consumers, and they
do not all want the same thing:**

| consumer | role | treatment |
| --- | --- | --- |
| `quarto-preview/src/capture_driver.rs:283` | the real staleness compare (`current_input_qmd != recorded_input_qmd`) | **masked** — else every affected document reads perpetually stale |
| `quarto-preview/src/cache.rs:162` | capture cache key | **masked**; one-time invalidation of existing entries for affected docs, benign |
| `quarto-hub-provider/src/execute.rs:392` | `write_review_file` — *"the artifact the operator reviews before consenting"* | **unmasked** |

The third is security-facing: an operator deciding whether to permit execution
must see the `{r}` they wrote, not `{.r q2-nested-executable}`. **Decision:
`compute_input_qmd` masks; `write_review_file` unmasks before writing.**

Its pinned invariant test `compute_input_qmd_matches_capture_input_qmd`
(`preview_record.rs:494`) uses a fixture with **no display block** (verified),
so it would stay green through a regression here — Phase 4 adds a fixture that
has one.

## Documentation

`docs/guides/authoring/computations.qmd` already exists (35 lines, body
`TBD.`) and already asserts the behaviour this bug breaks: *"the inner cell is
shown exactly as written (single braces and all) and is not executed."* That
claim is false today whenever the page has a live cell. This PR expands the
page and makes the claim true.

**The page gets no live executable cell**, on evidence:

- `docs/` is rendered in CI in exactly one place — `release.yml:209`,
  `cargo xtask build-agents-docs` → `cargo run --bin q2 -- render docs`.
- **No workflow in the repo installs R, Python, Jupyter or Julia.**
- An unavailable engine is a hard error, not a fallback:
  `ExecutionError::runtime_not_found` propagates through
  `engine_execution.rs:495` via `?`.

So a live cell would fail the **release** build, not a PR check — surfacing
late, and `release.yml:207` records that this has already cost a release
(*"`q2 render docs` fails hard without it (v0.27.0 attempt 1)"*). No page in
`docs/` executes code today, and this PR keeps it that way.

The live-cell case is covered by the regression tests instead, gated on knitr
availability. Docs prove the feature to a human; tests prove it to CI.

A scratch draft of the expanded page is at `.scratch/docs-draft/`.

## Known limitation

The unmask pattern-matches, so an author who writes `q2-nested-executable`
verbatim inside a display block gets it rewritten. Measured, negligible in
practice, and removed later in the epic when the assembler restores from
retained bytes rather than by matching. Documented, not fixed here.

## Forward-compatibility

Every mechanism here is replaced rather than contradicted by later epic work:
the reversible-edit restore becomes a minted-token lookup; the `Generated`
provenance becomes exact `Patched` provenance; applying the mask to all
engines narrows to knitr once our own partitioners are nesting-correct. The
fixture suite below is what those later plans get measured against.

## Test seam spec (frozen)

Bound before dispatch. Every row names the production hunk whose revert turns
that assertion RED. A row without one is theatre. Once a test is green, its
assertions and harness are frozen — never edited to go green.

### Production hunks

| id | hunk |
| --- | --- |
| H1 | `mask` rewrites an opener: `{r}` → `{.r q2-nested-executable}` |
| H2 | the in-scope predicate: classes empty or `["markdown"]`, and `engine_cell_lang(block).is_none()` |
| H3 | `unmask` restores an opener carrying the marker |
| H4 | `unmask` is prefix-tolerant (not `^`-anchored), so `> ` fences restore |
| H5 | `unmask` replays whitespace/backtick-width exactly |
| H6 | changed blocks get `Generated` + `Other("nested-cell-mask/origin")` |
| H7 | the top-level ancestor of a changed *nested* block is also marked |
| H8 | mask is applied **inside** the per-engine loop, before `serialize_ast_to_qmd` |
| H9 | `unmask(result.markdown)` runs **before** the capture emit |
| H10 | `compute_input_qmd` masks |
| H11 | `write_review_file` unmasks before writing |
| H12 | `MASK_OPENER_RE` carries the regex `R` (CRLF) flag, so a `\r\n`-terminated opener line still matches `$` |
| H13 | `mask_table` recurses into `table.caption.long`, mirroring the `Figure` arm |

### Tiers

- **U — unit**, in `nested_cell_mask.rs`. Pure functions over `Pandoc`/`&str`. No engine, no writer.
- **W — writer**, in-crate. Real `pampa::writers::qmd::write_with_source_info` over a real AST. No engine.
- **R — render**, `tests/integration/`. Real `render_document_to_file`, real knitr. Gated on Rscript **and** the knitr package (`marimo_engine_e2e.rs:76-94`), not `KnitrEngine::is_available()`, which checks only the binary.
- **C — capture**, in-crate. Real `compute_input_qmd` / `record_capture` with `PassthroughTestEngine` (`preview_record.rs:265`). No external runtime.

### The runtime-only marker rule

**Every render-tier fixture must make its executed marker impossible to satisfy
by an echo**, following `assert_fence_rendered`'s existing trick: the source
says `cat(paste0("DISPLAY", "-RAN"))`, so the string `DISPLAY-RAN` exists in
the HTML **only if the cell actually ran**. Counting occurrences of a literal
that appears in both source and output is fragile and, in one case below,
outright vacuous.

### Seam table

| # | tier | unit mounted | seam: input → assertion surface | revert → RED |
| --- | --- | --- | --- | --- |
| T1 | R | full render | live `{r}` + ` ````markdown ` displaying `{r}` → HTML | **H1** → `DISPLAY-RAN` present → RED |
| T2 | R | full render | same → HTML | **H3** → display `<pre>` shows `{.r q2-nested-executable}` → RED |
| T3 | R | full render | same → HTML | **H2** (widen to all blocks) → `REAL-RAN` absent, real cell masked → RED |
| T4 | R | full render | `echo=FALSE` display block → HTML | **H1** → `OPTS-RAN` present → RED. *See vacuity note 1* |
| T5 | R | full render | live `{r}` + displayed `{python}` → HTML | **H1** → render errors (`python_not_found`) → RED |
| T6 | R | full render | blockquoted display block → HTML | **H4** → block renders empty, two `.cell` divs → RED |
| T7 | R | full render | ` ``` {r} ` and ` ```{r}   ` → HTML | **H1** → `WS-RAN` present → RED |
| T8 | R | full render | bare fenced display block, no info string (not literally 4-space-indented — see Scope's correction: qmd has no indented-code-block grammar production, so this is the only way to reach the empty-classes AST shape) → HTML | **H1** → `IND-RAN` present → RED |
| T9 | U | `mask`+`unmask` | every shape → `unmask(mask(x)) == x` bytes | **H5** → whitespace/wide-fence rows → RED |
| T10 | U | `mask` | `{{python}}` in a display block → block text | **H1** (widen pattern past `[A-Za-z0-9_]`) → RED |
| T11 | U | `unmask` | author's `{.r}` in a display block → block text | **H3** (drop marker requirement) → RED |
| T12 | U | `mask` | `{=markdown}` **RawBlock** containing `{r}` → unchanged | **H2** (extend to RawBlock) → RED. *Guards the spike's measured downgrade* |
| T13 | U | `mask` | `{r}` nested inside a real `{r}` cell → unchanged | **H2** (drop the classes-empty-or-`["markdown"]` conjunct) → RED |
| T14 | U | `mask` | masked text → `parse_code_blocks` yields 0 cells | **H1** → RED. *`parse_code_blocks` is private (`text_execute.rs:124`) — put this test in that module or expose it; decide and note which* |
| T15 | W | writer | masked AST → `map_offset` into the masked block | **H6** → returns `Some(wrong)` not `None` → RED |
| T16 | W | writer | same → `map_offset` into an *unmasked sibling* | **H6** over-applied (mark all) → RED |
| T17 | W | writer | display block nested in a `Div` → ancestor's `SourceInfo` | **H7** → ancestor stays `Original` → RED |
| T18 | W | writer | doc with a highlight block whose body embeds a fence-opener-shaped substring + a real cell, **no** nested fence → piece `(offset_in_concat, length)` pairs identical to unmasked run | **H2** (mask everything) → RED. *See vacuity note 2 (corrected)* |
| T19 | R | full render | display block → reconcile result | **H5** → block drifts, lands in replaced not `blocks_kept`, inherits intermediate `source_info` → RED |
| T20 | C | capture | doc **with** a display block → `compute_input_qmd` bytes == `capture.input_qmd` | **H10** → RED. *Existing invariant fixture has no display block, so it stays green either way* |
| T21 | C | `write_review_file` | doc with a display block → review file contents | **H11** → contains the marker, not `{r}` → RED |
| T22 | C | two engines | `FixtureEngine` ×2 in sequence → second engine's received input | **H8** (hoist mask out of the loop) → second input unmasked → RED |
| T23 | U | `mask`+`unmask` | `\r\n`-terminated display block → `unmask(mask(x)) == x` bytes | **H12** (drop the `R` flag) → `mask` reports no change → RED. *Found in task 5 review (round 1): CRLF reaches the pipeline unnormalized, and plain `(?m)`'s `$` never matches before a bare `\r`, so masking silently became a no-op on any CRLF checkout* |
| T24 | U | `mask` | display block nested in a table's long-form caption → block text carries the marker | **H13** (drop the `table.caption.long` walk) → RED. *Found in task 5 review (round 1): `mask_table` walked head/bodies/foot but not the table's own `Caption`, unlike the `Figure` arm which does walk its caption* |

### Vacuity notes

**1. The `echo=FALSE` discriminator collapses if you count.** Before the fix
the marker appears once (output only, source consumed); after the fix it
appears once (source only, no output). **A count assertion passes in both
states.** T4 must therefore assert on two surfaces that genuinely differ:
`!html.contains("OPTS-RAN")` — the runtime-only string — **and**
`html.contains("paste0(\"OPTS\"")` — the author's source, absent before the
fix because knitr consumed it.

**2. A "no nested fences" document is vacuous if scanning an out-of-scope
block would find nothing to rewrite — not merely if the document has no code
at all.** The original wording of this note was itself wrong, caught in task
3's review round: a bare, empty-bodied ` ```python ` highlight block gives an
over-broad predicate (H2 reverted to "mask everything") *nothing to change*
even once it wrongly scans that block, so the piece pairs stay identical
either way and the test would pass under the revert too — vacuous, just one
level deeper than "no code at all". The fixture must instead embed a
fence-opener-shaped substring (` ```{r} `) *inside* the highlight block's own
body text, via a wider outer fence (the same technique T13 uses to embed a
literal-looking opener inside a real cell's source without it being mistaken
for a fence boundary). Under a correct H2 the highlight block's classes keep
it out of scope, so the embedded substring is never touched and the pieces
match; under H2 reverted to "mask everything" the block *is* scanned, the
embedded opener gets rewritten, and the pieces diverge — a real RED. The
fixture also keeps a real `{r}` cell as the second block, covering both
out-of-scope class shapes (a non-markdown highlight class, and a braced
executable cell).

**3. T5 is environment-sensitive.** "The render succeeds" passes before the
fix on any machine that has Python. Assert on content instead: the display
`<pre>` contains the literal ` ```{python} ` and the HTML contains no
`.cell-output` inside it. That discriminates whether or not Python is
installed.

### Accepted untested, with rationale

- **The marker collision** (an author writes `q2-nested-executable` verbatim in
  a display block and it is rewritten). Deliberate known limitation; the
  assembler removes the class later. A test would pin behaviour we intend to
  change. *Not* silently omitted — recorded here.
- **`{=html}`-style `{=lang}` openers nested in a display block.** Only
  `breakQuartoMd` matches `=`, and it mis-partitions rather than executes; no
  in-tree consumer exercises it.
- **Capture cache-key invalidation** (`cache.rs:162`). A one-time
  recomputation for affected documents, benign and self-healing; asserting on
  a hash adds coupling without protecting a behaviour.
- **Mask idempotence.** Structurally unreachable — `mask` runs on the AST,
  which is never masked between loop iterations. T22 guards the ordering that
  makes this true.
- **H2's `engine_cell_lang` conjunct, as an independent discriminator.**
  `engine_cell_lang` (`capture_splice.rs:86-100`) returns `Some` only when a
  block carries a brace-shaped class (`{lang}`). H2's *other* conjunct admits
  only blocks whose classes are empty or exactly `["markdown"]` — neither
  shape can contain a brace-shaped class. So for every block that passes the
  classes conjunct, `engine_cell_lang(block) == None` **necessarily**: no
  fixture, parser-built or hand-built, can make the two conjuncts disagree.
  T13 still guards the real property ("mask must never touch a genuine
  executable cell"), bound to the classes conjunct instead (revert-bound hunk
  above). The `engine_cell_lang` check is retained deliberately — see the
  scope note in `nested_cell_mask.rs`'s module doc — because it becomes
  load-bearing the moment the display-class predicate widens (e.g. to
  ` ```qmd `, flagged above as a separate decision), not because it does
  anything today.

## Phases

Per-task gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` and
`cargo nextest run -p quarto-core`. Phase boundary: `cargo nextest run
--workspace`, reported against the live baseline.

### Phase 0 — Don't measure the wrong binary

- [x] **`cargo build --bin q2` from a clean tree at this branch's HEAD before measuring anything.** `target/` in this worktree has held binaries built from a spike branch that already contains the fix; measuring without rebuilding records the fix as the baseline. Confirm `.scratch/nested/r1.qmd` gives `DISPLAY-BLOCK-RAN` = **2**.
- [x] Investigate the QNR risk before committing to the top-level-ancestor rule: `build_source_map`'s doc warns that an all-unmappable input makes the Julia engine send an empty `sourceRanges`, crashing QuartoNotebookRunner. A document that is one top-level Div containing one display block would mark its only body piece `Generated`. Determine whether that can actually reach QNR; record the finding here and mitigate only if it can.

**Phase 0 findings (2026-09-02).**

*Baseline.* Rebuilt at branch HEAD `a6374f6f4` before measuring. `.scratch/nested/r1.qmd`
gives `REAL-CELL-RAN` = 2, `DISPLAY-BLOCK-RAN` = **2**, `DISPLAY-OPTS-RAN` = 1 — the plan's
claimed 2/2/1, independently reproduced. `pyfail.qmd` hard-fails with `python_not_found`
(note: python3 3.14.2 *is* installed on this machine; reticulate's own discovery misses it via
a stale uv build-cache path — same observable failure, premise intact). `bq.qmd` emits two
`.cell` divs with the display blockquote empty. Live workspace baseline for later phase
boundaries: **13550 passed, 199 skipped, 0 failed** (119.3s).

*QNR risk: **REACHABLE**.* `build_source_map` (`engine/ts_engine.rs:671`) maps each line via
`ctx.source_info.map_offset(...)`; its doc comment records that an all-unmappable input makes
the Julia engine's `buildSourceRanges` send an empty `sourceRanges`, "which crashes QNR's
`compute_line_file_lookup` (`maximum` over an empty collection)". A document whose sole
top-level block is a `Div` containing both a live `{julia}` cell and a nested display block
would have its only top-level piece marked `Generated` by the H7 ancestor rule, making every
line unmappable. Unverified link: QuartoNotebookRunner is not vendored here, so
`compute_line_file_lookup` was not read directly; the in-repo doc comment is the authority.

*Decision (Gordon, 2026-09-02): **ship the ancestor rule as planned and defend that invariant
separately**.* It is `build_source_map`'s own documented contract, not the mask's job. Filed as
**bd-quydz82t**. Phase 3 therefore implements H7 unchanged — no guard in `build_source_map`,
no degenerate-case special-casing. A mitigation that was considered and rejected: anchoring
the first entry at file offset 0 when every entry is unmappable, which fixes the class but
emits a location that is not true, contradicting the "unknown beats wrong" principle the rest
of the provenance work is built on.

### Phase 1 — Failing tests (RED)

Implement the frozen **Test seam spec** above, T1–T22. Do not invent a harness:
each row names its tier, its mounted unit, its assertion surface and the hunk
whose revert reddens it.

Create `nested_cell_mask.rs` with `unimplemented!()` stubs **first**, so the
unit tier compiles and genuinely goes red. A test that fails to *compile* is
not a RED.

- [x] Render tier T1–T8 — inline fixtures + `TempDir` through `render_document_to_file`, following `knitr_display_fence.rs`; runtime-only markers per the spec
- [x] Unit tier T9–T14
- [x] Writer tier T15–T18
- [x] Capture tier T19–T22
- [x] Render-tier control (not revert-bound, logged as characterization): a display block in a document with **no** live cell renders unchanged, pinning "reachable only when the engine is live"
- [x] Confirm every render/writer/capture test fails, and every unit test compiles and fails — **except T3 and the render-tier control, which are green-at-baseline guards, not REDs.** T3 (real cell still executes) is bound to hunk H2 (widening the scope predicate to all blocks), which does not exist until Phase 2; the current bug only under-masks (executes displays) and never over-masks (blocks the real cell), so T3's assertion is already true with zero masking in place — structurally the same as the control (a guard against a regression the fix could introduce, not a symptom of the bug being fixed). Both are re-checked, still green, at the Phase 4 all-green gate.

**Phase 1 findings (2026-09-02).** 22 seam rows written across four tasks; 26 tests RED in
`quarto-core` and 1 in `quarto-hub-provider`, none skipped, no previously-green test regressed.
`cargo build --workspace` clean at the phase boundary. No workspace *test* run: the suite is
red by design until Phase 4, so its pass count carries no signal yet — the compile check is
what guards downstream crates here.

*T19 moved tiers.* The row above originally read "Render tier T1–T8, T19". T19 asserts the
display block survives reconcile with its original `FileId`, and `reconciliation_plan` is a
stage-local at `engine_execution.rs:699` that is never exposed on the stage's output — so
`blocks_kept` is unobservable from a render. T19 is therefore an in-crate stage test, gated on
Rscript + knitr, landed with the capture tier.

*Three seam rows were vacuous as specified, and all three are now corrected in this document.*
This is worth recording because the pattern is consistent: the plan reasoned correctly about
what each test should *mean*, and slipped when reasoning about what it would *do* against an
implementation that does not exist yet.

- **T8** named a 4-space indented display block. qmd has no indented-code-block production at
  all, so the fixture was unconstructible — see the Scope correction above.
- **T18** followed vacuity note 2, whose own wording was wrong: a bare ` ```python ` block gives
  an over-broad predicate nothing to rewrite, so the test passed with the predicate correct
  *or* broken. Note 2 is rewritten above.
- **T20 and T21** were vacuously green at baseline: with nothing masking yet,
  `compute_input_qmd`'s bytes and `capture.input_qmd` are identical unmasked bytes, so the
  equality assertion held for the wrong reason — likewise "the review file shows `{r}`, not the
  marker". Each test keeps its literal invariant assertion (which discriminates its hunk once
  the transform lands) and gained a second, currently-false assertion so it is genuinely RED
  now. Verified by review to hold in both directions.

*Baseline for Phase 2.* `quarto-core` 4327 run / 4301 passed / 26 failed / 31 skipped;
`quarto-hub-provider` 40 run / 39 passed / 1 failed / 4 skipped.

### Phase 2 — The transform

- [x] `crates/quarto-core/src/engine/nested_cell_mask.rs`. Pin the API in the module doc: `mask(&mut Pandoc) -> Vec<usize>` (indices of top-level blocks that changed) and `unmask(&str) -> String`
- [x] Mask rewrites openers in in-scope blocks; unmask is prefix-tolerant and not `^`-anchored
- [x] Walker recurses containers (Div, lists, blockquotes, table cells, table captions, Figure captions, NoteDefinitionFencedBlock, Custom slots)
- [x] Walker maps each changed nested block to its top-level ancestor index
- [x] Phase 1 unit tests green

**Phase 2 findings (2026-09-02).** Implemented as two `LazyLock<Regex>`s (`MASK_OPENER_RE`,
`UNMASK_OPENER_RE`) plus a recursive container walker. Because neither regex ever touches
anything outside the `{...}` opener span, byte-exactness (leading/trailing whitespace,
inter-token spacing, arbitrary backtick width) fell out of the design for free — only the
blockquote asymmetry (H4: `unmask` must not be `^`-anchored, since it runs on the writer's
`> `-reprefixed output while `mask` only ever sees the reader's already-stripped text) needed
deliberate handling.

Review round 1 found two gaps invisible to the frozen T9–T22 suite (added as H12/H13, tested as
T23/T24 above): `MASK_OPENER_RE` lacked the regex `R` (CRLF) flag, so a `\r\n`-terminated opener
line's trailing `\r` was never consumed by `[ \t]*$` under plain `(?m)` — `mask` silently became
a no-op on any CRLF checkout, corrupting nothing and failing no frozen test (since
`unmask(mask(x)) == x` holds trivially when neither function did anything). And `mask_table`
recursed into head/bodies/foot rows but not `table.caption.long`, even though the `Figure` arm
thirty lines above already walked its own (same-typed) caption — an oversight, not a scope
decision. Both fixed; T23/T24 added to close the gap. No existing test was edited.

Verified (`nested_cell_mask.rs` only touched; `cargo clippy -p quarto-core --all-targets -- -D
warnings` clean): `quarto-core` 4329 run / 4317 passed / 12 failed / 31 skipped — the 12
failures are exactly T15, T17 (writer tier, need Phase 3 provenance) and T1, T2, T4–T8, T19, T20,
T22 (render/capture tier, need Phase 4 wiring). T3 and the render-tier control (over-masking
guards) confirmed still green.

### Phase 3 — Provenance

Phase 1 cannot pin this — a correct-looking render is achievable with the
provenance wrong, so an implementer who skips this phase gets a green suite
and ships the drift. These tests are the only thing guarding it.

- [ ] Changed blocks, and the top-level ancestor of a changed nested block, get `Generated` + the `Other("nested-cell-mask/origin")` anchor
- [ ] Test: `map_offset` into a masked block returns `None`; an unmasked sibling in the same document still resolves to its true offset
- [ ] Test: a document with no nested fences yields a `SourceInfo::Concat` whose pieces are identical to an unmasked run (compare piece `offset_in_concat`/`length` pairs)

### Phase 4 — Wire the seam

- [ ] Mask the clone inside the per-engine loop, before `serialize_ast_to_qmd`; unmask `result.markdown` before the capture emit
- [ ] `compute_input_qmd` masks; `write_review_file` unmasks before writing the consent artifact
- [ ] Test: `compute_input_qmd` bytes equal `capture.input_qmd` for a document **containing a display block** — the gap in the existing invariant fixture
- [ ] Test: `write_review_file`'s output contains `{r}` and not the marker
- [ ] Test: the display block lands in `blocks_kept` and retains its original source_info (not the intermediate's)
- [ ] Test: two engines in sequence both receive masked input
- [ ] Phase 1 render-level tests green

### Phase 5 — Documentation

- [ ] Expand `docs/guides/authoring/computations.qmd`: the rule, the nesting requirement, cell options, deeper nesting, and the existing Quarto 1 migration callout
- [ ] No live executable cell on the page (see **Documentation** above)
- [ ] `cargo run --bin q2 -- render docs/` succeeds and the page's own displayed examples render verbatim
- [ ] Check the page against `cargo xtask lint` (error-docs rules touch `docs/`)

### Phase 6 — End-to-end verification

- [ ] `q2 render` each fixture through the real binary; inspect the HTML; record the exact invocations and observed output in this plan, per CLAUDE.md
- [ ] Confirm a real cell still executes and highlights in the same document

### Phase 7 — Wrap up

- [ ] `cargo nextest run --workspace`, delta accounted for against the live baseline
- [ ] `cargo xtask verify --skip-hub-build --skip-hub-tests`
- [ ] Reconcile this checklist against what landed; commit
- [ ] Comment the outcome on the strand
