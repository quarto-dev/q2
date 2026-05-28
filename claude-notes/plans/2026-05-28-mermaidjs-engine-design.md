---
date: 2026-05-28
branch: TBD (no implementation work yet — design phase)
status: v1 draft — design questions open, awaiting user review.
beads: bd-je48v (epic); see § Beads issues below.
---

# `mermaidjs` engine — design session

## Goal

Add `{mermaid}` code-block support to Quarto 2. The first cut should be
deliberately minimal: at the HTML output the engine should emit a
`<pre class="mermaid">…</pre>` per block plus a single
`<script type="module">` include that pulls mermaid from jsdelivr and
calls `mermaid.initialize({ startOnLoad: true })`. All diagram
rendering happens in the browser at page load; the engine does **no**
subprocess work, no SVG generation, no PDF fallback. Future formats
(PDF, docx, …) are out of scope for the first cut but should not be
foreclosed by the design.

This is intentionally the simplest non-trivial diagram engine we can
ship. We use it as a forcing function to surface the engine→pipeline
integration questions the user has flagged.

## Why this is a design session, not just a small PR

The user posed three architectural questions that demand answers
before code is written:

1. Can the `ExecutionEngine` trait, *as it exists today*, accommodate
   a clean format-agnostic vs. HTML-specific decomposition for a
   diagram engine? If not, what extension shape do we want?
2. How does the engine's contribution survive the `q2 preview` round
   trip (native → trace → automerge → WASM → replay)? In particular,
   can downstream stages observe "this doc has mermaid blocks" even
   when the engine never actually ran in the browser?
3. Should engines be allowed to declare Lua filters and/or publish
   virtual files to the VFS? Both are mechanisms Quarto 1 leaned on
   for diagram handling. Q2 nominally has the slots
   (`ExecuteResult::filters`, a `Vfs` layer) but the wiring is
   incomplete.

This plan answers (1)–(3) with concrete code citations, lays out the
design options, and proposes a phased path that ships the mermaid
engine without committing to the larger architectural moves
prematurely.

## Current substrate (verified, with file:line citations)

All citations refer to the workspace root `/Users/cscheid/rooms/room-2/q2`.

### The `ExecutionEngine` trait

`crates/quarto-core/src/engine/traits.rs:56-127`. Three required
methods (`name`, `execute`, defaults for `can_freeze`,
`intermediate_files`, `is_available`). The trait doc explicitly
characterizes the contract as **text-in / text-out**:

> Engines transform markdown with executable code cells into markdown
> with execution outputs. The transformation is text-in/text-out.

`ExecuteResult` (`crates/quarto-core/src/engine/context.rs:127-206`)
is the engine's full output surface:

| Field | Purpose | Consumed downstream? |
|---|---|---|
| `markdown: String` | Transformed QMD that downstream stages re-parse | **Yes** — `EngineExecutionStage` replaces `PipelineData::DocumentAst` with the re-parsed result. |
| `supporting_files: Vec<PathBuf>` | Figures, data files | **Yes** — drained into `ctx.resource_report` (`engine_execution.rs:291-296`) for orchestrator publishing. |
| `includes: PandocIncludes` | Per-block CSS/JS injected into header/body/before-body | **Yes** — extended into `ctx.includes` (`engine_execution.rs:273-281`); consumed by `ApplyTemplateStage`. |
| `filters: Vec<String>` | Lua/Pandoc filters the engine wants applied | **No** — see § Gap G1. |
| `needs_postprocess: bool` | Hint for post-processing | **No** — set by engines, no consumer found. |

### Existing engines

`MarkdownEngine` (passthrough), `KnitrEngine`, `JupyterEngine`,
`ReplayEngine`. The native registry wires markdown+knitr+jupyter; the
WASM registry wires markdown only
(`crates/quarto-core/src/engine/registry.rs` lines 52–68 per the
research agent's report — to re-verify before implementation).

`KnitrEngine` (`crates/quarto-core/src/engine/knitr/mod.rs:215`) sets
`filters: result.filters` from the R subprocess output — and a unit
test asserts `vec!["rmarkdown/pagebreak.lua"]`
(`knitr/types.rs:281`). So Knitr actively populates a field that is
never read — see § Gap G1.

### Engine determination is document-wide, not per block

`detect_engine(&doc_ast.ast.meta)` reads the YAML `engine:` key from
document metadata (`engine_execution.rs:164` per agent report).
There is one engine per document. `{r}` / `{python}` / `{julia}`
class membership is interpreted *within* the chosen engine, not used
to dispatch *between* engines.

**This is the most important architectural fact for the mermaid
design.** A user writing a Knitr or Jupyter document who wants a
single mermaid block does not currently have a clean way to express
"use Knitr for R blocks, but use the mermaid handler for this one
block." See § Design question Q-A.

### Pipeline stages and the generate/render boundary

`build_html_pipeline_stages()` in `crates/quarto-core/src/pipeline.rs:207-300`
defines the canonical native HTML pipeline. The 19-stage order
(verbatim from the research report; spot-check before implementation):

1. `ParseDocumentStage` — QMD → Pandoc AST
2. `MetadataMergeStage`
3. `IncludeExpansionStage` — `{{< include >}}` shortcodes
4. `IncludeResolveStage` — `include-in-header` etc.
5. `ListingItemInfoStage`
6. **`DocumentProfileStage`** ← profile checkpoint
7. `LinkResolutionStage`
8. **`UnwrapProfileStage`** ← exit profile checkpoint
9. `PreEngineSugaringStage` — crossref registry seed
10. **`EngineExecutionStage`** ← engines run here
11. `CompileThemeCssStage`
12. `BootstrapJsStage` (native only)
13. `ClipboardJsStage` (native only)
14. `UserFiltersStage::pre()`
15. `AstTransformsStage` — Quarto built-in transforms (callouts, …)
16. `UserFiltersStage::post()`
17. `CodeHighlightStage`
18. `RenderHtmlBodyStage` — HTML body emission
19. `ApplyTemplateStage` — wrap with HTML template

Stages 1–17 operate on AST and are nominally format-agnostic; stages
18–19 are HTML-specific. The pipeline list itself is hardcoded
(option-based feature flags exist but engines cannot mutate the
list). See § Gap G3.

### Trace + WASM replay status

`quarto-trace` provides `EngineCapture` (`engine_name`, `input_qmd`,
`result: serde_json::Value`) and `TraceDocument` which holds them.
`crates/quarto-core/src/engine/replay.rs:53` defines `ReplayEngine`,
which deserializes the recorded `ExecuteResult` and returns it
verbatim. Hard-fails if input QMD doesn't match exactly — i.e. it's a
deterministic regression-test tool, not a fuzzy replay.

`q2 preview` uses a different path: `CaptureSpliceStage`
(`crates/quarto-core/src/stage/stages/capture_splice.rs`) sits between
`PreEngineSugaringStage` and `EngineExecutionStage`, splices captured
engine output AST onto the live pre-engine AST keyed by
`(structural_hash(cell), occurrence_index)`, then lets the (WASM
markdown-only) engine stage no-op. Recent commits
(c86e1d96 "fold capture sequence", 51abb673 "per-engine trace
captures + sequence replay", 30a57abc "array engine config",
ed9cbbfe "Phases 0-5 complete") indicate **multi-engine capture
support is in flight** under bd-5yff4. We need to read that work
before designing.

**Critical observation about capture-splice and `ExecuteResult`
fields**: `capture_splice.rs:121-126` explicitly *only* consumes
`result.markdown` from the captured `ExecuteResult`:

> Extract result.markdown from the opaque JSON. We don't need the
> rest of the ExecuteResult shape here (filters, includes,
> supporting_files) — those are engine-side concerns the splice
> doesn't reproduce in the AST. Future work could surface them
> through StageContext if a real splice consumer needs them.

So even if the mermaid engine populates `includes` with the script
tag, the WASM-preview path **drops it on the floor**. See § Gap G6.

### VFS

`crates/quarto-system-runtime/src/wasm.rs:227-261` defines
`VirtualFileSystem` with `project_root: PathBuf::from("/project")`.
Write entry points (`vfs_add_file`, `vfs_add_binary_file`) are
exposed as `wasm-bindgen` exports — i.e. **JavaScript → WASM
only**. There is no internal Rust API a pipeline stage or engine
could call to publish a file. See § Gap G2.

### Filter resolution

`crates/quarto-core/src/filter_resolve.rs` resolves filters from the
YAML `filters:` document metadata key. Per the research agent: it
checks `runtime.path_exists(document_dir.join(name))` then falls
back to extension lookup. It **does not** consult
`ExecuteResult.filters`. Confirmed: a workspace-wide grep for
`result.filters` / `ExecuteResult.*filters` returns hits only in
tests, the knitr engine (write side), extension parsing, and the
capture-splice comment quoted above — **no production consumer**.

## Gaps identified by the design exercise

These are pre-existing gaps in the codebase that the mermaid engine
forces us to confront. Each is a candidate beads issue.

### G1. `ExecuteResult.filters` is captured but never consumed

Verified by workspace grep + reading `filter_resolve.rs`. The Knitr
engine writes filters; nothing reads them. Knitr currently produces
documents that silently skip whatever filters it intended to apply.

**Mermaid-specific consequence:** If we choose to implement mermaid
handling as a Lua filter declared via `ExecuteResult.filters`, that
path is dead until G1 is fixed.

### G2. Engines cannot publish virtual files to the VFS

`VirtualFileSystem` write APIs are JS-only. There is no
`Vfs::publish(path, contents)` callable from inside the pipeline.

**Mermaid-specific consequence:** Even if (G1) is fixed and the
mermaid engine declares a Lua filter, it has no way to *provide
the filter source*. The filter would have to ship pre-installed
into the binary or be served by an extension — adding complexity
out of proportion to the mermaid use case.

### G3. No mechanism for engines to declare pipeline stages

The 19-stage list in `pipeline.rs:207-300` is hardcoded. Stages can
be gated by config (`native only`), but no engine or extension can
register a new one.

**Mermaid-specific consequence:** If we want a dedicated
`MermaidTransformStage` for the marker→`<pre class=mermaid>`
conversion, *we* must add it to the canonical list; engines can't
contribute it.

### G4. Format-conditional pipeline stages exist only via cfg

Stages like `BootstrapJsStage` are HTML-only via `native only` gates
in code, not via a format-target abstraction. There's no
`Stage::applies_to(format: &Format) -> bool` or equivalent
per-format dispatch.

**Mermaid-specific consequence:** The mermaid AST marker→HTML
conversion is HTML-specific. We need a place to put it that won't
fire when we eventually support PDF (where mermaid would need a
different rendering strategy — server-side SVG or a pre-rendered
image).

### G5. (Subsumed into G3 — capture-splice path drops aux ExecuteResult fields)

Strictly a sub-case of how the engine boundary is too narrow for
side-effecting "include this script" to survive the q2-preview round
trip. See `capture_splice.rs:121-126`. May be cheap to fix
independently of the broader engine→pipeline extension question.

### G6. `ExecuteResult.includes` does flow on native but is dropped on q2-preview replay

This deserves its own item even though it's mechanically the same as
G5. On native, the script include the mermaid engine would emit
*does* reach `ApplyTemplateStage`. On q2-preview (the capture-splice
path), it doesn't. So a naively-implemented mermaid engine would
**work in `q2 render` but silently fail to render diagrams in
`q2 preview`** — exactly the same shape of bug as the
`CodeHighlightStage` and stale-WASM incidents in CLAUDE.md.

## Design questions to resolve in this session

### Q-A. What kind of "thing" is mermaid?

The user phrased it as an "engine," but mechanically the work is
closer to an AST transform: see a code block with class `mermaid`,
rewrite it. In Quarto 1, this *was* a Lua filter, not a top-level
engine. In Q2 the `ExecutionEngine` trait is currently
**document-wide** (one engine per doc, selected by YAML `engine:`).
So "MermaidEngine implements `ExecutionEngine`" doesn't fit the
existing dispatch — it would only work if (a) we make engine
selection per-block, or (b) the mermaid handler runs *layered on
top* of the doc-level engine, processing only its own blocks.

Options:

- **A1.** Implement mermaid as an `ExecutionEngine` impl. Engine
  dispatch becomes per-block (significant change). The doc-level
  `engine:` key still applies for ambiguous blocks; per-block
  `{mermaid}` overrides.
- **A2.** Implement mermaid as an `ExecutionEngine` impl that runs
  *in addition to* the doc-level engine, only on its own blocks.
  New "pre-engine" or "post-engine" extension slot.
- **A3.** Implement mermaid as an AST transform inside
  `AstTransformsStage` (stage 15). Matches Q1's Lua-filter model.
  No engine-system changes. Cleanest first cut.
- **A4.** Implement mermaid as a dedicated `MermaidStage` between
  AstTransforms and CodeHighlight. Mechanically identical to A3 but
  more discoverable in the pipeline list.
- **A5.** Implement mermaid as an `ExecutionEngine` *and* as an AST
  transform, depending on context. Worst of both worlds; reject.

**Recommendation:** A3 or A4 for the first ship. They exercise zero
new architecture, work in native and (modulo G6) in preview, and
match user mental model of "Quarto handles `{mermaid}` blocks
specially." A1/A2 are the more interesting architectural directions
to evolve toward, but they are out of scope for the first cut.

### Q-B. Where does the format-agnostic vs HTML-specific split land?

Two reasonable splits:

- **B1.** Engine/transform emits a `RawBlock(HTML, "<pre class=\"mermaid\">…</pre>")`
  directly. Format-agnostic story is: "in non-HTML output, mermaid
  blocks are unsupported and pass through as raw text." Simple but
  closes the door on PDF.
- **B2.** Engine/transform emits a Pandoc `Div` with class
  `mermaid` wrapping the original code as a `CodeBlock`. A separate
  HTML-conditional stage (or HTML writer extension) rewrites the
  `Div` into `<pre class="mermaid">…</pre>`. Format-agnostic AST
  preserves the marker; format-specific render does the actual
  emission.

**Recommendation:** B2. It costs little extra (one extra pass) and
preserves the architectural invariant the user values.

### Q-C. How does the script include reach the document?

The script tag needs to be injected once per document iff at least
one mermaid block exists. Options:

- **C1.** The transform emits the script as a `RawBlock` at the end
  of the body on first sight of a mermaid block.
- **C2.** The transform populates `ctx.includes`
  (`PandocIncludes::include_after_body`) via a side channel.
  Requires the transform stage to have mutable access to
  `StageContext.includes`. Native: yes. q2-preview-via-splice:
  needs G6.
- **C3.** Engine returns the include in `ExecuteResult.includes`
  on the engine's first invocation per document. Requires
  Q-A to land on A1/A2 (engine path).

**Recommendation:** C1 for the first cut. It works uniformly on
native and q2-preview, doesn't depend on G6, and is observable in
the AST (debuggable). C2 is cleaner but blocked on G6.

### Q-D. WASM replay coverage

Even with A3+B2+C1, the q2-preview path needs verification:

- Native `q2 render` produces correct HTML — should be straightforward
  to test.
- `q2 preview` of a doc containing both `{r}` (replayed) and
  `{mermaid}` blocks should still produce a working diagram. Since
  the mermaid transform runs *after* `EngineExecutionStage` and is
  pure AST→AST, it should fire identically in the WASM pipeline.
  But we have to confirm that:
  - The WASM pipeline includes the mermaid transform stage.
  - `CaptureSpliceStage` doesn't accidentally remove the mermaid
    code block (it splices recorded *output*; the input mermaid
    block should be passed through unchanged because the mermaid
    handler is not the doc-level engine).
- Multi-engine work in flight (bd-5yff4) may already address some
  of this. Read that plan before settling on the test matrix.

## Proposed phased work

### Phase 0 — design ratification (this session)

- [ ] Confirm or revise the gap analysis (G1–G6) by reading the
      actual code under the citations above. Anything that's been
      fixed since the agent reports were generated needs updating.
- [ ] User decides on Q-A (architectural framing), Q-B (split), Q-C
      (script include strategy).
- [ ] Update this plan with the decisions, then open implementation
      beads.

### Phase 1 — multi-engine current-state audit

- [ ] Read `claude-notes/plans/` entries for bd-5yff4 ("multi-engine
      preview" work) and the related commits (c86e1d96, 51abb673,
      30a57abc, ed9cbbfe). Summarize the current state of per-engine
      capture and what's still TBD. Add findings to this plan.
- [ ] Re-verify whether the `EngineRegistry` API has changed since
      the research-agent report; recheck the registry construction
      and engine name set.

### Phase 2 — mermaid transform implementation (assuming A3/A4 + B2 + C1)

- [ ] Add a new pipeline stage `MermaidTransformStage` (or inline
      into `AstTransformsStage`) that:
  - Walks the AST for `CodeBlock` nodes with class `mermaid`.
  - Rewrites each into a `Div` with class `mermaid` wrapping the
    code text (B2 format-agnostic step).
  - On first sight of any mermaid block in a document, appends a
    `RawBlock(HTML, …)` at the end of the body with the jsdelivr
    script tag (C1).
- [ ] Add HTML-render conversion: either a sub-stage that turns the
      `Div.mermaid` into `RawBlock(HTML, "<pre class=\"mermaid\">…</pre>")`,
      or special-case the HTML writer.
- [ ] Tests:
  - Unit: transform on a fixture AST.
  - Integration: full `q2 render` of a fixture qmd containing
    `{mermaid}` blocks; inspect generated HTML for `<pre class="mermaid">`
    and the script tag.
  - **End-to-end per CLAUDE.md**: actually run `cargo run --bin q2 -- render fixture.qmd`
    and grep the output. Record the invocation + observed output in
    this plan.

### Phase 3 — q2-preview verification

- [ ] Write a test that runs a mermaid-containing fixture through
      the `q2-preview` pipeline (with and without a capture). Verify
      the mermaid blocks survive and the script include is present.
- [ ] If G6 or related capture-splice gaps block this, decide
      between fixing the gap now vs. accepting a known limitation
      tracked as a separate beads issue.

### Phase 4 — documentation

- [ ] User-facing docs page under `docs/` for the mermaid engine.
- [ ] Architecture note in `claude-notes/designs/` if the
      ExecutionEngine extension story moves (i.e. if Q-A lands on
      A1/A2 in a follow-up).

### Follow-up issues (separate beads — not blockers for shipping mermaid via A3)

- [ ] **G1**: Wire `ExecuteResult.filters` into `filter_resolve.rs`
      (or remove the field if we decide the architectural direction
      is different).
- [ ] **G2**: Define a `SystemRuntime`/`Vfs` publication API that
      pipeline producers can call.
- [ ] **G3**: Define an engine→pipeline stage extension API
      (probably tied to whichever direction Q-A lands on).
- [ ] **G6**: Make `CaptureSpliceStage` surface `includes` /
      `filters` / `supporting_files` from the recorded
      `ExecuteResult` through `StageContext`. Tracked separately
      because it has its own design.

## Open questions

1. Is there a stronger reason than "Q1 did it this way" to model
   mermaid as an *engine* rather than a transform? The user framed
   it as an engine — was that mechanical-API language ("a thing
   that handles `{mermaid}` blocks") or architectural intent ("a
   first-class `ExecutionEngine` impl")?
2. Should the script-tag include be inside the document body
   (`RawBlock` at end of body) or via template (`include-after-body`
   metadata)? Body-inline is simpler and survives any template;
   metadata is more idiomatic. We probably want body-inline for the
   first cut.
3. The user mentioned `mermaid@11` pinned at jsdelivr. Should the
   version be configurable per-document or hardcoded? Defer to a
   user-facing config follow-up.
4. Mermaid supports init options (theme, fontFamily, etc.). The
   first cut hardcodes `initialize({ startOnLoad: true })`. Defer
   options to a follow-up.

## Beads issues

Created 2026-05-28:

- **`bd-je48v`** (epic, P2) — "Add mermaid diagram engine to Quarto 2."
- **`bd-c6h96`** (task, P2, child of epic) — "Mermaid engine design
  session: resolve architectural questions." This is where Q-A/Q-B/Q-C/Q-D
  get ratified. Plan doc: this file.
- **`bd-fztki`** (task, P2, child of epic, blocks impl) — "Audit
  current multi-engine + per-engine trace state (bd-5yff4)."
- **`bd-gwfdo`** (task, P2, child of epic, blocked-by `bd-c6h96` +
  `bd-fztki`) — "Implement mermaid AST transform + HTML emission."
- **`bd-my0o5`** (task, P2, child of epic, blocked-by `bd-gwfdo`) —
  "q2 preview end-to-end verification for mermaid blocks."
- **`bd-5ijtt`** (task, P3, child of epic, blocked-by `bd-gwfdo`) —
  "User-facing docs for mermaid diagrams."

Follow-up issues (filed as discovered-from `bd-c6h96` — not blockers
for shipping mermaid via the recommended A3/B2/C1 path, but separate
bugs/features that the design exercise surfaced):

- **`bd-14rer`** (bug, P2) — "ExecuteResult.filters set by Knitr
  engine is never consumed downstream." Knitr's `rmarkdown/pagebreak.lua`
  is silently dropped today.
- **`bd-s8llm`** (feature, P3) — "No internal API for pipeline
  producers to publish files into the VFS."
- **`bd-mqk49`** (feature, P3) — "Design: how should
  engines/extensions declare additional pipeline stages?"
- **`bd-cp3em`** (bug, P2) — "CaptureSpliceStage drops
  includes/filters/supporting_files from recorded ExecuteResult."
  Same shape as the CodeHighlightStage incident in CLAUDE.md.

## References

- ExecutionEngine trait: `crates/quarto-core/src/engine/traits.rs:56-127`
- ExecuteResult: `crates/quarto-core/src/engine/context.rs:127-206`
- EngineExecutionStage (includes/files drain): `crates/quarto-core/src/stage/stages/engine_execution.rs:241-296`
- KnitrEngine filter write site: `crates/quarto-core/src/engine/knitr/mod.rs:215`
- Capture-splice aux-field drop site: `crates/quarto-core/src/stage/stages/capture_splice.rs:121-126`
- ReplayEngine: `crates/quarto-core/src/engine/replay.rs:53,95-111`
- Pipeline stages: `crates/quarto-core/src/pipeline.rs:207-300`
- Document profile contract: `claude-notes/designs/document-profile-contract.md`
- Filter resolution: `crates/quarto-core/src/filter_resolve.rs`
- VFS: `crates/quarto-system-runtime/src/wasm.rs:227-261`
- q2-preview epic: `claude-notes/plans/2026-05-11-q2-preview-epic.md`
- Multi-engine work in flight: `claude-notes/plans/` entries for bd-5yff4 (re-verify)
