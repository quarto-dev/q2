# Sequential multi-engine execution

**Issue:** bd-5yff4 — feature/design.

**Status:** design / investigation. Implementation is gated on explicit
user go-ahead after this plan is reviewed and iterated.

## Overview

Today a Quarto 2 document declares exactly one execution engine:

```yaml
engine: knitr
```

This plan investigates running **several engines in sequence** for one
document:

```yaml
engine:
  - knitr
  - mermaidjs   # hypothetical future diagram engine
```

The two coupled pieces of work are (1) a YAML-config change to accept an
ordered array of engines, and (2) a pipeline change to thread N engines
through the engine-execution stage. A third concern — tracing, replay,
and preview — must be redesigned because today's model assumes a single
engine per document.

### Decisions locked with the user (2026-05-27)

1. **Engines are distinct within the sequence; order is significant.**
   The same engine never appears twice. Ordering matters because engine
   N may emit code cells that engine N+1 consumes. → Replay captures can
   stay **keyed by engine name** (the current registry model), but the
   execution order and per-engine input must be preserved.
2. **Array merge uses the existing default (`!concat`).** We do *not*
   introduce an engine-specific default merge op (e.g. `!prefer`).
   Schema-specific tag defaults are deferred until we have a genuine
   schema-driven merging system. Users opt into replacement with an
   explicit `!prefer` tag.
3. **Validate with a simple file-backed test engine**, not a real second
   engine. The test engine reads a results file (per-cell outputs, in
   order) and splices them in — deterministic, dependency-free, and a
   small useful design in its own right. A real second engine (e.g.
   mermaidjs) is a separate follow-up.
4. **Trace records one snapshot per engine.** One AST snapshot **and**
   one `EngineCapture` per engine invocation within the stage, mirroring
   the existing `transform:<name>` sub-entry pattern — provided the
   trace-format change stays tractable (it should; see §4).

## Current architecture (what we're changing)

### Engine detection — `crates/quarto-core/src/engine/detection.rs`

`detect_engine(metadata) -> DetectedEngine` returns a **single** engine:

```rust
pub struct DetectedEngine {
    pub name: String,                 // singular
    pub config: Option<ConfigValue>,
}
```

It handles three input shapes (`detection.rs:150-190`):
- `engine: knitr` (string) → `DetectedEngine::new("knitr")`
- `engine: { jupyter: { kernel: python3 } }` (map) → takes
  `entries.first()` — the **first** key only
- top-level `jupyter:`/`knitr:` key (no `engine:` key)
- defaults to `markdown`

There is **no array branch**. An `engine: [knitr, mermaidjs]` would fall
into the map branch, find no `.first()` map entry on a sequence, and fall
through to `markdown`.

### Engine execution — `crates/quarto-core/src/stage/stages/engine_execution.rs`

`EngineExecutionStage::run` (`engine_execution.rs:150-401`) does, once:

1. `detect_engine(&doc_ast.ast.meta)` (single engine).
2. Markdown engine → early return, passthrough (`:191-198`).
3. `serialize_ast_to_qmd(&doc_ast.ast)` → `(qmd, source_info)` (`:201`).
4. `engine.execute(&qmd, &exec_context)` → `ExecuteResult` (`:230`).
5. Emit `EngineCapture` aux event (`:248-270`).
6. Accumulate `includes` + `supporting_files` onto `ctx` (`:273-297`).
7. Parse engine output back to AST against an intermediate filename
   `<stem>.rmarkdown` (`:313-324`).
8. Build a **2-slot** merged `ASTContext`: slot 0 = original `.qmd`
   (`FileId(0)`), slot 1 = intermediate `.rmarkdown` (`:326-362`).
9. Remap executed-AST `FileId(0)` → `FileId(1)` (`:368-370`).
10. `quarto_ast_reconcile::reconcile(doc_ast.ast, executed_ast)` (`:376`).

The serialize → execute → parse → reconcile loop is **already
idempotent in type**: it consumes a `DocumentAst` and produces a
`DocumentAst`. Threading N engines is mechanically a `for` loop over that
body. The non-trivial part is the FileId/slot bookkeeping (step 8–9) and
the trace/replay model (steps 5).

### Engine registry — `crates/quarto-core/src/engine/registry.rs`

`EngineRegistry` is a `HashMap<String, Arc<dyn ExecutionEngine>>` keyed by
`engine.name()`. `register` is last-write-wins. `with_replay(capture)`
(`registry.rs:157`) registers a single `ReplayEngine` under the recorded
engine's name. Because names are the key and our sequences are
name-distinct, registering one `ReplayEngine` per engine works — but the
constructor only takes one capture today.

### Engine trait — `crates/quarto-core/src/engine/traits.rs`

```rust
pub trait ExecutionEngine: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, input: &str, ctx: &ExecutionContext)
        -> Result<ExecuteResult, ExecutionError>;
    fn can_freeze(&self) -> bool { false }
    fn intermediate_files(&self, _input_path: &Path) -> Vec<PathBuf> { Vec::new() }
    fn is_available(&self) -> bool { true }
}
```

The trait is **per-invocation text→text** and needs no change to support
sequencing — the *stage* drives the loop, not the engine.

### Metadata merge + tags — `crates/quarto-config/`

`!prefer` / `!concat` are parsed in `quarto-config/src/tag.rs:135-181`.
The merge in `quarto-config/src/merged.rs:286-327` walks config layers
lowest→highest; arrays **concatenate by default** (`MergeOp::Concat`),
and `MergeOp::Prefer` clears prior items. `MetadataMergeStage`
(`crates/quarto-core/src/stage/stages/metadata_merge.rs:133`) assembles
the layers (project → extension → directory → document → runtime) and
materializes the merged config into `doc.ast.meta`.

→ **An `engine:` array Just Works under the existing array-merge
machinery.** No merge-engine change is required; we only need to *read*
the merged array and enforce distinctness (see §1, "Duplicate handling").

### Trace / replay / preview — the single-capture assumption

- `quarto-trace/src/lib.rs:111`: `TraceDocument.engine_capture:
  Option<EngineCapture>` — **one** slot.
- `EngineCapture` = `{ engine_name, input_qmd, result }`
  (`lib.rs:138-155`).
- Trace observer (`crates/quarto-core/src/stage/trace.rs:209-243`):
  `on_auxiliary_data` routes the `EngineCapture` kind to the single
  `engine_capture` slot. There is precedent for **intra-stage**
  granularity: `on_transform_data` (`trace.rs:185-207`) emits a
  `transform:<name>` `TraceEntry` per transform within
  `AstTransformsStage`.
- Replay engine (`crates/quarto-core/src/engine/replay.rs`): validates
  `input_qmd` byte-equality and returns the recorded `ExecuteResult`;
  hard-fails on mismatch.
- Preview record (`crates/quarto-core/src/engine/preview_record.rs`):
  `record_capture` runs the pipeline through `EngineExecutionStage`,
  collects the **first** `EngineCapture` (`first-write-wins`,
  `preview_record.rs:79-82`), returns `Option<EngineCapture>`.
- Preview cache (`crates/quarto-preview/src/cache.rs`): caches one
  capture per doc keyed by SHA-256 of the canonical input QMD; the
  browser-side WASM replays it.

All four hinge on "one engine ⇒ one capture." Multi-engine breaks the
assumption.

## Feasibility verdict

**Feasible.** The type contract closes end-to-end (each engine consumes
and produces a `DocumentAst`), the merge system already concatenates
arrays, and the trace model has a precedent for intra-stage sub-entries.
The genuine design work is in three places:

1. Reading + validating the engine array (distinctness, ordering).
2. Generalizing FileId/intermediate-slot bookkeeping from 2 → N+1.
3. Turning the single-capture trace/replay/preview path into an ordered
   list, with per-engine snapshots.

Each is addressed below, with the open design questions called out.

---

## 1. YAML config: `engine:` as an ordered array

### Accepted shapes (back-compat is mandatory)

| YAML | Parsed sequence |
|------|-----------------|
| `engine: knitr` | `[knitr]` |
| `engine: { jupyter: { kernel: python3 } }` | `[jupyter+config]` |
| `engine: [knitr, mermaidjs]` | `[knitr, mermaidjs]` |
| `engine:`<br>`  - knitr`<br>`  - mermaidjs: { theme: dark }` | `[knitr, mermaidjs+config]` |
| top-level `jupyter:` (no `engine:`) | `[jupyter+config]` |
| (none) | `[markdown]` |

Array elements may be a bare string or a single-key map (engine name →
config), matching the existing scalar/map forms element-wise.

### New API

Add `detect_engines(metadata) -> Vec<DetectedEngine>` alongside (or
replacing the internals of) `detect_engine`. Keep `detect_engine` as a
thin wrapper returning the first element for any remaining single-engine
call sites, or migrate all call sites — TBD during implementation.

### Merge behavior — verified against the code (2026-05-27)

`MetadataMergeStage` materializes the merged config via
`materialize_cursor` (`crates/quarto-config/src/materialize.rs:85`),
which calls `MergedCursor::as_value`
(`crates/quarto-config/src/merged.rs:238-256`). `as_value` walks layers
**highest-priority-first and returns on the first layer that defines the
key** — so the **topmost layer that sets `engine:` decides the kind**:

| Project (lower) | Doc (higher) | Materialized `engine` | Detected sequence |
|---|---|---|---|
| `[knitr]` | `jupyter` (scalar) | scalar `jupyter` (array dropped) | `[jupyter]` |
| `jupyter` (scalar) | `[knitr]` (array) | `[knitr]` (scalar dropped) | `[knitr]` |
| `[jupyter]` | `[jupyter]` | `[jupyter, jupyter]` (concat) | dup → see below |
| `[knitr]` | `[mermaidjs]` | `[knitr, mermaidjs]` (concat) | `[knitr, mermaidjs]` |

Two consequences, both confirmed by reading `as_array`
(`merged.rs:295-320`, which collects items **only** from layers whose
kind is `Array`, line 297):

1. **Duplicates arise only in the array + array case.** Scalar/array
   mismatches never produce a duplicate, because the topmost layer's
   kind wins and a mismatched lower layer is dropped wholesale. The only
   shape that repeats an engine is two array layers naming the same
   engine (e.g. project `engine: [jupyter]` and a doc that restates
   `engine: [jupyter]`).
2. **Gotcha to document:** because `as_array` drops lower *scalar*
   layers, the "concat accumulates across layers" intuition holds **only
   when every contributing layer uses array syntax.** A project-level
   `engine: jupyter` (scalar) is silently discarded the instant any
   higher layer writes `engine:` as an array. We surface this in user
   docs.

### Duplicate handling (resolved with user 2026-05-27)

**Dedup keeping the first occurrence**, and emit a diagnostic naming any
dropped duplicate. This fires only for the array + array repeated-engine
case above. Erroring would be hostile to the benign "two array layers
both say jupyter" case. The cost: a cross-layer *reordering* attempt
(project `[knitr, jupyter]` + doc `[jupyter, knitr]` → deduped
`[knitr, jupyter]`) is silently normalized to first-seen order;
reordering requires an explicit `!prefer` to replace the list. Documented
as a v1 limitation.

### Schema / validation

There is currently **no Rust-side schema** validating `engine:`
(detection-only; unknown names fall back with a warning at the stage).
We keep that posture: the array branch accepts any element shape and
defers unknown-engine handling to the stage's existing
`get_engine_with_fallback`. No new schema work in this plan.

---

## 2. Pipeline: thread N engines through `EngineExecutionStage`

### Loop shape

```text
engines = detect_engines(meta)           // ordered, distinct
ast = doc_ast.ast
slots = [".qmd"]                          // FileId(0)
for (i, engine) in engines.enumerate():
    if engine.name == "markdown": continue   // per-engine no-op skip
    (qmd, source_info) = serialize_ast_to_qmd(ast)
    emit pre-engine capture input (qmd)      // for trace/replay
    result = engine.execute(qmd, exec_ctx_for(engine, i))
    record EngineCapture { engine_name, input_qmd: qmd, result }
    accumulate includes + supporting_files
    executed_ast = parse(result.markdown, intermediate_name(i))
    merged_context.add_slot(intermediate_name(i))   // FileId(slots.len())
    remap executed_ast FileId(0) -> FileId(slots.len()-1 .. )   // see below
    (ast, plan) = reconcile(ast, executed_ast)
    emit per-engine AST snapshot (trace)     // engine:<name>
final DocumentAst { ast, ast_context: merged_context, ... }
```

### FileId / intermediate-slot bookkeeping (the hard part)

Today (`engine_execution.rs:326-370`): 2 fixed slots, executed AST's
`FileId(0)` remapped by `+1` to land in slot 1. For N engines we need
**N+1 slots** (original + one intermediate per non-markdown engine) and
the remap offset becomes "current slot count," not a constant `+1`.

Invariant to preserve: a block's `FileId` identifies its provenance —
the `.qmd` for kept blocks, the relevant intermediate for blocks first
introduced by engine k. We must verify that `quarto_ast_reconcile`'s
keep/replace/recurse decisions remain correct when the "original" side
of the reconcile already carries FileIds from multiple prior slots.

**Risk:** the second reconcile's "original" AST (output of the first
reconcile) mixes `FileId(0)` and `FileId(1)`. Remapping the second
engine's executed AST to `FileId(2)` must not collide. We add a dedicated
test (Phase 1) that runs two appending engines and asserts three distinct
FileIds land coherently.

### Markdown skip

Markdown engines anywhere in the sequence are individually skipped
(passthrough), preserving the current optimization. A sequence of only
markdown engines collapses to today's no-op fast path.

### Includes / supporting files

Already additive (`extend` / `add_engine_files`). Accumulating across
engines needs no change beyond running inside the loop.

### Determinism requirement (load-bearing for replay)

Engine k+1's `input_qmd` is the serialization of the AST *after* engine
k's reconcile. For replay to validate (byte-equality on `input_qmd`),
serialize→reconcile→serialize must be **deterministic** across runs. This
is already assumed by single-engine replay; we extend the assumption and
add a regression test that records a two-engine trace and replays it
byte-clean.

---

## 3. Test engine: file-backed cell-results splicer

Per decision 3 — a deterministic, dependency-free engine to exercise
sequencing. Proposed contract:

- **Name:** `fixture` (working name; bikeshed later).
- **Config:** `engine: { fixture: { results: results.json } }` — a path
  (relative to the doc) to an ordered list of cell results.
- **Behavior:** walk the input QMD's executable code cells in document
  order; for cell i, splice the i-th entry from the results file as the
  cell's output (as Quarto already represents engine output — e.g. an
  output block / div following the source). Cells beyond the results
  list pass through unchanged; surplus results are a diagnostic.

To exercise **engine→engine handoff** (engine A emits a cell that engine
B executes), the test fixtures use two registrations of the file-backed
engine under two distinct names (e.g. `fixture-a`, `fixture-b`), where
`fixture-a`'s results include a fenced `{fixture-b}` cell that
`fixture-b` then fills. This proves the "engine N produces cells for
engine N+1" requirement without any real runtime.

This belongs in `crates/quarto-core/src/engine/` next to `markdown.rs`
and `replay.rs`. **Registration is test-registry-only** (decision
2026-05-27): the engine is constructed in tests via
`EngineRegistry::register`, never wired into the default registry, and
never user-selectable in a real render.

**Not a freeze mechanism.** The file-backed engine resembles Quarto 1's
freeze, but Quarto 2's freeze will instead reuse the **trace** directly —
roughly "`engine: replay` as freeze": commit a trace file into the repo
and flag Quarto to replay its recorded `ExecuteResult` instead of
running the engine. So the test engine carries no freeze responsibility;
it exists purely to make multi-engine sequencing testable without R /
Python / Jupyter.

---

## 4. Trace / replay / preview redesign

### Trace schema (one capture + one snapshot per engine)

- `quarto-trace`: replace `engine_capture: Option<EngineCapture>` with
  **`engine_captures: Vec<EngineCapture>`** (ordered by execution).
  - Back-compat: readers fold a legacy single `engine_capture` into a
    one-element vec; writers emit the vec. Keep `#[serde(default,
    skip_serializing_if = "Vec::is_empty")]`.
  - Bump `SCHEMA_VERSION` 2 → 3 and document; readers stay
    forward/backward tolerant (unknown fields ignored).
- Per-engine AST snapshot: emit an `engine:<name>` `TraceEntry` after
  each engine's reconcile, **exactly mirroring** `on_transform_data` →
  `transform:<name>` (`trace.rs:185-207`). This is the precedent that
  makes "one snapshot per engine" *not* a big change: we add an
  `on_engine_data(name, index, ast, ast_context)` observer hook (or
  reuse the transform hook with an `engine:` prefix) and call it from the
  stage loop. The dedup pass (bd-5qnj) collapses identical snapshots, so
  the size cost is bounded.

### Observer / capture plumbing

- `engine_execution.rs` emits one `EngineCapture` aux event **per
  engine** (already a loop after the change). The `index` field
  distinguishes them.
- `JsonTraceObserver::on_auxiliary_data` (`trace.rs:215`) pushes onto the
  `engine_captures` vec instead of overwriting the single slot.

### Replay

- `EngineRegistry::with_replay(capture)` → `with_replay_many(captures:
  Vec<EngineCapture>)`: register one `ReplayEngine` per capture, keyed by
  `engine_name` (distinct names ⇒ no collision — decision 1).
- The stage loop drives each engine; each `ReplayEngine` validates its
  own `input_qmd`. Order is implied by the engine sequence in `meta`,
  which must match the recording.
- A replay miss on **any** engine surfaces as a stage error (extend the
  existing single-engine behavior, `replay.rs`).

### Preview (`q2 preview`) — in scope

`q2 preview` support ships **with** this feature, not after it. The
preview flow has two parts; both are in scope:

1. **Capture the sequence + replay in the browser** (the part that makes
   preview *correct* for multi-engine docs):
   - `record_capture` (`preview_record.rs:130`) returns
     `Vec<EngineCapture>`; `CaptureCollector` collects **all** captures
     in order instead of first-write-wins (`preview_record.rs:79`).
   - The WASM hub-client registers the ordered captures via
     `with_replay_many` and replays the sequence so the browser render
     matches the server render.

2. **Incremental capture cache** (the part that makes preview *fast*
   across edits — `quarto-preview/src/cache.rs`'s `record_capture_cached`
   keys a capture by SHA-256 of the canonical input QMD so unchanged code
   cells don't re-run the engine). For a sequence this becomes:
   - store/serve the **ordered vec** of captures per doc;
   - staleness: engine 1's input is the doc's canonical QMD (today's
     key); engines 2..N consume *derived* inputs (each is a
     deterministic function of the prior engine's reconciled output), so
     a single key on the doc's canonical QMD invalidates the whole
     sequence correctly. We confirm this during implementation; if a
     finer per-engine input vector is needed it's a localized change.

   ("Cache sequencing" earlier referred to *this* incremental
   optimization — not to preview support itself.)

### "Overwhelmingly complex?" assessment

No. The vec-ification of `engine_capture` is additive + a schema bump;
the per-engine snapshot reuses the `transform:<name>` mechanism
verbatim; `with_replay_many` is small; the preview collector change is
collecting a vec instead of first-write-wins. The only piece with any
unknowns is the incremental-cache staleness model for sequences, and
even there the deterministic-derivation argument suggests the existing
doc-keyed invalidation already covers it.

---

## Phased work plan (TDD)

> Phases are ordered so each ends green. No implementation starts until
> the user approves this plan.

### Phase 0 — Test scaffolding
- [ ] Design + land the **file-backed test engine** (`fixture`) with unit
      tests (reads results file, splices per-cell outputs in order;
      surplus/missing-result diagnostics). **Test-registry-only** — never
      wired into the default registry.
- [x] Duplicate-handling policy: **dedup keeping first occurrence +
      diagnostic** (resolved with user; only fires for array+array
      repeated engine).
- [x] Test-engine registration scope: **test-registry-only** (resolved
      with user; Q2 freeze will use trace-replay, not this engine).

### Phase 1 — Pipeline threading (single → N engines)
- [ ] **Failing test first:** two-engine fixture (`fixture-a` emits a
      `{fixture-b}` cell that `fixture-b` fills); assert final AST
      contains both engines' outputs in order.
- [ ] `detect_engines() -> Vec<DetectedEngine>` with the array branch +
      back-compat for string/map/top-level forms. Unit tests per shape.
- [ ] Generalize `EngineExecutionStage::run` to loop; generalize the
      merged `ASTContext` to N+1 slots and the FileId remap offset.
- [ ] **FileId coherence test:** two appending engines ⇒ three distinct,
      non-colliding FileIds with correct provenance.
- [ ] Post-merge duplicate dedup + diagnostic (per Phase 0 decision).
- [ ] Markdown-in-sequence skip; all-markdown fast path preserved.

### Phase 2 — YAML merge integration
- [ ] Tests: `engine:` array merges via `!concat` default across
      project/dir/doc layers; `!prefer` replaces; duplicates deduped.
- [ ] End-to-end: a fixture project with layered `_quarto.yml` /
      `_metadata.yml` / front-matter engine arrays renders with the
      expected resolved sequence.

### Phase 3 — Trace / replay redesign
- [ ] `quarto-trace`: `engine_captures: Vec<EngineCapture>`, schema bump
      to 3, reader back-compat (fold legacy single → vec). Round-trip
      tests (v2 read, v3 read/write).
- [ ] Per-engine `engine:<name>` AST snapshot via the transform-style
      hook; trace shows one entry per engine.
- [ ] `with_replay_many`; two-engine record→replay byte-clean regression
      test (the determinism invariant from §2).

### Phase 4 — Preview integration (`q2 preview`, in scope)
- [ ] `record_capture` → `Vec<EngineCapture>`; collector keeps order.
- [ ] hub-client WASM registers the ordered captures (`with_replay_many`)
      and replays the sequence; browser render matches server render.
- [ ] Incremental capture cache stores/serves the ordered vec; confirm
      doc-keyed staleness invalidates the whole sequence (deterministic
      derivation argument, §4).
- [ ] Verify in a real browser session per CLAUDE.md end-to-end policy;
      if a browser isn't available, say so explicitly rather than
      inferring success.

### Phase 5 — End-to-end verification
- [ ] `cargo run --bin q2 -- render <multi-engine fixture>.qmd` with the
      file-backed engines; inspect output; record the invocation +
      observed markup in this plan (per CLAUDE.md E2E policy).
- [ ] `cargo nextest run --workspace`; `cargo xtask verify` (full,
      since `quarto-core` / `quarto-trace` / `quarto-pandoc-types` feed
      the WASM leg).
- [ ] Re-confirm trace/replay/preview parity with the single-engine path.

### Phase 6 — Commit (await explicit push approval per CLAUDE.md)

## Resolved with the user (2026-05-27)

1. **Duplicate handling** (§1): dedup keeping first occurrence +
   diagnostic. Verified this only ever fires for the array+array
   repeated-engine case; scalar/array mismatches can't produce a
   duplicate.
2. **Test-engine registration** (§3): test-registry-only. Q2 freeze will
   be trace-replay-based ("`engine: replay` as freeze"), so the test
   engine has no freeze role.
3. **Real second engine:** mermaidjs / any concrete engine is **out of
   scope** here; separate follow-up.
4. **Preview** (§4): `q2 preview` support ships **with** this feature.
   "Cache sequencing" referred only to the incremental capture-cache
   optimization, not to preview support; both are in scope, with the
   incremental cache the only piece carrying minor unknowns.

## Remaining open questions

_None blocking. Surface during implementation if the FileId-coherence
reconcile (§2) or the incremental-cache staleness model (§4) turns out
harder than the analysis suggests._

## Out of scope

- A real second engine implementation (mermaidjs etc.) — follow-up.
- Parallel engine execution (engines run strictly in sequence).
- Schema-driven / key-specific merge-tag defaults — deferred until a
  schema-driven merging system exists (user decision 2).
- Cross-document engine sequencing concerns beyond a single doc.

## Key source references

- Engine detection: `crates/quarto-core/src/engine/detection.rs:150`
- Engine stage: `crates/quarto-core/src/stage/stages/engine_execution.rs:150`
- Registry / replay seam: `crates/quarto-core/src/engine/registry.rs:157`
- Engine trait: `crates/quarto-core/src/engine/traits.rs`
- Replay engine: `crates/quarto-core/src/engine/replay.rs`
- Preview record: `crates/quarto-core/src/engine/preview_record.rs:130`
- Preview cache: `crates/quarto-preview/src/cache.rs`
- Trace schema: `crates/quarto-trace/src/lib.rs:91` (`TraceDocument`),
  `:138` (`EngineCapture`)
- Trace observer + `transform:<name>` precedent:
  `crates/quarto-core/src/stage/trace.rs:185` (`on_transform_data`),
  `:209` (`on_auxiliary_data`)
- Merge tags: `crates/quarto-config/src/tag.rs:135`
- Array merge: `crates/quarto-config/src/merged.rs:286`
- Metadata merge stage:
  `crates/quarto-core/src/stage/stages/metadata_merge.rs:133`
- Pipeline builder + stage order:
  `crates/quarto-core/src/pipeline.rs:242` (engine stage at `:288`)
