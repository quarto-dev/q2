# Nested include expansion: expand `{{< include >}}` inside container blocks

**Strand:** bd-1fz3vh99 (discovered-from bd-qpvoamvu)
**Status:** approved 2026-08-07 — implementation in progress on
branch `braid/bd-1fz3vh99-includes-nested-inside-container`

## Overview

`IncludeExpansionStage` (`crates/quarto-core/src/stage/stages/include_expansion.rs`)
walks only the **top-level** block list. An `{{< include >}}` inside any
container — a fenced div (callouts!), blockquote, list item, table cell —
is never expanded: the content is missing from the output and the only
signal is a Q-17-4 "Include Not Expanded Here" warning (before
bd-qpvoamvu it was a misleading Q-16-3). The Connect docs port hits this
with includes inside callout divs.

This plan proposes making include expansion apply at **every block-list
position in the AST**, and documents why the AST model makes the
"indentation-sensitive constructs" complication largely dissolve.

## How Quarto 1 does it (studied + empirically verified)

Q1's implementation (`external-sources/quarto-cli/src/core/handlers/include.ts`
+ `include-standalone.ts`) is **pre-engine textual splicing**:

- An include directive is recognized only when a **line, standing alone,
  is a block shortcode** (`isBlockShortcode` over `rangedLines`; the
  directive must form its own cell in break-quarto-md).
- The directive line is replaced by the included file's **raw text**, with
  recursion done textually (`retrieveInclude` re-scans the included
  file's lines) and cycle detection via a path stack (throws on cycle).
- **No re-indentation** is ever performed.

Empirical matrix (system `quarto` = Q1 dev checkout, probe fixture with
one include per position, 2026-08-07):

| Position | Q1 result |
|---|---|
| top level | expanded |
| fenced div / callout | expanded |
| blockquote (`> {{< include … >}}`) | **not expanded** — raw `{{< include … >}}` text passes through into the rendered output, silently |
| list item (`- {{< include … >}}`) | **not expanded** — same silent raw-text passthrough |

The div case works in Q1 *because of* the textual model: between `:::`
fences, the directive line still stands alone at column 0, so it is
recognized, and the spliced text lands inside the fences. Indented
positions (blockquote, list) never match the line-shape test, so Q1
doesn't support them at all — and fails **silently**, worse than Q2's
current warning.

The textual model also has a known footgun we do *not* want to
replicate: spliced text interacts with surrounding markup, so an
included file with unbalanced `:::` fences can corrupt the including
document's structure.

## What Q2 has today

- AST-level expansion, **top-level blocks only**, recognizer =
  `Paragraph` whose sole inline is `Shortcode(include)`
  (`extract_include_path`).
- Failure paths (cycle / not-found / parse error) report Q-17-1/2/3,
  surface the included file's own diagnostics, and remove the block
  (bd-qpvoamvu).
- Leftover `include` shortcodes reaching `ShortcodeResolve` get Q-17-4.
- The preview dep-graph endpoint (`quarto-preview/src/deps.rs::extract_include_deps`)
  reuses `extract_include_path` — and **also walks only top-level
  blocks**, so it has the same blind spot and must move in lockstep.

### Parse shapes inside containers (probed via pampa, 2026-08-07)

Every container position parses with the shortcode intact and
recognizable; the only wrinkle is **`Plain` vs `Paragraph`**:

| Position | Block shape holding the shortcode |
|---|---|
| fenced div | `Paragraph[Shortcode]` |
| blockquote | `Paragraph[Shortcode]` |
| tight bullet/ordered list item | **`Plain[Shortcode]`** |
| pipe-table cell | **`Plain[Shortcode]`** |

So the recognizer must generalize to *Paragraph-or-Plain whose sole
inline is the include shortcode*. (`Plain[Shortcode]` can only exist
where the author wrote exactly a lone shortcode in that position, so no
false-positive risk.)

## Proposed semantics

**One rule: an include block (`Paragraph`/`Plain` containing only the
include shortcode) is expanded at *any* block-list position in the
AST.** The included file is parsed as a standalone document (its own
column-0 baseline), and the resulting blocks are spliced into the block
list at the shortcode's position — i.e. they become children of
whatever construct contains the include.

Block-list positions in the pre-transform AST (all `Blocks = Vec<Block>`):

- top level (today's behavior)
- `Div.content` — **the Q1-parity case** (callouts are still plain divs
  at this stage; `AstTransformsStage` runs later)
- `BlockQuote.content`
- `BulletList.content` / `OrderedList.content` (each item is a `Blocks`)
- `DefinitionList.content` (each definition is a `Blocks`)
- `Figure.content`
- `NoteDefinitionFencedBlock.content` (fenced footnote definitions)
- `Table`: every `Cell.content` in `TableHead.rows`,
  `TableBody.{head,body}`, `TableFoot.rows`; `Caption.long`
- `Custom(CustomNode)`: none exist pre-transform (custom nodes are
  created by `AstTransformsStage`); skipped with a comment rather than
  traversed.

### Why the indentation complication dissolves

The anticipated hard case — includes inside indentation-sensitive
constructs — is a *textual-model* problem: Q1 would have had to
re-indent every included line to keep content inside a list item or
blockquote, and (correctly judging that fragile) chose not to support
those positions at all. In the AST model there is no indentation:
by the time expansion runs, the parser has already decided what is
inside the blockquote/list item, and the included file is parsed
against its own left margin. Splicing blocks into a `Blocks` vector is
position-independent. The construct's "indentation sensitivity" is
fully absorbed by parsing, which has already happened on both sides.

### Semantic consequences (to document)

1. **Included content cannot escape its container.** A two-paragraph
   include inside a list item yields two paragraphs *in that item* —
   the well-defined, almost-surely-intended reading. (In Q1's textual
   model, unindented continuation text would have broken out of the
   item; another reason Q1 didn't go there.)
2. **Sibling-item injection is inexpressible.** An included file that
   is itself a bullet list, included from inside a list item, becomes a
   *nested* list within that item — it cannot contribute sibling items
   to the surrounding list. (This was never expressible in Q1 either —
   includes in list items didn't work — so nothing regresses; it is
   simply the boundary of the block-splice model, worth documenting.)
3. **Fence-corruption tricks from Q1 are impossible** (already true in
   Q2 today): an included file's unbalanced `:::` cannot break the
   including document, because each file is parsed independently.
4. **Q-17-4 narrows to inline includes** (`text {{< include … >}} text`)
   — the one position that stays unsupported, matching Q1. Its catalog
   `message_template` ("only expanded when the shortcode is the sole
   content of its own paragraph") remains literally accurate; only the
   docs page's nested-container bullet needs removal.
5. **Frontmatter of included files stays ignored** (existing behavior,
   pinned by `included_file_frontmatter_stripped`).

### Adjacent gap, folded in (same machinery)

On the *success* path, `pampa::readers::qmd::read`'s returned
`_warnings` for the included file are silently dropped
(`include_expansion.rs`, the `(… , _warnings)` binding). That is the
success-side sibling of the bug fixed in bd-qpvoamvu. Since the fix is
the identical remap-and-push we already apply to error diagnostics,
fold it in: surface included-file parse warnings, remapped into the
parent's `SourceContext`.

## Implementation sketch

### 1. Restructure the walker

Replace the free function + `sub_doc`/`split_off` recursion dance with
an expander struct that borrows the document-level state once
(destructure `DocumentAst` to satisfy the borrow checker):

```rust
struct IncludeExpander<'a> {
    ctx: &'a mut StageContext,            // runtime + diagnostics
    ast_context: &'a mut ASTContext,      // FileId registration (lockstep)
    source_context: &'a mut SourceContext,
    recorded_includes: &'a mut Vec<IncludeEntry>,
    include_stack: HashSet<PathBuf>,
}

impl IncludeExpander<'_> {
    fn expand_blocks(&mut self, blocks: &mut Vec<Block>, current_file: &Path)
        -> Result<(), PipelineError>
    {
        let mut i = 0;
        while i < blocks.len() {
            if let Some(path) = extract_include_path(&blocks[i]) {
                // resolve → cycle-check → read → parse
                // on failure: diagnostics + blocks.remove(i); continue
                // on success:
                //   register file in BOTH contexts, remap FileId(0),
                //   push remapped parse warnings, record_include,
                //   include_stack.insert(canonical);
                //   let mut children = included.blocks;
                //   self.expand_blocks(&mut children, &resolved)?;   // recurse FIRST
                //   include_stack.remove(&canonical);
                //   let n = children.len();
                //   blocks.splice(i..i + 1, children);               // THEN splice
                //   i += n;
            } else {
                self.expand_containers(&mut blocks[i], current_file)?; // Div, lists, …
                i += 1;
            }
        }
        Ok(())
    }
}
```

Expanding the included children *before* splicing (instead of today's
splice-then-rewalk-through-a-cloned-sub-document) removes the
`split_off` + full-clone merge dance and makes the recursion shape
identical at every nesting level. `expand_containers` is a match over
the container variants listed above, calling `expand_blocks` on each
`Blocks` field. All existing per-include mechanics (dual-context
registration with lockstep FileIds, `remap_file_ids`, `record_include`
dedup, cycle stack push/pop) carry over unchanged — the FileId
bookkeeping the strand worried about is per-*include*, not per-*depth*,
so nesting adds nothing new.

### 2. Generalize the recognizer

`extract_include_path` accepts `Block::Paragraph` **or** `Block::Plain`
with a single `Inline::Shortcode(include)`. (Public API shared with
quarto-preview; signature unchanged.)

### 3. Shared traversal for the preview dep-graph

Add next to it, exported the same way:

```rust
pub fn collect_include_paths(blocks: &[Block]) -> Vec<String>
```

— an immutable walk over the *same* container positions. Switch
`quarto-preview/src/deps.rs::extract_include_deps` from its top-level
`filter_map(extract_include_path)` to this helper, so "what the
renderer expands" and "what the preview dep-filter sees" cannot drift.
(Otherwise: preview would serve stale content for pages whose includes
are nested — the exact bug class `deps.rs` exists to prevent.)

### 4. Diagnostics & docs touch-ups

- `shortcode_resolve.rs`: the Q-17-4 comment mentioning bd-1fz3vh99
  updates to "inline includes only".
- `docs/errors/include/Q-17-4.qmd`: drop the nested-container bullet.
- Q-17-1 docs page: unchanged (cycles can now thread through nested
  positions; the description already covers indirect cycles).
- No new error codes needed.

## Test plan (TDD — tests written and verified failing first)

### Phase 1 — unit tests (`include_expansion.rs` tests mod, MockFileRuntime)

- [x] U1: include inside a `Div` expands (blocks land inside the div;
      div's other children intact).
- [x] U2: include inside a `BlockQuote` expands into the quote.
- [x] U3: include as a tight bullet-list item (`Plain` shape) expands
      inside that item; sibling items untouched.
- [x] U4: include in an ordered-list item expands.
- [x] U5: nested containers (include in a div in a div) expands.
- [x] U6: nested include chains resolve relative to the *declaring*
      file's directory across subdirectories (a/`x.qmd` includes
      b/`y.qmd` from inside a div; y includes `z.qmd` relative to b/).
- [x] U7: cycle threading through a container (doc → div-include →
      file that includes doc) reports Q-17-1, block removed, no hang.
- [x] U8: parse-error include inside a div → Q-17-3 + remapped inner
      diagnostics, block removed from the *div's* child list.
- [x] U9: include in a table cell expands (multi-block content in a
      cell).
- [x] U10: included-file parse *warnings* surface remapped (success
      path; the folded-in gap).

### Phase 2 — integration tests (`include_expansion_diagnostics.rs` sibling file)

- [x] I1: full-pipeline render of a callout-div include → content in
      the emitted HTML inside the callout, **no** Q-17-4, no Q-16-3.
      (This is the bd-1fz3vh99 repro, inverted.)
- [x] I2: full-pipeline render of a list-item include → content inside
      `<li>`.
- [x] I3: `extract_include_deps` (quarto-preview) reports includes
      nested in divs/lists.

### Phase 3 — implementation

- [x] Expander-struct refactor (behavior-preserving at top level —
      existing 22 include tests must stay green before containers are
      added).
- [x] Container recursion + `Plain` recognizer + warnings surfacing.
- [x] `collect_include_paths` + preview `deps.rs` switch.
- [x] Docs/comment touch-ups (Q-17-4 page, shortcode_resolve comment).

### Phase 4 — verification

- [x] Smoke-all fixture `includes/nested/` (div + list positions,
      `ensureHtmlElements` asserting content inside the containers).
- [x] `cargo nextest run --workspace`: 11018 passed (2026-08-07).
      Full `cargo xtask verify` (WASM + hub legs): green — see final
      session notes.
- [x] E2E per CLAUDE.md (2026-08-07): `q2 render index.qmd --to html`
      on a scratchpad fixture with the same include in a callout div, a
      blockquote, and a tight bullet item (`_inc.qmd` = two paragraphs
      with an `E2E-NESTED-MARKER`). Inspected `index.html`: marker
      appears 3×; regex checks confirm it sits after `callout-note`,
      inside `<blockquote>…</blockquote>`, and inside `<li>…</li>`.
      Render printed zero warnings/errors.
- [x] Connect docs check (2026-08-07): a structural search found the
      port currently has **no** includes directly inside div fences
      (60+ files use top-level includes; the nested pattern was the
      hazard, not yet the practice). Re-rendered the include-heaviest
      page (`admin/integrations/oauth-integrations/microsoft/index.qmd`,
      6 includes) — zero Q-17-1/2/4 or Q-16-3 diagnostics; top-level
      path unregressed. Nested positions are covered by the smoke
      fixture and integration tests.

## Decisions (reviewed 2026-08-07)

1. **Position scope: uniform.** Expand at every block-list position
   (incl. table cells, figure content, footnote definitions,
   `Caption.long`).
2. **User-facing includes guide: separate docs strand.** Filed as
   **bd-whft9m1j**, child of the existing documentation epic
   **bd-tr81** ("bootstrap Quarto 2 docs using Quarto 2 itself"),
   related-linked to this strand. This strand stays code+tests.
3. **Table-cell includes: in scope**, with the writer question settled
   by verification (below). If the tests still surface a writer
   problem, demote table cells to a follow-up rather than blocking the
   div/list core.

### Table-writer verification (2026-08-07)

Confirmed: pampa's qmd writer switches between pipe-table and
list-table notation exactly as expected (the list-table pandoc-filter
convention made first-class). `table_can_use_pipe_format`
(`crates/pampa/src/writers/qmd.rs:969`, call site :1274) forces
list-table notation whenever any cell has row/col spans or content
that is not a single break-free `Plain`/`Paragraph` —
`cell_has_simple_content` (:946). Multi-block cells produced by
include expansion therefore serialize correctly. Verified empirically:
a `.list-table` div fixture with a two-paragraph cell round-trips
through `pampa -t qmd` in list-table notation. The HTML writer emits
`Blocks` inside `<td>` naturally.

One latent gap found while verifying, filed as **bd-ao03uwvm** (p3,
discovered-from this strand): `table_can_use_pipe_format` checks
`table.head.rows`, each body's `.body` rows, and `foot.rows`, but
skips `TableBody.head` (per-row-group header rows) — a complex cell
there would be mis-written as a pipe table. Only reachable when
`TableBody.head` is populated (likely JSON-input-only today); not a
blocker for this strand.

## Related

- bd-qpvoamvu (merged, PR #465) — failure diagnostics + Q-17-x codes;
  built the machinery (dual-context registration, remap, block removal)
  this plan reuses.
- `claude-notes/plans/2026-08-07-include-error-diagnostics.md` — prior
  plan with the diagnosis that discovered this strand.
