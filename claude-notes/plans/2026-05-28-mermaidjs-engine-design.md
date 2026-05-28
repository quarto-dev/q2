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
- [x] User ratified plan v2.1 (2026-05-28); bd-c6h96 closed

### Phase 1 — multi-engine current-state audit

- [x] Read `claude-notes/plans/2026-05-27-multi-engine-execution.md`
      (on `feature/multi-engine`) and confirm:
  - [x] Where `MermaidEngine` would register
  - [x] The per-engine FileId provenance scheme and mermaid's
        intermediate name
  - [x] That `result.markdown` is QMD-text re-parsed for the next
        engine
- [x] Re-verify `capture_splice.rs` drops aux fields — confirmed
      against `feature/multi-engine` (bd-cp3em remains valid)
- [ ] If PR #238's review surfaces design changes that affect mermaid,
      reflect them here (deferred until #238 merges or stabilizes)

#### Phase 1 findings (2026-05-28)

Pulled from `feature/multi-engine` (PR #238 head `ed9cbbfe`) and the
multi-engine plan `claude-notes/plans/2026-05-27-multi-engine-execution.md`.

**F1. Registration site is `EngineRegistry::new`
(`crates/quarto-core/src/engine/registry.rs:48-66`).** The current
shape is "always-register markdown; native-only register knitr +
jupyter":

```rust
registry.register(Arc::new(MarkdownEngine::new()));

#[cfg(not(target_arch = "wasm32"))]
{
    registry.register(Arc::new(KnitrEngine::new()));
    registry.register(Arc::new(JupyterEngine::new()));
}
```

**Mermaid registers in the always-block alongside `MarkdownEngine`.**
Mermaid is text-only (no subprocess, no R/Python runtime), so the
same code can run in native + WASM. This is **strictly better than
the trace-replay route** for q2 preview: the WASM build executes
mermaid live in-browser, no recorded capture needed, and bd-cp3em's
aux-field-drop never bites mermaid because mermaid never executes
on the native side to need replaying. (Native render still uses the
local engine the same way; both paths converge on the same engine
code.)

**F2. `KNOWN_ENGINES` const
(`crates/quarto-core/src/engine/detection.rs:31`)** is
`&["markdown", "knitr", "jupyter"]`. Used to gate the top-level
config shortcut (`jupyter: { kernel: python3 }` instead of
`engine: jupyter`). **Add `"mermaidjs"` to this list** so a
top-level `mermaidjs: { ... }` is recognized when no `engine:` is
declared. The `engine: [..., mermaidjs, ...]` path works regardless;
this is the nice-to-have for the bare top-level form.

**F3. Per-engine FileId provenance is per-loop-iteration in the
stage, not per-engine.** `EngineExecutionStage::run` walks each
engine in `to_run` and appends one intermediate slot per executed
engine to `merged_context`. The intermediate filename is
`<stem>.<engine>.rmarkdown` (per the multi-engine plan §2). **Mermaid
gets `<stem>.mermaidjs.rmarkdown` automatically** — no engine-side
participation required. `MermaidEngine::execute` only needs to
return `ExecuteResult { markdown, ..Default }`; the loop does the
rest.

**F4. `result.markdown` is QMD text re-parsed for the next engine.**
Confirmed at `engine_execution.rs:255` (multi-engine version):

```rust
let (qmd, qmd_source_info) = serialize_ast_to_qmd(&ast)?;
// ...
let mut result = engine.execute(&qmd, &exec_context)?;
// later: parse result.markdown as the next iteration's input
```

So mermaid emits QMD text. Literal HTML in QMD (e.g.
`<pre class="mermaid">…</pre>` on its own lines, blank-separated)
parses as `RawBlock(HTML, …)` via pampa's QMD reader (Pandoc
convention). No need to emit `\`\`\`{=html}` raw-block fences
unless we hit an edge case during impl.

**F5. In-process engine convention is *text-level* fence scanning,
not AST parse-walk-serialize.** The biggest finding of the audit.
`FixtureEngine` (`crates/quarto-core/src/engine/fixture.rs:120-250`,
new in PR #238) is the only pure-Rust engine on the multi-engine
branch and it works text-level: a hand-rolled fence scanner finds
`{name}` cells and splices replacement text in. **Mermaid should
mirror this** rather than go through pampa's parser:

- Simpler. ~100 lines of text-walking vs. AST manipulation +
  `serialize_ast_to_qmd` (which is private to `engine_execution.rs`).
- Cheaper. No round-trip through the parser, no AST allocation.
- Less coupled. The mermaid engine never touches pampa internals or
  Pandoc AST types; only `ExecuteResult`/`ExecutionContext`/`ExecutionError`.
- Matches the multi-engine plan's design pattern. Future graphviz/
  plantuml/dot engines would all follow the same template.

The cell shape mermaid matches is exactly the FixtureEngine pattern:
opening fence `` ```{mermaid} `` ... source text ... closing fence
`` ``` ``. Replacement text is the literal HTML for the `<pre class="mermaid">`
wrapper, plus (once per document) the jsdelivr `<script>` block
appended after the document body.

**F6. The script-tag include is emitted in the engine's `markdown`
output, NOT via `ExecuteResult.includes`.** This is the C1 decision
from the plan, but F5 makes it natural: mermaid is text-level so it
can simply append the `<script>` block to its returned markdown.
Survives capture-splice (which preserves `result.markdown`),
survives the engine sequence (HTML in QMD round-trips through
parse-and-reserialize), and reaches the HTML writer as a literal
`RawBlock(HTML)`.

**F7. `EngineExecutionStage` resolves engines via
`get_engine_with_fallback`** (multi-engine version). Unknown names
fall back to markdown with a warning. So even before mermaid lands,
`engine: [knitr, mermaidjs]` doesn't crash — it just warns and
no-ops on mermaidjs. This means landing the mermaid engine is
**purely additive**: it changes the behavior of `mermaidjs` from
"warn + skip" to "actually transform mermaid cells."

**F8. Capture-splice path drops aux fields per-iteration in the
fold.** Re-verified: `feature/multi-engine`'s
`crates/quarto-core/src/stage/stages/capture_splice.rs` still reads
`result.markdown` only and comments "filters, includes,
supporting_files — those are engine-side concerns the splice
doesn't reproduce in the AST." So bd-cp3em is post-PR-#238 valid.
Mermaid's C1 strategy (in-markdown script tag) doesn't depend on
this; but note that **the WASM build registers MermaidEngine
directly** (F1), so the splice path never replays mermaid anyway —
mermaid just runs live in-browser.

#### Phase 1 → Phase 2 implications

- Phase 2 step 1 ("mirror `markdown.rs` shape") refines to "mirror
  `fixture.rs` shape" — both are pure-Rust in-process engines, but
  fixture is the closer template for cell-scanning behavior.
- Phase 2 should add `"mermaidjs"` to `KNOWN_ENGINES`
  (`detection.rs:31`) for the top-level-shortcut form.
- Phase 2 should register `MermaidEngine` in the always-block of
  `EngineRegistry::new` so it's available in both native and WASM.
- Phase 2 does NOT need to publicize `serialize_ast_to_qmd` or
  touch pampa — text-level scanner is sufficient.

### Phase 2 — `MermaidEngine` implementation (Q-B → B1, Q-C → C1)

Refined after Phase 1 audit: mirror `fixture.rs` (text-level fence
scanner), register in always-block (native + WASM), add `"mermaidjs"`
to `KNOWN_ENGINES`.

- [x] Add `MermaidEngine` in
      `crates/quarto-core/src/engine/mermaid.rs` — text-level fence
      scanner, B1 emission, once-per-doc script append, HTML-escaped
      source, bd-mqk49 TODO comment at the emission site.
- [x] Register in `EngineRegistry::new` always-block
      (`crates/quarto-core/src/engine/registry.rs:52-67`).
- [x] Add `"mermaidjs"` to `KNOWN_ENGINES`
      (`crates/quarto-core/src/engine/detection.rs:31`).
- [x] Module wiring: export `MermaidEngine` from `engine/mod.rs`;
      doc-comment table updated for the new "always-available"
      mermaidjs row.
- [x] Unit tests in `engine/mermaid.rs` (12 tests, all passing):
      `name_is_mermaidjs`, `always_available`,
      `single_cell_emits_pre_and_script`,
      `multiple_cells_share_one_script`, `no_cells_means_no_script`,
      `other_engine_cells_pass_through`,
      `does_not_match_inside_other_fenced_blocks`,
      `html_escapes_lt_gt_amp_in_source`,
      `errors_on_unterminated_mermaid_cell`,
      `unterminated_non_mermaid_fence_is_passthrough`,
      `longer_fences_round_trip`, `script_appended_only_once_after_body`.
- [x] Full `quarto-core` test suite passes (2170 tests); full
      workspace passes (9496 tests). No regressions from
      `KNOWN_ENGINES` change.
- [x] Integration test in `crates/quarto-core/tests/mermaid_pipeline.rs`
      — 4 tests routed through `render_to_file` (the same path
      `q2 render` uses): single-doc emits pre+script, no-cells
      omits script, multiple cells share one script, array engine
      form works. All passing.
- [ ] Multi-engine integration: a fixture with `engine: [knitr,
      mermaidjs]` containing one `{r}` cell and one `{mermaid}`
      cell — both render correctly (gated on the knitr R runtime
      being available, or use the FixtureEngine pattern from PR #238
      to substitute). Deferred to a follow-up — single-doc and
      array-form coverage is sufficient for the first ship.
- [x] **End-to-end per CLAUDE.md** verification (recorded below).

#### Phase 2 finding: emission must be `\`\`\`{=html}`-fenced, not bare HTML

A finding the audit missed: pampa's QMD reader treats *bare* `<tag>`
markup at block position as a sequence of `RawInline` nodes — not a
block-level raw HTML element — and tries to parse the interior as
Markdown. That works for a `<pre>` with simple content but breaks
hard on the script block: `mermaid.initialize({ startOnLoad: true });`
contains `:`, which the parser treats as a definition-list-like
construct and rejects with `unexpected character or token here`.

The fix is to emit the explicit Pandoc raw-block form so the reader
treats the whole thing as opaque raw HTML and skips Markdown parsing
inside it:

```text
```{=html}
<pre class="mermaid">
…HTML-escaped source…
</pre>
```
```

…and the same wrapping for the `<script>` include. Two engine unit
tests assert the structural property:

- `cell_is_pampa_raw_html_block`: each cell wraps in `` ```{=html} ``.
- `script_block_is_pampa_raw_html_block`: the script include wraps
  in `` ```{=html} `` preceded by a blank line.

The integration tests in `tests/mermaid_pipeline.rs` catch the same
class of bug at the rendered-HTML layer (asserting `<pre class="mermaid">`
and `mermaid.esm.min.mjs` survive end-to-end).

#### Phase 2 end-to-end verification record

Per CLAUDE.md's end-to-end policy.

**Invocation:**

```bash
cargo run --bin q2 -- render /tmp/mermaid-e2e/test.qmd
```

where `/tmp/mermaid-e2e/test.qmd` is:

````qmd
---
title: Mermaid engine end-to-end test
engine: mermaidjs
---

# Hello

Here is a mermaid diagram:

```{mermaid}
graph TD
A[Client] --> B[Load Balancer]
B --> C[Server1]
B --> D[Server2]
```

And here is another:

```{mermaid}
graph LR
X[input] --> Y[output]
```

Done.
````

**Observed output** (excerpted from `/tmp/mermaid-e2e/test.html` after
inspection — two pre wrappers and one script include, prose intact):

```html
<p>Here is a mermaid diagram:</p>
<pre class="mermaid">
graph TD
A[Client] --&gt; B[Load Balancer]
B --&gt; C[Server1]
B --&gt; D[Server2]
</pre>
<p>And here is another:</p>
<pre class="mermaid">
graph LR
X[input] --&gt; Y[output]
</pre>
<p>Done.</p>
<script type="module">
import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
mermaid.initialize({ startOnLoad: true });
</script>
```

This output was inspected at the file system; the rendered HTML is
exactly the markup the user's hub-client browser session would
receive, and the mermaid runtime would pick up both diagrams at
page load.

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
