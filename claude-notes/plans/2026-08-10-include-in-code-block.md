# `{{< include >}}` inside a fenced code block is not expanded (bd-include-in-code-block-f8mvtczn)

**Date:** 2026-08-10
**Braid:** `bd-include-in-code-block-f8mvtczn` (bug, P1, label `parity`)
**Branch:** `braid/include-in-code-block-f8mvtczn`, off `main` @ `bcdbce6b`
**Status:** Implemented, fully verified, **awaiting review before commit**. All decisions (D1–D7 plus D3a/D3b/D4a found during implementation) are settled and recorded below.

## Triage verdict

**Ready to design** — the bug reproduces byte-for-byte at HEAD, the mechanism is fully understood, and there is a clean fix site; but the investigation found that the strand's own **suggested fix direction lands in the wrong pipeline stage**, so the location question had to be settled before implementation started. It now is (D1).

## Design decisions

Settled with the user on 2026-08-10. Each records what Q1 actually does, verified against `external-sources/quarto-cli` @ `abc6a78ed` and a real `quarto render` of the repro.

### D1 — Fix site: `IncludeExpansionStage` ✅ settled

The fix goes in `IncludeExpansionStage` (stage 3), not `ShortcodeResolveTransform` (stage 13). Rationale in "The finding that changes the fix site" below: the transform runs after the `DocumentProfileStage` checkpoint and so cannot register the include as a dependency without violating the read-only-profile contract.

The stage already walks the AST regardless, so recognizing a code-fence include there is a small parser plus a new per-block action — no new traversal.

### D2 — Recognition: strict, one shortcode alone on its own line ✅ settled

v1 recognizes an include **only** when the shortcode is the sole content of a line within `code_block.text` (leading/trailing whitespace permitted). Mid-line and multiple-per-line occurrences are not recognized. The permissive rule stays available as a future widening; a strict rule can be relaxed later, a permissive one cannot be tightened.

**This matches Q1 exactly** — a finding that emerged after the decision was made and confirms it. Q1's `processMarkdownIncludes` (`src/core/handlers/base.ts:355-380`) splits each cell into lines and tests each with `isBlockShortcode`, whose regex is anchored to the whole line:

```js
content.match(/^\s*{{< (?!\/\*)(.+?)(?<!\*\/) >}}\s*$/)
```

So "Q1 substitutes textually anywhere in the code text" — as the strand's framing implied — is **wrong**. Q1 is line-strict. Strict is parity, not a divergence.

Note the `^\s*` — Q1 accepts an *indented* include line and splices without re-indenting. We should match that.

### D3 — Recursion: recurse, matching Q1 ✅ settled (reversed 2026-08-10)

**Originally settled as "no recursion"; reversed during implementation** once D3a (below) showed that "no recursion" could not deliver what it promised. Spliced text **is** re-scanned for include lines, exactly as Q1's `standaloneInclude` (`src/core/handlers/include-standalone.ts`) does.

Consequences:

- A `.qmd` embedded as a listing shows the same content it would show as a page. Authors who want the *literal* source, shortcodes and all, use `shortcodes="false"` — the documented mechanism for exactly that.
- Cycles are real and must be caught: a document that embeds itself in a listing carries the same include line in the spliced copy, so it would recurse forever. Handled with the existing `include_stack` (canonicalized paths) and a `Q-17-1` warning; the offending line is dropped.
- **Nested paths anchor at the including file's directory**, matching how block-position includes nest (bd-1fz3vh99). Q1 anchors nested fence includes at the *document* instead (`standaloneInclude` resolves against `target.source` at every level). Ours is the more consistent rule and the one q2 already documents; a deliberate, narrow divergence.
- The parity gap this decision used to introduce is gone, which shrinks what bd-cq0xhxg5 has to explain.

Q1 trivia worth not copying: its cycle detection checks the *resolved* path (`retrievedFiles.indexOf(path)`) but pushes the *raw* filename (`retrievedFiles.push(filename)`), so it only catches cycles when the two coincide. Our `canonicalize`-based `include_stack` is already correct; don't regress toward Q1 here.

### D3a — Why D3 was reversed ✅ resolved

**Found during implementation (2026-08-10), verified end-to-end.** D3 says a file included into a fence has its own include lines "printed literally". That does not happen — and cannot, without an additional change.

The spliced text still contains `{{< include inner.qmd >}}`. That text reaches `ShortcodeResolveTransform` at stage 13, which does exactly what it does today for any unhandled include in code text: emits the `?include` token plus a `Q-17-4` warning. So the strand's own bug reappears one level down.

Measured, with `outer.qmd` = `top line\n{{< include inner.qmd >}}\nbottom line\n` spliced into a `.markdown` fence:

```html
<pre class="markdown code-with-copy"><code>top line
?include
bottom line</code></pre>
```

plus a `Q-17-4` whose location points at the *outer* fence — doubly misleading, since the author never wrote an include there.

Three ways out:

1. **Recurse (Q1 parity).** Re-scan spliced content for include lines, using the `include_stack` for cycles. No include survives to stage 13, so the problem vanishes by construction, and the D3 divergence disappears (shrinking bd-cq0xhxg5's scope). Cost: a listing that embeds a `.qmd` shows the *expanded* text, not the file's literal source. Users wanting literal source have `shortcodes="false"` — which is already the documented mechanism for exactly that.
2. **Preserve unhandled includes in code text.** In the code/raw text arms of `ShortcodeResolveTransform`, treat an `include` shortcode as `Preserve` (emit its source text verbatim) instead of dispatching it to the error branch. The invariant that makes this sound: after stage 3, every *authored* fence include has been expanded, so anything still present must have come from spliced content — and literal is the right rendering for it. Keeps D3's intent and makes it actually true. Cost: a new (small) rule in the transform, and the `?include` token stops existing for fences entirely.
3. **Escape includes in spliced content.** Rewrite `{{< include … >}}` to the escaped `{{{< … >}}}` form while splicing. Works, but it is a text mutation the author never wrote, and it creates an asymmetry with `{{< meta … >}}` in the same fence. Not recommended.

**Chosen: option 1 (recurse).** It is the Q1-parity answer, needs no new rule in the shortcode transform, and makes the nested `?include` impossible by construction rather than by special-casing. Pinned by `code_fence_include_recurses`, `code_fence_include_cycle_reports_and_drops_line`, `code_fence_include_of_self_reports_cycle` (unit) and `nested_code_fence_include_expands_without_token`, `self_including_code_fence_reports_a_cycle` (full pipeline).

### D3b — Unhandled includes in code text are preserved, not `?include`-ed ✅ settled

**Found by the widened Q1 comparison (2026-08-10).** Line-strict recognition (D2) means a *mid-line* include in a fence is deliberately not expanded — but the leftover shortcode then reached `ShortcodeResolveTransform` and came out as `?include`, corrupting the listing. Same failure mode as D3a, but for authored (not spliced) text, so recursion does not cover it.

Measured before the fix, for `x = 1  {{< include a.py >}}` in a `.python` fence:

| | rendered |
|---|---|
| Q1 | `x = 1  {{< include a.py >}}` (untouched, silent) |
| q2 | `x = 1  ?include` + a `Q-17-4` warning |

Fix: `expand_text_segments` takes an `UnhandledInclude` mode; the two **code** text contexts (`Block::CodeBlock`, `Inline::Code`) pass `Preserve`, which emits the shortcode's source text instead of the error marker and suppresses the diagnostic. Every other text context (raw blocks/inlines, math, attribute values, link targets, `rendered.includes.*` slots) keeps `Report`.

Why `Preserve` is sound rather than a papering-over: after stage 3, every include occupying a whole line of a non-opted-out fence has been expanded. Anything still present in code text is there because the line-strict rule declined it — so the author's literal text *is* the correct output. Q1 reaches the same result by never matching the line in the first place.

The check keys off the dispatch *result* (`Error` whose key is `include`), not the shortcode name, so a user-supplied Lua handler named `include` still takes precedence.

### D4 — Trailing newline: trim exactly one ✅ settled

The initial lean was "splice verbatim"; the evidence below reversed it, and the user accepted the reversal. **Trim exactly one trailing `\n`, plus the `\r` of a preceding `\r\n`.** Detail:

**What Q1's source does.** `standaloneInclude` appends a newline fragment after the content:

```js
textFragments.push(includeSrc.value.endsWith("\n") ? "\n" : "\n\n");
```

Either branch ends the spliced text with `\n\n`. So Q1 *adds* a blank line.

**What Q1 actually renders.** Not that. `quarto render` of the repro produces exactly three highlighted lines — `cb1-1`, `cb1-2`, `cb1-3` — with **no trailing blank line**. Q1 gets away with the appended newline because the spliced markdown is re-read by Pandoc's markdown reader, which normalizes trailing whitespace inside a fence.

**Why q2 cannot rely on that.** q2 emits HTML directly from the AST — there is no Pandoc re-read (see CLAUDE.md, "No DOM postprocessor"). Two measurements at HEAD:

- The qmd parser's convention for a fence whose last line has content is **no trailing newline**: ```` ```{.python}\nimport os\n\nprint("hi")\n``` ```` yields `CodeBlock.text == "import os\n\nprint(\"hi\")"`. A source blank line before the closing fence *does* yield a trailing `"\n"`, so the trailing newline is representable and meaningful.
- The HTML writer emits `escape_html(&codeblock.text)` verbatim (`crates/pampa/src/writers/html.rs:1478`). Rendered side by side:

  ```
  with a trailing newline:  …<span class="hl-variable">os</span>\n</code></pre>
  without:                  …<span class="hl-variable">os</span></code></pre>
  ```

  A newline immediately before `</code></pre>` is *not* stripped by the HTML spec (only one immediately after `<pre>` is), so it renders as an empty final line.

**Consequence.** Nearly every source file ends with a newline (POSIX convention; most editors and linters enforce it). Splicing verbatim would therefore give *every* listing a spurious blank last line — the default case, not an edge case — and would break parity for all ~44 Connect-docs listings, which is the reason this strand exists.

**On the "users can't force an intentional trailing newline" concern.** They can, symmetrically with how the parser already treats fences: end the file with two newlines — one is consumed by the trim, one remains. That is the same affordance a hand-written fence has.

**Decision:** trim exactly one trailing `\n` (and the `\r` of a preceding `\r\n`) from the spliced bytes.

### D4a — Consecutive includes in one fence: no blank separator (divergence) ✅ settled

Two include lines in one fence:

| | rendered |
|---|---|
| Q1 | `AAA\n\nBBB` |
| q2 | `AAA\nBBB` |

Q1's blank line is the same `standaloneInclude` artifact D4 documents — it appends `"\n"` after each spliced file because in *markdown* a blank line separates blocks. Inside a fence it is noise, and at the end of a fence Pandoc's re-read deletes it (which is why the single-include case shows none). Applying D4's endorsed principle uniformly — normalize to the parser's convention rather than inherit markdown-separator artifacts — means no separator here either. Pinned by `code_fence_include_expands_multiple_targets`.

### D5 — `.cell-code` timing: accept the splice-before-execution semantics ✅ settled

At stage 3 an authored executable cell is still ```` ```{python} ````; `.cell-code` is written later by the Jupyter engine (`crates/quarto-core/src/engine/jupyter/text_execute.rs:440`) at `EngineExecutionStage`. So an include inside an authored executable cell gets spliced into the cell's *source* before the engine runs it. This matches Q1's text-level model and is accepted.

Rationale to record in a comment at the fix site: q2's tooling and diagnostics are meant to keep executable cell source parseable as the target language (hence `#|`, `//|` for cell metadata), so a user reaching this state unintentionally should be caught upstream. Engine *output* code — the case `.cell-code` exists to protect — cannot contain an authored include, so the two never actually collide.

`shortcodes="false"` is authored and therefore visible at stage 3; it must keep winning.

Q1 quirk not to copy: Q1's opt-out test is `newCells[i].value.search(/\s*```\s*{\s*shortcodes\s*=\s*false\s*}/)`, which only matches a fence whose attributes are *exactly* `{shortcodes=false}` — ```` ```{.python shortcodes=false} ```` does **not** opt out in Q1. Our `code_shortcode_opt_out` is attribute-based and correct; keep it.

### D6 — `Q-17-4` ✅ settled by construction

Once the fence shape is expanded at stage 3, the shortcode is gone before stage 13 and `Q-17-4` stops firing for it automatically — no change to the diagnostic's condition, and the invariant documented at `shortcode_resolve.rs:623-629` stays true. The remaining firing position is inline-in-a-sentence, which the current hint already describes correctly. Keep the hint as is.

`docs/errors/include/Q-17-4.qmd` still needs a small edit: its "What this means" says expansion requires the shortcode be "the sole content of its own paragraph", which will no longer be the whole story.

### D7 — Alternatives rejected ✅ settled

A fence attribute (```` ```{.python include="app.py"} ````) and a dedicated `embed-file` shortcode are **rejected**. Both would make every existing Q1 document wrong and would require a `qmd-syntax-helper` migration rule to repair them, whereas supporting the existing spelling costs those documents nothing. Recorded here so it is not re-litigated.

Not left open in writing as a promised future affordance — if someone wants one later, they can make the case then.

## Issue context

An `include` shortcode standing alone inside a fenced code block — the standard Q1 idiom for embedding a source file as a listing —

````markdown
```{.python filename="app.py"}
{{< include app.py >}}
```
````

renders with the entire fence body replaced by the single token `?include`. Q1 splices the included file's raw text into the fence and the listing renders as a syntax-highlighted copy of `app.py`.

A `Q-17-4` "Include not expanded" warning does fire, so this is not silent — but its hint ("Put the include shortcode in its own paragraph, surrounded by blank lines") is actively wrong here: the shortcode belongs inside the fence, and following the hint would change what the page means.

**Real-world impact.** Filed against the Posit Connect docs port: 21 files, ~44 listings, ~1300 lines of embedded source — the single largest remaining content loss in that port. Every cookbook integration recipe embeds its `requirements.txt` / `app.py` / `app.R` / `manifest.json` this way, so the recipes lose the code they exist to show. Worst case is `cookbook/content/integrations/databricks/viewer/python/index.qmd` — ten such listings across five framework tabs, all showing `?include`.

Filed 2026-08-10 by Carlos Scheidegger; status `open`, priority 1, type `bug`, label `parity`. Not stale — filed today.

## Dependency graph

**Empty.** `braid dep tree` shows the strand alone; `braid dep list` returns no edges.

The strand names an origin (`br-4kslym5r`) in the **connect-docs porting skein**, which is a different braid document — so the "discovered-from" context is not reachable from this skein and is instead captured in prose in the strand description (the Connect-docs port, quantified above).

Consequence for the calculus: no incoming `blocks` pressure from other strands in this skein, so urgency comes entirely from the Connect-docs port, not from internal dependents. Nothing here is blocked on anything else.

Adjacent (not linked, but the relevant prior work): **bd-fz6gwfq0** — the 0.16.0 text-level shortcode work that made `{{< meta … >}}` expand inside code fences. That is the machinery that currently eats the include.

## What the code looks like today

Every path the strand names still exists with the shape it describes. Spot-check at `bcdbce6b`:

### Reproduces at HEAD — confirmed

```bash
cargo run --bin q2 -- render claude-notes/plans/include-in-code-block-investigation/repro
```

emits the `Q-17-4` warning at `index.qmd:8:1` and produces

```html
<pre class="sourceCode python" data-filename="app.py"><code class="sourceCode python">?<span class="hl-variable">include</span></code></pre>
```

Byte-identical to the strand's recorded symptom. The control include at top level in the same document expands correctly. Fixture + captured output: `claude-notes/plans/include-in-code-block-investigation/`.

### Half one — `IncludeExpansionStage` cannot reach a code fence

`crates/quarto-core/src/stage/stages/include_expansion.rs`:

- `extract_include_path` (`:507`) matches only `Block::Paragraph` / `Block::Plain` whose sole inline is an `include` shortcode.
- `child_block_lists_mut` (`:385`) is the documented **single source of truth** for "where can an include appear": Div, BlockQuote, Bullet/Ordered/Definition list items, Figure, `NoteDefinitionFencedBlock`, Table cells. A `CodeBlock` is a leaf (its content is a `String`), so it falls into the `_ => Vec::new()` arm and is unreachable by construction.
- The pinning test `extract_include_path_from_non_paragraph` (`:646`) asserts a CodeBlock yields `None`. Any fix revisits it.

Note that `expand_blocks` *does* visit every `CodeBlock` — as an element `blocks[i]` of some block list at any nesting depth. What it cannot do is descend *into* one. That matters: a fix at this site needs no new traversal, only a new per-block action.

### Half two — `ShortcodeResolveTransform` is what writes `?include`

`crates/quarto-core/src/transforms/shortcode_resolve.rs`:

- The `Block::CodeBlock(code_block)` arm (`:1834`) calls `expand_text_in_place` on `code_block.text` unless `code_shortcode_opt_out` (`:1509`) fires.
- `dispatch_shortcode` has no `include` handler by design; the `shortcode.name == "include"` branch (`:630`) returns `ShortcodeResult::Error` with the `Q-17-4` diagnostic.
- `expand_text_segments` (`:1489-1493`) handles `Error` by pushing `'?'` then the shortcode name — hence `?include` replacing the whole body.

So the mechanism that already makes `{{< meta … >}}` work inside a fence is the same mechanism that eats the include, and it operates on exactly the right data: raw text.

### The finding that changes the fix site — the profile checkpoint

The strand suggests fixing this **in the text-expansion path** (`ShortcodeResolveTransform`). That site cannot satisfy the strand's own dependency-tracking requirement:

| # | Stage (from `pipeline.rs:220-231`) |
|---|---|
| 3 | `IncludeExpansionStage` — records `IncludeEntry` via `record_include` |
| 4 | `IncludeResolveStage` |
| 6 | **`DocumentProfileStage`** — *drains* the include side-channel into `DocumentProfile.includes` (`document_profile.rs:422`, profile version 2, `bd-r82e`) |
| 8 | `UnwrapProfileStage` |
| 13 | `AstTransformsStage` — where `ShortcodeResolveTransform` runs (`pipeline.rs:1205`) |

By the time the transform runs, the profile is already extracted, and **profiles are read-only** per `claude-notes/designs/document-profile-contract.md` (CLAUDE.md restates the rule: a feature needing state not in the profile moves its producer *earlier*, it does not back-patch). A code-fence include registered at stage 13 could not reach `DocumentProfile.includes`, so editing `app.py` would not rebuild the page that embeds it.

Supporting facts for the alternative site (`IncludeExpansionStage`):

- It already holds everything the fix needs: `ctx.runtime.file_read`, `base_dir` (`current_file.parent()`), `ctx.project.dir`, `recorded_includes`, and the cycle-detection `include_stack`.
- `parse_text_shortcodes` (`crates/quarto-core/src/transforms/shortcode_text.rs:50`) is `pub` and reusable from the stage — the same textual parser the transform uses.
- Splicing the file text into `code_block.text` at stage 3 means the shortcode is *gone* before stage 13, so `Q-17-4` naturally stops firing for this shape without touching the diagnostic's condition. That preserves the existing invariant documented at `shortcode_resolve.rs:623-629` ("includes are expanded … before transforms run") rather than carving an exception into it.

### `code_shortcode_opt_out` has a timing wrinkle at the earlier site

`code_shortcode_opt_out` checks two things:

- `shortcodes="false"` — **authored**, so visible at stage 3. Fine.
- `.cell-code` — **engine-produced**. It is written by the Jupyter engine (`crates/quarto-core/src/engine/jupyter/text_execute.rs:440`), which runs at `EngineExecutionStage`, *after* `IncludeExpansionStage`. So no block carries `.cell-code` at stage 3.

This is not a blocker but it is a real semantic decision: at stage 3 an authored executable cell is still ```` ```{python} ````, so an include inside one would be spliced into the cell's *source* before the engine executes it. Q1's text-level model does the same thing, but q2's current opt-out contract says `.cell-code` means "print as-is". The two only collide for engine-*output* code, which cannot contain an authored include — but the reasoning should be written down, not discovered later.

The predicate itself is private to `shortcode_resolve.rs`; sharing it (rather than duplicating) is a small refactor.

### Preview dependency graph

`quarto-preview/src/deps.rs` reuses `collect_include_paths` (`include_expansion.rs:437`), which walks via the same `child_block_lists_mut` accessor and `extract_include_path`. The doc comment on `child_block_lists_mut` explicitly says the two walkers share it "so the two can never drift". A code-fence include must be added to **both** the expander and the collector, ideally through one shared helper, or the preview dep-graph silently drifts from the renderer.

### Existing tests and fixtures in the blast radius

- `crates/quarto-core/src/stage/stages/include_expansion.rs:646` — `extract_include_path_from_non_paragraph`, pins CodeBlock → `None`.
- `crates/quarto-core/tests/integration/include_expansion_diagnostics.rs:247-261` — asserts `Q-17-4` fires for the unsupported position.
- `crates/quarto-core/tests/integration/include_nested_expansion.rs:23` — asserts `Q-17-4` does *not* fire for supported nesting.
- `crates/quarto/tests/smoke-all/includes/nested/nested.qmd:12` — asserts the `Q-17-4` text appears.
- `crates/quarto/tests/smoke-all/includes/code-cell/` — **different shape**, do not confuse: it includes a *file containing* a code cell. A new fixture should be named something like `includes/in-code-fence/`.
- `docs/errors/include/Q-17-4.qmd` — user-facing error page; its "What this means" and "How to fix" both assume the only unsupported position is inline-in-a-sentence. Needs updating either way.

No snapshot anywhere currently pins the literal string `?include`.

## Work items

Branch: `braid/include-in-code-block-f8mvtczn`. D4 settled 2026-08-10: **trim exactly one trailing `\n`** (plus a preceding `\r`).

### Phase 0 — Test plan (TDD: written and failing first)

- [x] Unit: line-strict recognizer accepts a lone `{{< include x >}}` line (leading/trailing whitespace ok)
- [x] Unit: recognizer rejects mid-line, two-per-line, and non-`include` shortcodes (D2)
- [x] Unit: recognizer rejects the escaped form `{{{< include x >}}}`
- [x] Unit: trailing-newline trim — exactly one `\n`, and `\r\n` → nothing (D4)
- [x] Unit: `shortcodes="false"` opts out (D5)
- [x] Unit: indented include line splices without re-indentation (D2)
- [x] Integration: fence body equals the target's content; no `Q-17-4` (D6)
- [x] Integration: missing target → `Q-17-2`, no `?include` leakage
- [x] Integration: dependency recorded in `DocumentProfile.includes`
- [x] Integration: `.qmd` included into a fence expands recursively, no `?include` (D3, revised)
- [x] Smoke-all fixture `includes/in-code-fence/` (note: `includes/code-cell/` is a *different* shape — do not conflate)
- [x] Rewrite (do not delete) `extract_include_path_from_non_paragraph` so the new contract stays pinned

### Phase 1 — Recognizer + textual splice in `IncludeExpansionStage` (D1, D2)

- [x] Share `code_shortcode_opt_out` out of `shortcode_resolve.rs` (make `pub(crate)`)
- [x] Add the shared recognition helper (single source of truth, mirroring `child_block_lists_mut`'s role)
- [x] Splice in `expand_blocks`: read target, replace the line, no parsing, no re-indentation, D4 trim
- [x] Diagnostics on read failure, consistent with the block-position arms

### Phase 2 — Dependency tracking

- [x] `record_include` for each fence target
- [x] Extend `collect_include_paths` through the *same* helper so the preview dep-graph cannot drift
- [x] Confirm `quarto-preview`'s `extract_include_deps` picks it up

### Phase 3 — Opt-out + recursion

- [x] `shortcodes="false"` wins (D5)
- [x] Recursion with `include_stack` cycle detection + `Q-17-1` (D3, revised)

### Phase 4 — Diagnostics

- [x] Confirm `Q-17-4` no longer fires for the fence shape (should fall out of D1)
- [x] Update the four existing test sites that assert today's behavior

### Phase 5 — Docs

- [x] `docs/errors/include/Q-17-4.qmd` — "sole content of its own paragraph" is no longer the whole story
- [ ] (separate strand **bd-cq0xhxg5**) authoring-docs gap

### Phase 6 — Verification

- [x] `cargo nextest run --workspace`
- [x] `cargo xtask verify` (WASM leg: `quarto-core` is in hub-client's dependency closure) — all 14 steps green
- [x] End-to-end through the binary on the repro, output inspected (CLAUDE.md requirement)
- [x] Widen the Q1 comparison: multi-include fences, indented includes, `.qmd`-into-fence, mid-line
- [x] Re-render the Connect-docs corpus and confirm the ~44 listings come back

**Result (2026-08-10).** 44 fence includes across 20 files, matching the strand's count. Full render: `352 of 352 files`. Corpus-wide `?include` count in the rendered HTML dropped to **1**, and that one is *not* a fence — it is the raw-HTML shape the strand itself calls docs-fixable (`licenses/index.md`: an include directly under an HTML comment with no blank line, swallowed into a `RawBlock`). It still reports `Q-17-4`, and there the existing hint — "put the include in its own paragraph, surrounded by blank lines" — is exactly right. The worst-case page, `cookbook/content/integrations/databricks/viewer/python/index.qmd`, now renders all its listings including a 92-line `app.py`, with zero `?include`.

## Open questions

None. D1–D7 are settled; implementation is underway.

## Follow-ups filed

- **bd-cq0xhxg5** (docs, P2, `discovered-from` this strand) — document the two include expansion mechanisms: AST-splice at block positions vs textual splice inside code fences. The audit found `include` is absent from the built-ins table in `docs/guides/authoring/shortcodes.qmd`, absent from its "Where shortcodes are evaluated" section, and that there is no includes guide page at all — includes are documented only through the four `Q-17-*` error pages. Write it once this strand lands, so the docs describe shipped behavior.

## Risks / tradeoffs (draft)

- **Cross-walker drift.** The expander and the preview dep collector share `child_block_lists_mut` precisely so they cannot diverge. A code-fence include is a *new kind* of position that does not fit that accessor's shape (it is not a block list). Adding it to both walkers without a shared helper reintroduces exactly the drift the current design prevents. Design the shared helper first.
- **Invariant preserved, not broken.** `shortcode_resolve.rs:623-629` documents "any `include` still present here is inline among other content, the one unsupported position." D1's site keeps that sentence true (the transform-site alternative would have falsified it) — a point in D1's favor, now settled.
- **Test churn is modest but load-bearing.** Four test sites plus a docs page assert today's behavior. None of them are snapshots, so the changes are explicit and reviewable — good. The pinning unit test `extract_include_path_from_non_paragraph` should be *rewritten*, not deleted, so the new contract stays pinned.
- **A deliberate Q1 divergence ships with this** (D3, no recursion inside a fence). It is defensible and arguably better for listings, but it is a parity gap on a strand labeled `parity`. bd-cq0xhxg5 is where it gets explained to users; make sure that lands rather than being dropped once the code works.
- **Q1 comparison is now done, at one data point.** `quarto render` of the repro was run against `external-sources/quarto-cli` @ `abc6a78ed`; source was read for the recursion, opt-out and newline logic. Not yet compared: multi-include fences, indented includes, `.qmd`-into-fence, and the full Connect-docs corpus. Phase 6 should widen this before the work is called done.
- **No incoming dependencies** means nothing else in this skein breaks whichever way we go — the risk is confined to include semantics.
