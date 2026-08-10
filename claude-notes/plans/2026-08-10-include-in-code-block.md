# `{{< include >}}` inside a fenced code block is not expanded (bd-include-in-code-block-f8mvtczn)

**Date:** 2026-08-10
**Braid:** `bd-include-in-code-block-f8mvtczn` (bug, P1, label `parity`)
**Branch:** `main` @ `bcdbce6b` — investigated in place, no worktree created
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design** — the bug reproduces byte-for-byte at HEAD, the mechanism is fully understood, and there is a clean fix site; but the investigation found that the strand's own **suggested fix direction lands in the wrong pipeline stage**, so the location question has to be settled before implementation starts.

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion below.

- **Phase 0 — Test plan (TDD, failing first).**
  - Unit: a code-fence include is recognized and its target resolved (relative, and leading-`/` project-root).
  - Unit: `shortcodes="false"` still opts out.
  - Integration: the fence body equals the file's bytes; no `Q-17-4`.
  - Integration: dependency recorded — the included file appears in `DocumentProfile.includes` / the preview dep set.
  - Smoke-all fixture `includes/in-code-fence/`.
  - Revisit `extract_include_path_from_non_paragraph` and the `Q-17-4` assertions above.
- **Phase 1 — Textual splice at the chosen stage** (see Q1 below): read the target, replace the shortcode span in `code_block.text` verbatim, no parsing, no re-indentation.
- **Phase 2 — Dependency tracking**: `record_include` for the fence target; extend `collect_include_paths` (or a shared helper) so the preview dep-graph agrees.
- **Phase 3 — Opt-out + recursion semantics**: `shortcodes="false"` wins; decide the recursion/cycle rule (Q3).
- **Phase 4 — Diagnostics**: confirm `Q-17-4` stops firing for this shape; adjust the hint if it survives in any code-adjacent position.
- **Phase 5 — Docs**: `docs/errors/include/Q-17-4.qmd`, plus documenting the fence idiom wherever includes are described.
- **Phase 6 — Verify against the real corpus**: re-render the Connect-docs repro set and confirm the ~44 listings come back.

## Open design questions for the user

1. **Fix site — the central question.** The strand suggests handling `include` inside `ShortcodeResolveTransform`'s text-expansion path. The investigation argues for `IncludeExpansionStage` instead, because the transform runs *after* the `DocumentProfileStage` checkpoint and therefore cannot register the include as a dependency without violating the read-only-profile contract. Do you agree the fix belongs in `IncludeExpansionStage`? (If you prefer the transform site, we need a third answer for how dependencies get recorded — moving the profile checkpoint is the only route I see, and that is a much larger change.)

2. **Scope of recognition inside a fence.** Q1 substitutes textually anywhere in the code text. Do we match that (any `{{< include … >}}` occurrence in `code_block.text`, possibly several, possibly mid-line), or restrict v1 to the observed idiom — a shortcode that is the sole content of its own line? The strand's corpus only exercises the sole-content case; the permissive rule is simpler to implement but harder to un-ship.

3. **Recursion.** The strand proposes "no recursion inside a code fence — a file included into a fence is code, not qmd" as the defensible v1, and asks that it be a decision rather than an accident. Confirm? If we do not recurse, do we still push the target onto the cycle stack (so `a.py` including itself is impossible by construction), or is recursion-off enough?

4. **Non-`.qmd` targets and trailing newlines.** `app.py`'s bytes end with a newline; the fence body does too. Do we splice bytes verbatim and let the writer normalize, or trim exactly one trailing newline so the fence has no blank last line? Q1's behavior here should be checked against a real Q1 render before we pin a test.

5. **The `.cell-code` timing wrinkle.** Do we accept that at include-expansion time an authored executable cell (```` ```{python} ````) gets its include spliced into the cell source *before* execution — matching Q1's text-level model — or do we deliberately skip fences whose first class names a known engine language? I lean toward matching Q1 (splice), with the reasoning recorded in a comment.

6. **`Q-17-4`'s future.** Once this shape is expanded at stage 3, the warning stops firing for it. Does `Q-17-4` keep its current single hint (now correct only for the inline-in-a-sentence case), or do we want position-aware hint text while we are here?

7. **Reject the alternatives explicitly?** The strand raises a fence attribute (```` ```{.python include="app.py"} ````) or a dedicated `embed-file` shortcode, and argues for rejecting them because they make every existing Q1 document wrong. Should the plan record that rejection formally (so it is not re-litigated), and is the "could be added later as an additional affordance" door worth leaving open in writing?

## Risks / tradeoffs (draft)

- **Cross-walker drift.** The expander and the preview dep collector share `child_block_lists_mut` precisely so they cannot diverge. A code-fence include is a *new kind* of position that does not fit that accessor's shape (it is not a block list). Adding it to both walkers without a shared helper reintroduces exactly the drift the current design prevents. Design the shared helper first.
- **Changing an invariant with a written rationale.** `shortcode_resolve.rs:623-629` documents "any `include` still present here is inline among other content, the one unsupported position." The `IncludeExpansionStage` fix keeps that sentence true; the transform-site fix falsifies it. Worth weighing when answering Q1.
- **Test churn is modest but load-bearing.** Four test sites plus a docs page assert today's behavior. None of them are snapshots, so the changes are explicit and reviewable — good. The pinning unit test `extract_include_path_from_non_paragraph` should be *rewritten*, not deleted, so the new contract stays pinned.
- **Unverified against real Q1.** The expected-output details in Q4 (trailing newline, indentation) are taken from the strand's prose, not from a side-by-side Q1 render performed in this investigation. Before pinning snapshot-grade expectations, run `quarto render` on the repro and diff. (The strand's README claims to carry expected-vs-actual markup; that file lives in the local-only Connect-docs checkout and was not consulted here.)
- **No incoming dependencies** means nothing else in this skein breaks whichever way we go — the risk is confined to include semantics.
