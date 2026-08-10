# `{{< include >}}` inside a fenced code block is not expanded (bd-include-in-code-block-f8mvtczn)

**Date:** 2026-08-10
**Braid:** `bd-include-in-code-block-f8mvtczn` (bug, P1, label `parity`)
**Branch:** `main` @ `bcdbce6b` — investigated in place, no worktree created
**Status:** Design settled (2026-08-10) — see **Design decisions** below. One decision (D4, trailing newline) carries a recommendation that **reverses the user's initial lean** on new evidence and needs a yes/no before Phase 1. Everything else is pinned.

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

### D3 — Recursion: none inside a code fence ✅ settled, and it is a deliberate divergence

A file included into a fence is code, not qmd; its contents are spliced and not re-scanned.

**Q1 does recurse**, so this is a divergence, not parity. `standaloneInclude` (`src/core/handlers/include-standalone.ts`) is the *same* code path for both positions, and its `retrieveInclude` re-scans the included text line-by-line for further block-shortcode includes and recurses.

Where the divergence is observable: including a `.qmd` into a fence in order to *show* its source. Q1 would expand include lines inside that snippet; we would print them literally. Printing them literally is arguably the better behavior for a listing — but it is a difference, and it should be documented (that is bd-cq0xhxg5).

For non-qmd targets the divergence is unreachable in practice: a bare `{{< include … >}}` line in a `.py`/`.R`/`.json` file is a syntax error in that language.

**Cycle stack:** with recursion off, a fence include cannot start a cycle, so pushing onto `include_stack` is not needed for correctness. Recommend pushing anyway for uniformity and cheap future-proofing if recursion is ever enabled — decide at implementation time; it is not load-bearing.

Q1 trivia worth not copying: its cycle detection checks the *resolved* path (`retrievedFiles.indexOf(path)`) but pushes the *raw* filename (`retrievedFiles.push(filename)`), so it only catches cycles when the two coincide. Our existing `canonicalize`-based `include_stack` is already correct; don't regress toward Q1 here.

### D4 — Trailing newline: ⚠️ recommend trimming exactly one — reverses the initial lean

The initial lean was "splice verbatim". The evidence says trimming exactly one trailing newline is what reproduces Q1's *rendered output*, and that verbatim would regress the entire motivating corpus. Detail:

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

**Recommendation:** trim exactly one trailing `\n` (and the `\r` of a preceding `\r\n`) from the spliced bytes. Equivalent framing, possibly cleaner to implement: splice verbatim, then normalize the resulting `code_block.text` to the parser's own convention. Needs a yes/no before Phase 1.

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

## Proposed phases

Now grounded in the settled decisions. Contents are still draft at the work-item level.

- **Phase 0 — Test plan (TDD, failing first).**
  - Unit: a line-strict code-fence include is recognized; mid-line and multi-per-line occurrences are **not** (D2).
  - Unit: target resolution matches `resolve_include_target` — relative to the declaring file, leading `/` project-root-relative.
  - Unit: an indented include line splices without re-indentation (D2).
  - Unit: `shortcodes="false"` opts out (D5).
  - Unit: trailing-newline handling per D4, once decided.
  - Integration: the fence body equals the file's content; no `Q-17-4` (D6).
  - Integration: dependency recorded — the target appears in `DocumentProfile.includes` and in the preview dep set.
  - Integration: an included `.qmd` inside a fence shows its own include lines **literally** (D3, the deliberate divergence).
  - Smoke-all fixture `includes/in-code-fence/` — note `includes/code-cell/` already exists and is a *different* shape (a file containing a cell, included at top level); do not conflate.
  - Rewrite (do not delete) `extract_include_path_from_non_paragraph` so the new contract stays pinned.
- **Phase 1 — Line-strict recognizer + textual splice in `IncludeExpansionStage`** (D1, D2): scan `code_block.text` line-wise, splice the target's content in place, no parsing, no re-indentation, per-D4 trailing-newline handling.
- **Phase 2 — Dependency tracking**: `record_include` for the fence target; extend `collect_include_paths` through a **shared helper** so the preview dep-graph cannot drift (see Risks).
- **Phase 3 — Opt-out + recursion**: `shortcodes="false"` wins (D5); no recursion (D3); decide the `include_stack` push at implementation time.
- **Phase 4 — Diagnostics**: confirm `Q-17-4` no longer fires for this shape (should fall out of D1 with no diagnostic change); update the four test sites that assert today's behavior.
- **Phase 5 — Docs**: edit `docs/errors/include/Q-17-4.qmd` ("sole content of its own paragraph" is no longer the whole story). The larger authoring-docs gap is tracked separately as **bd-cq0xhxg5**.
- **Phase 6 — Verify against the real corpus**: re-render the Connect-docs set and confirm the ~44 listings come back; diff against a `quarto render` of the same tree.

## Remaining question for the user

**D4 only** — trailing newline. Everything else is settled. The recommendation (trim exactly one) reverses the initial lean toward verbatim, on the evidence recorded in D4: Q1's rendered output has no trailing blank line, q2 has no normalizing re-read, and a verbatim splice would add a spurious blank final line to essentially every listing.

## Follow-ups filed

- **bd-cq0xhxg5** (docs, P2, `discovered-from` this strand) — document the two include expansion mechanisms: AST-splice at block positions vs textual splice inside code fences. The audit found `include` is absent from the built-ins table in `docs/guides/authoring/shortcodes.qmd`, absent from its "Where shortcodes are evaluated" section, and that there is no includes guide page at all — includes are documented only through the four `Q-17-*` error pages. Write it once this strand lands, so the docs describe shipped behavior.

## Risks / tradeoffs (draft)

- **Cross-walker drift.** The expander and the preview dep collector share `child_block_lists_mut` precisely so they cannot diverge. A code-fence include is a *new kind* of position that does not fit that accessor's shape (it is not a block list). Adding it to both walkers without a shared helper reintroduces exactly the drift the current design prevents. Design the shared helper first.
- **Invariant preserved, not broken.** `shortcode_resolve.rs:623-629` documents "any `include` still present here is inline among other content, the one unsupported position." D1's site keeps that sentence true (the transform-site alternative would have falsified it) — a point in D1's favor, now settled.
- **Test churn is modest but load-bearing.** Four test sites plus a docs page assert today's behavior. None of them are snapshots, so the changes are explicit and reviewable — good. The pinning unit test `extract_include_path_from_non_paragraph` should be *rewritten*, not deleted, so the new contract stays pinned.
- **A deliberate Q1 divergence ships with this** (D3, no recursion inside a fence). It is defensible and arguably better for listings, but it is a parity gap on a strand labeled `parity`. bd-cq0xhxg5 is where it gets explained to users; make sure that lands rather than being dropped once the code works.
- **Q1 comparison is now done, at one data point.** `quarto render` of the repro was run against `external-sources/quarto-cli` @ `abc6a78ed`; source was read for the recursion, opt-out and newline logic. Not yet compared: multi-include fences, indented includes, `.qmd`-into-fence, and the full Connect-docs corpus. Phase 6 should widen this before the work is called done.
- **No incoming dependencies** means nothing else in this skein breaks whichever way we go — the risk is confined to include semantics.
