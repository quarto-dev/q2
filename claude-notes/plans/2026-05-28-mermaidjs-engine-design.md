---
date: 2026-05-28
branch: TBD (no implementation work yet — design phase)
status: >
  v2 — Q-A resolved by PR #238 (engine sequence); Q-B/Q-C/Q-D still
  open. Implementation gated on PR #238 merging.
beads: bd-je48v (epic); see § Beads issues below.
---

# `mermaidjs` engine — design session

## Goal

Add `{mermaid}` code-block support to Quarto 2. The first cut should be
deliberately minimal: at HTML output, each `{mermaid}` block becomes
`<pre class="mermaid">…</pre>` plus a single
`<script type="module">` include that pulls mermaid from jsdelivr and
calls `mermaid.initialize({ startOnLoad: true })`. All diagram
rendering happens in the browser at page load; the engine does **no**
subprocess work, no SVG generation, no PDF fallback. Future formats
(PDF, docx, …) are out of scope for the first cut but should not be
foreclosed by the design.

## Why this is a design session, not just a small PR

The user posed three architectural questions that demand answers
before code is written:

1. Can the `ExecutionEngine` trait accommodate a clean
   format-agnostic vs. HTML-specific decomposition for a diagram
   engine? If not, what extension shape do we want?
2. How does the engine's contribution survive the `q2 preview` round
   trip (native → trace → automerge → WASM → replay)?
3. Should engines be allowed to declare Lua filters and/or publish
   virtual files to the VFS? Both are mechanisms Quarto 1 leaned on
   for diagram handling. Q2 nominally has the slots
   (`ExecuteResult::filters`, a `Vfs` layer) but the wiring is
   incomplete.

## v2 revision summary (post PR #238)

PR [#238](https://github.com/quarto-dev/q2/pull/238) ("Sequential
multi-engine execution") lands the `engine: [a, b, …]` model where
engines run in sequence and each consumes a `DocumentAst` and returns
one. The PR description literally hypothesizes `engine: [knitr,
mermaidjs]`. This **resolves Q-A** (mermaid is a first-class
`ExecutionEngine`, not an AST transform) and sharpens Q-B (the engine
emits a format-agnostic marker — but where does the HTML-specific
conversion live?). Q-C and Q-D are unchanged in shape. The mermaid
implementation is now **gated on PR #238 merging**.

The detailed multi-engine plan lives at
`claude-notes/plans/2026-05-27-multi-engine-execution.md` (on the
`feature/multi-engine` branch). Read it before starting impl. Key
mechanics reproduced here:

- `engine: [knitr, mermaidjs]` parses via the same array-merge
  machinery as other config; `detect_engine_sequence` returns the
  ordered list.
- `EngineExecutionStage` loops the engines in order. For each: serialize
  AST → engine `execute` → parse result → reconcile against prior AST.
- Per-engine FileId provenance: `<stem>.<engine>.rmarkdown`.
- Trace records one `EngineCapture` per engine — `TraceDocument.engine_captures: Vec`
  (with back-compat read of the legacy single slot).
- `CaptureSpliceStage` folds the capture sequence (engine N+1's splice
  runs on engine N's spliced output).

PR #238 explicitly calls out three honest gaps:

- **bd-iq0hp** — no browser E2E of multi-engine preview was possible
  in the PR (real engines don't compose cleanly: knitr claims
  `{python}` via reticulate). **The mermaid engine is the natural
  first real-engine pair with clean cell ownership** (`{r}` vs
  `{mermaid}` never overlap), so our preview-e2e task (bd-my0o5)
  effectively closes bd-iq0hp.
- **bd-r8n4r** — capture-splice fold walks top-level blocks; a
  handoff cell nested inside a `Div.cell` wrapper is a v1
  limitation. Mermaid cells appear at the top level so this should
  not bite.
- **bd-8h3sn** — engine-2+ source attribution is best-effort.

## Current substrate (verified, with file:line citations)

All citations refer to the workspace root `/Users/cscheid/rooms/room-2/q2`.
Where the substrate is reshaped by PR #238 the citation is to
`feature/multi-engine`; where the substrate is unchanged the citation
is to `main`.

### The `ExecutionEngine` trait

`crates/quarto-core/src/engine/traits.rs:56-127` (unchanged by
PR #238). Three required methods (`name`, `execute`, defaults for
`can_freeze`, `intermediate_files`, `is_available`). PR #238's own
plan note: "The trait is per-invocation text→text and needs no change
to support sequencing — the *stage* drives the loop, not the engine."

`ExecuteResult` (`crates/quarto-core/src/engine/context.rs:127-206`)
is the engine's full output surface:

| Field | Purpose | Consumed downstream on native? | On q2-preview-replay? |
|---|---|---|---|
| `markdown: String` | Transformed QMD that downstream stages re-parse | **Yes** | **Yes** (via capture-splice) |
| `supporting_files: Vec<PathBuf>` | Figures, data files | **Yes** | **No** — dropped by capture-splice (§ Gap G6) |
| `includes: PandocIncludes` | Per-block CSS/JS injected into header/body | **Yes** | **No** — dropped by capture-splice (§ Gap G6) |
| `filters: Vec<String>` | Lua/Pandoc filters | **No** — never read by any pipeline stage (§ Gap G1) | n/a |
| `needs_postprocess: bool` | Post-processing hint | **No** | n/a |

### Engine determination (post PR #238)

Documents declare an *ordered sequence* of engines:

```yaml
engine:
  - knitr
  - mermaidjs
```

`detect_engine_sequence(meta)` returns the list; the singular
`detect_engine` shim returns the first for back-compat callers.
Array merge follows the existing `!concat` default — a project-level
`[knitr]` plus a doc-level `[mermaidjs]` materializes as
`[knitr, mermaidjs]`. Duplicates are deduped first-wins with a
diagnostic. Cell ownership is **each engine's internal concern**:
mermaidjs's `execute` walks for `{mermaid}` cells, ignores
everything else, and returns. Per the PR's verified merge table,
`[knitr, mermaidjs]` is the canonical compose-from-two-layers
example.

### Pipeline stages and the generate/render boundary

`build_html_pipeline_stages()` in `crates/quarto-core/src/pipeline.rs`
defines the canonical native HTML pipeline. The 19-stage order (verify
ordering against the current PR #238 file before implementation):

1. `ParseDocumentStage`
2. `MetadataMergeStage`
3. `IncludeExpansionStage`
4. `IncludeResolveStage`
5. `ListingItemInfoStage`
6. **`DocumentProfileStage`** ← profile checkpoint
7. `LinkResolutionStage`
8. **`UnwrapProfileStage`** ← exit profile checkpoint
9. `PreEngineSugaringStage`
10. **`EngineExecutionStage`** ← loops the engine sequence here (PR #238)
11. `CompileThemeCssStage`
12. `BootstrapJsStage` (native only)
13. `ClipboardJsStage` (native only)
14. `UserFiltersStage::pre()`
15. `AstTransformsStage` — Quarto built-in transforms (callouts, …)
16. `UserFiltersStage::post()`
17. `CodeHighlightStage`
18. `RenderHtmlBodyStage` — HTML body emission
19. `ApplyTemplateStage` — wrap with HTML template

Stages 1–17 are AST-level and nominally format-agnostic; stages 18–19
are HTML-specific. The pipeline list itself is hardcoded (option-based
feature flags exist but engines cannot mutate the list). See § Gap G3.

### Trace + WASM replay status (post PR #238)

`quarto-trace` now carries `TraceDocument.engine_captures: Vec<EngineCapture>`
with a back-compat read of the legacy single `engine_capture` field.
`EngineRegistry::with_replay_many` registers a `ReplayEngine` per
recorded engine name; the byte-equality miss policy is unchanged.

`CaptureSpliceStage` (`crates/quarto-core/src/stage/stages/capture_splice.rs`
on `feature/multi-engine`) holds `captures: Vec<EngineCapture>` and
folds them in order — engine N+1's splice runs on engine N's spliced
output. Each iteration still extracts only `result.markdown` from the
captured JSON; the comment in the new code still reads:

> We don't need the rest of the ExecuteResult shape here (filters,
> includes, supporting_files) — those are engine-side concerns the
> splice doesn't reproduce in the AST.

So **Gap G6 (capture-splice aux-field drop) is still present
post-PR-#238**, just now affecting a *sequence* of captures rather
than one. bd-cp3em remains valid.

### VFS

`crates/quarto-system-runtime/src/wasm.rs:227-261`. `project_root` is
`/project`. Write entry points are `wasm-bindgen` exports — JS → WASM
only. No internal Rust API for pipeline producers. See § Gap G2.

### Filter resolution

`crates/quarto-core/src/filter_resolve.rs` resolves filters from YAML
`filters:` document metadata only; never consults
`ExecuteResult.filters`. Verified by workspace-wide grep on `main`;
not changed by PR #238. See § Gap G1.

## Gaps identified by the design exercise

These are pre-existing gaps in the codebase that the mermaid engine
forces us to confront. Each is a beads issue (see § Beads issues).

### G1. `ExecuteResult.filters` is captured but never consumed

Knitr writes `vec!["rmarkdown/pagebreak.lua"]`
(`crates/quarto-core/src/engine/knitr/mod.rs:215`); no pipeline stage
reads it. Tracking: **bd-14rer**.

**Mermaid-specific consequence:** If we wanted to implement mermaid
handling as a Lua filter declared via `ExecuteResult.filters`, that
path is dead until G1 is fixed. With the engine-impl approach (post
PR #238) we don't take this path, so it's no longer on our critical
path — just a real bug Knitr suffers from today.

### G2. Engines cannot publish virtual files to the VFS

Tracking: **bd-s8llm**. Not on the mermaid critical path under the
engine-impl approach (we don't need to materialize any files).

### G3. No mechanism for engines to declare pipeline stages

Tracking: **bd-mqk49**. **More relevant after PR #238**, not less:
with engines locked in as first-class citizens, the natural way to
do "format-agnostic engine output + format-specific emission" (Q-B)
is to let the engine declare a per-format AST pass on its output. We
likely can ship mermaid without this (Q-B option B2c — a fixed stage
in the canonical list), but bd-mqk49 is the architecturally correct
direction for the class of engine.

### G6. Capture-splice path drops aux ExecuteResult fields

Tracking: **bd-cp3em**. Verified still present in
`feature/multi-engine`'s `capture_splice.rs`. The fix is independent
of multi-engine work.

**Mermaid-specific consequence:** A mermaid engine that emits the
jsdelivr `<script>` via `ExecuteResult.includes` would work on
`q2 render` but silently fail on `q2 preview` replay. The C1 strategy
(inline `RawBlock` at end of body, encoded into the engine's
`markdown` output) dodges this entirely because `markdown` *is*
preserved through the splice.

## Design questions to resolve in this session

### Q-A. What kind of "thing" is mermaid? — **RESOLVED**

**Resolved by PR #238: mermaid is an `ExecutionEngine` impl, used in
a sequence `engine: [knitr, mermaidjs]` (or `engine: [mermaidjs]` for
mermaid-only docs).** Engines compose because cell-class ownership
sets are disjoint (`{r}` vs `{mermaid}` never overlap). The
"per-block dispatch" framing was a red herring — each engine
internally decides which cells it owns.

This obsoletes options A3/A4 (AST transform) from v1 of this plan.

### Q-B. Where does the format-agnostic vs HTML-specific split land?

Two genuine options remain, both compatible with the engine-impl
approach:

- **B1.** The engine emits a `RawBlock(HTML, "<pre class=\"mermaid\">…</pre>")`
  directly. The engine itself becomes format-aware. Simplest. When
  PDF lands, the engine has to grow a format switch.
- **B2.** The engine emits a Pandoc `Div` with class `mermaid`
  wrapping the source as a `CodeBlock`. A format-specific step turns
  that marker into `<pre class="mermaid">…</pre>` for HTML output.
  Three sub-options for where that step lives:
  - **B2a.** Special-case `Div.mermaid` in the HTML writer. Couples
    the writer to one engine's marker name; ugly precedent.
  - **B2c.** A dedicated `MermaidHtmlEmitStage` between
    `AstTransformsStage` and `RenderHtmlBodyStage`, gated by
    HTML-output. Mermaid-specific code in the canonical pipeline,
    but architecturally clean.
  - **B2e.** Engine declares a per-format post-processing AST pass.
    Requires the engine→stage extension API in bd-mqk49. Strictly
    the right shape if we expect more engines like this
    (graphviz, plantuml, dot, …).

**Recommendation for the first ship (decided 2026-05-28):** **B1 —
the hack.** The engine emits `RawBlock(HTML, …)` directly and we
ship. The architectural correctness lives in two follow-up signals:

- A comment in the mermaid engine source pointing at **bd-mqk49**
  ("when the engine→stage extension API lands, route this through a
  per-format pass instead of emitting HTML directly").
- A note on **bd-mqk49** itself that the mermaid engine is a known
  beneficiary of the API and should be refactored when bd-mqk49
  ships.

This is the honest small-scope move — Quarto 2 only ships HTML today
anyway, so B2c's format-agnostic invariant is being maintained
hypothetically against a future cost. B1 + linked TODO captures the
intent without blocking shipping.

### Q-C. How does the script include reach the document?

- **C1.** The engine emits a once-per-doc `RawBlock(HTML, "<script type=\"module\">…</script>")`
  at the end of the body, inline in its `markdown` output. Survives
  the capture-splice path because `result.markdown` is the only
  field the splice preserves.
- **C2.** The engine populates `ExecuteResult.includes`
  (`include_after_body`). Cleaner on native but **silently lost on
  q2 preview replay** until bd-cp3em is fixed.
- **C3.** Engine returns the include in `ExecuteResult.includes`
  *and* we fix bd-cp3em as part of this work. Cleanest. Adds scope.

**Recommendation for the first ship:** C1. Zero new infrastructure;
correct on both `q2 render` and `q2 preview` from day one;
debuggable in the AST. C2/C3 are a follow-up after bd-cp3em.

### Q-D. WASM replay coverage

PR #238's bd-iq0hp documents that a browser E2E of *multi-engine*
preview was not possible at PR time — the default registry uses real
engines, `FixtureEngine` is test-only, and knitr/jupyter don't compose
cleanly. **Our preview-e2e task (bd-my0o5) closes bd-iq0hp**: mermaid
+ knitr is the first real-engine pair that composes cleanly because
cell-class ownership doesn't overlap.

Test matrix:

1. Document with mermaid block only — `q2 preview` renders the
   diagram.
2. Document with `engine: [knitr, mermaidjs]`, knitr block + mermaid
   block, with recorded captures — both render in preview.
3. Edit the mermaid block — preview updates without re-running
   knitr's capture (capture invariance on engine 1's input).

## Proposed phased work

### Phase 0 — design ratification (this session)

- [x] v1 plan written
- [x] PR #238 surfaced; v2 revision applied
- [x] Decisions locked: A1 (engine impl), B1 (direct RawBlock HTML
      emission with bd-mqk49 follow-up TODO), C1 (inline script
      RawBlock), D=bd-iq0hp closure

### Phase 1 — multi-engine current-state audit

- [ ] Read `claude-notes/plans/2026-05-27-multi-engine-execution.md`
      (on `feature/multi-engine`) and confirm:
  - Where `MermaidEngine` would register in
    `EngineRegistry::register_default`-equivalent (native + WASM
    builds both need it — mermaid is pure-Rust, no subprocess, so
    the WASM build can register it too).
  - The per-engine FileId provenance scheme (`<stem>.<engine>.rmarkdown`)
    and what mermaid's intermediate name should be.
  - That `result.markdown` is the QMD-text re-parsed for the next
    engine — i.e. the mermaid engine should emit QMD, not Pandoc-JSON.
- [ ] Verify the new `capture_splice.rs` actually drops aux fields
      (re-confirm bd-cp3em is post-PR-#238 relevant) — already done
      in v2 revision, but spot-check at impl time.
- [ ] If PR #238's review surfaces design changes that affect mermaid,
      reflect them here.

### Phase 2 — `MermaidEngine` implementation (Q-B → B1, Q-C → C1)

- [ ] Add `MermaidEngine` implementing `ExecutionEngine` in
      `crates/quarto-core/src/engine/mermaid/` (mirror the
      `markdown.rs` shape — it's the closest precedent for a
      no-subprocess engine).
  - `name() == "mermaidjs"`.
  - `execute(input, ctx)`: parse `input` as QMD, walk for code cells
    with class `mermaid`, replace each cell with a
    `RawBlock(HTML, "<pre class=\"mermaid\">…</pre>")` (B1 —
    direct HTML emission), and append a once-per-doc
    `<script type="module">…</script>` `RawBlock` at end of body
    (C1). Serialize back to QMD and return as
    `ExecuteResult { markdown, ..Default }`.
  - **Add a source-code comment** at the RawBlock-emission site:
    `// bd-mqk49: when engines can declare per-format AST passes,
    // route this through a format-conditional transform instead of
    // emitting HTML inline. Today, Quarto 2 only renders HTML, so
    // the format-locked emission is acceptable.`
  - Register in native + WASM `EngineRegistry`s.
- [ ] Tests:
  - Unit: mermaid engine on a fixture qmd containing `{mermaid}` and
    non-mermaid blocks — only `{mermaid}` cells touched; script tag
    appended.
  - Pipeline: render a fixture qmd through the full HTML pipeline;
    HTML contains `<pre class="mermaid">…</pre>` and the script tag.
  - **End-to-end per CLAUDE.md**: `cargo run --bin q2 -- render fixture.qmd`,
    grep the actual output, record invocation + observed output in
    this plan before claiming done.

### Phase 3 — q2-preview verification (closes bd-iq0hp)

- [ ] Per the Q-D test matrix above. The fact that mermaid+knitr is
      the first cleanly-composing real-engine pair makes this work
      the canonical multi-engine browser preview E2E.

### Phase 4 — documentation

- [ ] User-facing docs page under `docs/`. Render with
      `cargo run --bin q2 -- render docs/` (Q2, not Q1).

### Follow-up issues (separate beads — not blockers for shipping
mermaid via A1/B2c/C1)

- **G1** (bd-14rer): wire `ExecuteResult.filters` into resolver, or
  remove the field. Knitr currently drops `rmarkdown/pagebreak.lua`
  silently.
- **G2** (bd-s8llm): VFS publication API for pipeline producers.
- **G3** (bd-mqk49): engine→pipeline stage extension API. Upgrades
  Q-B from B2c (dedicated stage) to B2e (engine-declared stage).
- **G6** (bd-cp3em): make capture-splice surface `includes` /
  `filters` / `supporting_files`. Once landed, C1 → C2/C3 becomes
  the cleaner mermaid implementation.

## Open questions

1. Should the jsdelivr URL be configurable per-document
   (`mermaid: { src: ..., version: 11 }`)? Defer.
2. Should `mermaid.initialize({...})` options be configurable
   (theme, fontFamily, …)? Defer.
3. Does WASM build need to register `MermaidEngine` separately, or
   does the engine being subprocess-free let us share a registration?
   Resolve at impl time; expected: yes, share.
4. Where in the canonical pipeline does `MermaidHtmlEmitStage` slot?
   After `AstTransformsStage` (15)? Between user-filters-post (16)
   and code-highlight (17)? Resolve at impl time based on whether
   user filters should see the marker `Div` or the
   `<pre class="mermaid">`.

## Beads issues

Created 2026-05-28 (v1), revised 2026-05-28 (v2 after PR #238 review):

- **`bd-je48v`** (epic, P2) — "Add mermaid diagram engine to Quarto 2."
- **`bd-c6h96`** (task, P2, child of epic) — "Mermaid engine design
  session." v2: Q-A resolved (engine sequence via PR #238); Q-B
  sharper (B1 vs B2a/B2c/B2e); Q-C unchanged (C1 recommended); Q-D
  redirected at closing bd-iq0hp.
- **`bd-fztki`** (task, P2, child of epic, blocks impl) — "Audit
  current multi-engine state." v2: retargeted at PR #238's plan +
  code instead of a generic multi-engine investigation.
- **`bd-gwfdo`** (task, P2, child of epic, blocked-by `bd-c6h96` +
  `bd-fztki`, gated on PR #238 merge) — "Implement `MermaidEngine`."
  v2: revised from "AST transform" to "ExecutionEngine impl emitting
  RawBlock HTML directly (B1)." A bd-mqk49 follow-up will refactor
  to format-conditional emission once the engine→stage extension
  API exists.
- **`bd-my0o5`** (task, P2, child of epic, blocked-by `bd-gwfdo`) —
  "q2 preview end-to-end verification for mermaid blocks." v2:
  explicitly closes PR #238's bd-iq0hp.
- **`bd-5ijtt`** (task, P3, child of epic, blocked-by `bd-gwfdo`) —
  "User-facing docs for mermaid diagrams."

Follow-up issues (discovered-from `bd-c6h96`):

- **`bd-14rer`** (bug, P2) — Knitr's `filters` silently dropped.
- **`bd-s8llm`** (feature, P3) — VFS publication API.
- **`bd-mqk49`** (feature, P3) — Engine→stage extension. **More
  relevant after PR #238**; the natural mechanism for Q-B's full
  B2e form. The mermaid engine ships under B1 (direct
  `RawBlock(HTML, …)` emission); when bd-mqk49 lands, the mermaid
  engine should be refactored to declare a per-format AST pass
  instead. A source-code comment in `engine/mermaid/` flags the
  TODO.
- **`bd-cp3em`** (bug, P2) — Capture-splice aux-field drop;
  verified still present in `feature/multi-engine`.

## References

- ExecutionEngine trait: `crates/quarto-core/src/engine/traits.rs:56-127`
- ExecuteResult: `crates/quarto-core/src/engine/context.rs:127-206`
- EngineExecutionStage (single-engine, on `main`):
  `crates/quarto-core/src/stage/stages/engine_execution.rs:150-401`
- EngineExecutionStage (multi-engine, on `feature/multi-engine`):
  per PR #238 file list (+625/-232)
- Capture-splice aux-field drop (on `feature/multi-engine`):
  `crates/quarto-core/src/stage/stages/capture_splice.rs` —
  comment unchanged from `main` version
- Pipeline stages: `crates/quarto-core/src/pipeline.rs`
- Document profile contract: `claude-notes/designs/document-profile-contract.md`
- Filter resolution: `crates/quarto-core/src/filter_resolve.rs`
- VFS: `crates/quarto-system-runtime/src/wasm.rs:227-261`
- q2-preview epic: `claude-notes/plans/2026-05-11-q2-preview-epic.md`
- Multi-engine plan (on `feature/multi-engine`):
  `claude-notes/plans/2026-05-27-multi-engine-execution.md`
- Multi-engine PR: https://github.com/quarto-dev/q2/pull/238
