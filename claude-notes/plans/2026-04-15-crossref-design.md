# Crossref Design for Quarto 2

**Beads Issue**: bd-jsbg (epic)
**Created**: 2026-04-15
**Status**: Design — iterating with user before implementation

---

## Implementation Status (updated 2026-04-16)

**Phases 0–3 complete.** Phase 4 (multi-file) remains (design only in this plan).

| Phase | Status | Commits |
|-------|--------|---------|
| 0 — Foundation | Done | `c5ce6bb8` |
| 1 — Floats, single file | Done | `309500f4`, `1bd90b69` |
| 2 — Block-level (theorems, proofs, callouts) | Done | `3280a446`, `926e3c11` |
| 3 �� Equations | Done | (pending commit) |
| 4 — Multi-file foundations | Not started (design only) | — |

### Key design validation

The `plain_data` triple (`ref_type`, `kind`, `identifier`) proved to be the right integration point. Four different custom node types (FloatRefTarget, Theorem, Callout, Equation) now flow through the same indexer, resolver, and render transforms with **zero type-specific code** in the indexer and resolver. The indexer extension for equations required only adding inline walking — the `has_crossref_plain_data` predicate and `index_custom_target` method are shared unchanged. Adding a new crossref-capable type is: populate three JSON fields in the sugaring transform, done.

### File inventory

```
crates/quarto-core/src/crossref/
├── mod.rs                    # Constants (FLOAT_REF_TARGET, THEOREM, PROOF, CROSSREF_RESOLVED_REF,
│                             #   TRACE_KIND_CROSSREF_INDEX), re-exports
├── index.rs                  # CrossrefIndex, CrossrefEntry, Order, PromisedId, HeadingRecord
├── registry.rs               # RefTypeRegistry, RefTypeDef, RefTypeSource, RegistryError
├── metadata.rs               # Read crossref.custom + crossref.ids from merged metadata
├── target.rs                 # crossref_target_view() — uniform read-only view over any
│                             #   crossref-capable CustomNode
├── codeblock_shorthand.rs    # Pre-engine code-block desugar (#| label: → Div scaffold)
└── roundtrip_tests.rs        # QMD serialize/parse round-trip guard for synthetic Div

crates/quarto-core/src/transforms/
├── theorem.rs                # TheoremSugarTransform (.theorem/.lemma/… → CustomNode("Theorem"))
├── proof.rs                  # ProofSugarTransform (.proof → CustomNode("Proof"))
├── float_ref_target.rs       # FloatRefTargetSugarTransform (Div/Figure → CustomNode("FloatRefTarget"))
├── equation_label.rs         # EquationLabelTransform (Span.quarto-math-with-attribute → CustomNode("Equation"))
├── crossref_index.rs         # CrossrefIndexTransform (walk AST+inlines, assign order, build index)
├── crossref_resolve.rs       # CrossrefResolveTransform (Cite → CustomNode("CrossrefResolvedRef"))
└── crossref_render.rs        # CrossrefRenderTransform (CustomNodes → Figure/Div/Span/Link for writer)

crates/quarto-core/src/stage/stages/
└── pre_engine_sugaring.rs    # PreEngineSugaringStage (registry build + metadata + shorthand desugar)

crates/quarto-core/tests/
└── crossref_fixtures.rs      # 27 qmd-level integration fixtures asserting over CrossrefIndex
```

### Pipeline order (normalization + crossref + finalization)

```
... existing transforms ...
CalloutTransform            ← now injects crossref triple when id matches
...
TheoremSugarTransform       �� runs BEFORE FloatRefTarget to prevent greedy float claim
ProofSugarTransform
FloatRefTargetSugarTransform
EquationLabelTransform      ← Span.quarto-math-with-attribute → Inline::Custom("Equation")
CrossrefIndexTransform      ← now walks both blocks AND inlines
CrossrefResolveTransform
... TOC phase ...
... AppendixStructureTransform ...
CrossrefRenderTransform     ← finalization: CustomNodes → writer-visible shapes
ResourceCollectorTransform
```

### Known gaps / tech debt for future sessions

1. **StageProvenance (P1) not yet implemented.** The design is in the plan and the SourceInfo variant is proposed, but synthetic Divs from code-block shorthand desugar don't yet carry `StageProvenance` source info linking back to the original `#| label:` line. Diagnostics pointing at synthetic nodes will show default source locations.

2. **Subfloats deferred.** Parent/child id assignment, nested numbering ("Figure 1a"), and `fig.subplots`-style engine output. Q1 reference: `parsefiguredivs.lua:41-60`. This is delicate and needs its own plan.

3. **Remark / Solution blocks.** `.remark` and `.solution` have built-in ref-type prefixes (`rem`, `sol`) and could be numbered like theorems, but Q1 treats them as proof-like (optional numbering). Currently they remain plain Divs — neither theorem nor proof sugar claims them.

4. **`crossref.ids` manifest post-engine validation.** The `PromisedId` entries are lifted and the registry is extended, but we do not yet verify after engine execution that every promised id was realized. The plan (D6, O6) says undeclared dynamic ids produce a diagnostic — that enforcement logic is not yet implemented.

5. **Multi-crossref ranges.** `@fig-a; @fig-b` in a single bracket currently resolves to the first id only. "Figures 1-2" style range rendering is deferred.

6. **Diagnostic source locations.** Metadata extraction errors and unresolved-ref warnings are plain-text `DiagnosticMessage::warning()` strings. They do not yet carry `SourceInfo` for ariadne-style rendering with file/line context. The `DiagnosticMessage` type supports it; the threading just isn't wired.

7. **Success criterion #2** (engine-blissful shorthand) is implemented structurally but only tested with the markdown engine passthrough. Real Jupyter/knitr execution hasn't been tested end-to-end with the pre-engine desugar.

8. **Caption short (`fig-scap` / `tbl-scap`).** The `caption_short` slot exists on FloatRefTarget but is never populated. The cell-option parser knows about `<reftype>-scap` (it's consumed), but the value isn't wired into the slot. Render code checks for it but the path is untested.

9. **Equation crossrefs (Phase 3).** ~~Completely untouched.~~ **Done.** pampa wraps `$$ ... $$ {#eq-xxx}` as `Span.quarto-math-with-attribute` containing `DisplayMath`. `EquationLabelTransform` converts this to `Inline::Custom("Equation")` with the crossref triple; `CrossrefIndexTransform` was extended to walk inlines; `CrossrefRenderTransform` renders equations as `Span(id) > Math(DisplayMath, text + \tag{N})` for MathJax numbering.

---

## Goals

1. Implement crossref functionality for **single-file projects** in Quarto 2.
2. Lay foundations that make **multi-file crossrefs** (books, websites) a natural extension, not a rewrite.
3. Fix Q1 architectural issues that were path-dependent on the TS+Pandoc split:
   - Take the **FloatRefTarget div syntax seriously** as the canonical representation throughout the pipeline.
   - **Desugar the code-block shorthand pre-engine-execution**, so execution engines do not need to know about crossref structure.
   - Enforce a clean **front-end / back-end pipeline split**, in contrast to Q1's `crossref/theorems.lua` which mixes the two.
4. Sketch a path for **static analysis of `output: asis`** crossref targets via a user-declared id manifest.

## Non-goals (for this plan)

- Actual multi-file (book/website) crossref resolution — we only design so that we are not cornered.
- Full LaTeX/Typst/Docx writer parity — initial target is HTML + JSON debug.
- Dynamic crossref targets emitted from `output: asis` beyond the static-manifest opt-in.

---

## Background / References

### Quarto 1 (TS + Lua)
- Pipeline entry: `external-sources/quarto-cli/src/command/render/crossref.ts:27-99`
- Lua orchestration: `external-sources/quarto-cli/src/resources/filters/crossref/crossref.lua:174-197`
- FloatRefTarget custom node: `external-sources/quarto-cli/src/resources/filters/customnodes/floatreftarget.lua:74-110`
- Code-block → FloatRefTarget desugar in Lua: `external-sources/quarto-cli/src/resources/filters/quarto-pre/parsefiguredivs.lua:150+`
- ref_type extraction (`fig-`, `tbl-`, etc.): `.../filters/common/refs.lua:57-64`
- Custom categories: `.../filters/crossref/custom.lua:6-158` and `mainstateinit.lua:32-119`
- Theorems/proofs (antipattern — front/back-end mix): `.../filters/crossref/theorems.lua:21-133` + `.../customnodes/theorem.lua`
- Equations: `.../filters/crossref/equations.lua:12-97`
- Sections: `.../filters/crossref/sections.lua`
- Book multi-file (two-pass, post-render HTML fixup): `external-sources/quarto-cli/src/project/types/book/book-crossrefs.ts:42-280`
- `@ref` resolution: `.../filters/crossref/refs.lua:8-145`

### Quarto 2 (this repo)
- Pipeline orchestration: `crates/quarto-core/src/stage/mod.rs:20-112`, `crates/quarto-core/src/stage/stages/`
- Stages in order today: `ParseDocumentStage`, `MetadataMergeStage`, `EngineExecutionStage`, `AstTransformsStage`, `RenderHtmlBodyStage`, `ApplyTemplateStage`.
- Engine execution: `crates/quarto-core/src/stage/stages/engine_execution.rs:39-160`, registry at `crates/quarto-core/src/engine/registry.rs`.
- Transform pipeline registration: `crates/quarto-core/src/pipeline.rs` (`build_transform_pipeline`) and `crates/quarto-core/src/transforms/mod.rs`.
- Callout transform (template for sugaring): `crates/quarto-core/src/transforms/callout.rs`.
- CustomNode type (target representation): `crates/quarto-pandoc-types/src/custom.rs`.
- Div / Figure / attribute parsing + source info:
  - `crates/quarto-pandoc-types/src/block.rs:118-132` (`Figure`, `Div`)
  - `crates/quarto-pandoc-types/src/attr.rs:28-86` (`Attr`, `AttrSourceInfo`)
  - `crates/pampa/src/pandoc/treesitter_utils/fenced_div_block.rs:17-62`
  - `crates/pampa/src/pandoc/treesitter_utils/commonmark_attribute.rs:14-59`

### Prior Quarto 2 plans to be aware of
- `claude-notes/plans/2026-01-24-qmd-sugaring.md` — Already anticipated **FloatRefTarget as a CustomNode** sugaring transform (Phase D). This plan makes that concrete and extends it into full crossref handling.
- `claude-notes/plans/2026-01-26-document-structure-transforms.md` — Defines normalization / crossref / post-crossref / finalization phase ordering; this plan fills the **crossref** slot.
- `claude-notes/plans/2026-01-06-pipeline-stage-design.md` — `PipelineStage` abstraction we will extend with a new pre-engine stage.
- `claude-notes/plans/2026-01-06-execution-engine-infrastructure.md` — Established that `EngineExecutionStage` runs after include resolution but before most AST transforms. This plan inserts a new **pre-engine sugaring stage** between metadata-merge and engine-execution.

---

## Architectural Decisions

### D1. Canonical representation: `CustomNode("FloatRefTarget", ...)`

Throughout Quarto 2, once we are past the pre-engine sugaring stage, every float cross-reference target is represented as the same shape:

```rust
Block::Custom(CustomNode {
    type_name: "FloatRefTarget",
    slots: {
        "content":      Slot::Blocks([...]),   // the image/table/code/custom content
        "caption_long": Slot::Blocks([...]),   // optional
        "caption_short": Slot::Inlines([...]), // optional; from cap-short
    },
    plain_data: {
        "ref_type":   String,   // "fig" | "tbl" | "lst" | <custom> — the id prefix
        "kind":       String,   // "Figure" | "Table" | "Listing" | <custom> — display/category name
        "identifier": String,   // e.g. "fig-myplot"
        "parent":     Option<String>,   // for subfloats
        "order":      Option<{section: Vec<u32>, order: u32}>,  // filled by indexing stage
    },
    attr: <preserves original Div attributes>,
    source_info: <preserves source location>,
})
```

`ref_type` and `kind` are split in Q1 (`ref_type_from_float` vs. `category.name`) and we should preserve that split — `ref_type` is the syntactic prefix that matches `@fig-..` references; `kind` is the human/display/LaTeX-env name. For built-ins the mapping is 1:1; user-defined categories decouple them.

**Why CustomNode, not a new `Block::FloatRefTarget` variant?** Consistent with Q1 (`floatreftarget.lua`) and with the already-blessed pattern in `2026-01-24-qmd-sugaring.md`. Keeps `Block` closed to additions, since custom crossref categories are user-extensible.

### D1b. Block-level crossref targets: separate CustomNodes + future `BlockRefTarget`

Theorems/lemmas/proofs, and callouts-with-ids, get their own `CustomNode` types (`Theorem`, `Proof`, `Callout`). We considered unifying them under a generic `BlockRefTarget` (analogous to `FloatRefTarget`), but several of these nodes already exist as first-class custom kinds in Q1 (`Callout` most notably) and carry their own structural slots. Coercing them into a `BlockRefTarget` supertype would require a "multiple interfaces" story for filter matching (filters that want to match `BlockRefTarget` vs. `Callout` as node kinds), which we do not want to take on now.

For this plan: keep type-specific `CustomNode` types, and expose a shared inspection API (e.g. `fn crossref_target(&Block) -> Option<CrossrefTargetView>`) that front-end transforms use to treat any crossref-capable block uniformly for indexing and resolution.

For the **future**: once user-defined block-level crossref categories are needed (the analogue of `crossref.custom` but for blocks), we will introduce a generic `BlockRefTarget` CustomNode with `{ref_type, kind, identifier, content, caption, order}` for user-extensible designs. Built-in categories stay as their own CustomNode types. We do not implement `BlockRefTarget` in this plan; we only keep the door open by making the shared inspection API the canonical way indexing code touches block crossref targets — so adding a new CustomNode type that participates is a localized change.

### D2. Pre-engine sugaring stage

A new stage, `PreEngineSugaringStage`, is inserted between `MetadataMergeStage` and `EngineExecutionStage`.

Its responsibilities for this plan:

1. **Build the `RefTypeRegistry`** (see D7) from built-ins + `crossref.custom` metadata + `crossref.ids` manifest, and stash it on the stage context.
2. **Convert the code-block shorthand into the canonical FloatRefTarget div structure**, so that `EngineExecutionStage` and the engines behind it see only "plain" code blocks.
3. **Strip consumed cell options** from the code block body.
4. **Validate** that declared `crossref.ids` entries use registered ref-type prefixes.

Input:
````qmd
```{python}
#| label: fig-1
#| fig-cap: This is a caption.
from matplotlib import pyplot
pyplot.plot([1,2,3])
```
````

AST before stage (note: cell options live as leading `#|` lines inside `CodeBlock.text`, **not** in `CodeBlock.attr` — see C2 below):
```
CodeBlock(
  classes=["python"],
  attr={},
  text="#| label: fig-1\n#| fig-cap: This is a caption.\nfrom matplotlib import pyplot\npyplot.plot([1,2,3])\n"
)
```

After stage (still a `CodeBlock` that the engine will execute, but wrapped in the canonical scaffold):
```
Div(attr={id: "fig-1"})
  CodeBlock(classes=["python"], text="from matplotlib import pyplot\npyplot.plot([1,2,3])\n")
  Paragraph([Str("This is a caption.")])
```

**Cell-option partitioning.** `#|` options split into three disjoint sets:
- **Consumed by this stage** (lifted into the Div scaffold and removed from `text`): `label`, `fig-cap`, `fig-scap`, `fig-alt`, `tbl-cap`, `lst-cap`, `lst-label`, and any `<ref_type>-cap` / `<ref_type>-scap` for user-declared categories.
- **Passed through to the engine** (left in `text`): `echo`, `eval`, `warning`, `message`, `include`, `output`, `results`, engine-specific keys.
- **Consumed by later stages** (left in `text` for now): anything not in the first set. Downstream consumers trim as needed.

The exhaustive list lives alongside the stage implementation; Phase 1.1 defines it.

**Reconciliation works across the synthetic Div** because `EngineExecutionStage` serializes the *entire* (already-wrapped) AST to QMD, runs the engine on that QMD, and reconciles the post-engine parsed AST against the pre-engine AST (`crates/quarto-core/src/stage/stages/engine_execution.rs:14-16`). Both sides of the reconciliation see the wrapper Div at the same depth; positional matching at the top lines them up, and engines that wrap the inner CodeBlock in their own output scaffolding (e.g., `::: code` / `::: output`) are handled by the global structural-hash pass in `crates/quarto-core/src/stage/stages/compute.rs:82-102`. Phase 0 adds a round-trip fixture test confirming the synthetic Div survives QMD serialize → parse unchanged before anything else in Phase 1 is built.

This stage is also the natural home for other "pre-engine-aware" normalization: lifting `crossref.ids` manifests (see D6) and validating that declared ids match what the static AST offers.

**Why not do the full FloatRefTarget wrapping here?** Because engine output itself needs to be scanned for engine-generated figures/tables (e.g., a knitr chunk that emits three plots, a matplotlib call with `fig.subplots`). The canonical wrap happens **after** engine execution in a sugaring transform that handles both user-authored `::: {#fig-..}` divs and engine-authored figure divs uniformly.

### D3. Crossref pipeline stages (post-engine)

Inside `AstTransformsStage`, we add crossref-specific transforms in a fixed order. Aligning with the phase taxonomy from `2026-01-26-document-structure-transforms.md`:

```
NORMALIZATION PHASE (front-end, format-agnostic):
  ... existing transforms ...
  FloatRefTargetSugarTransform        # Div(#fig-..) -> CustomNode("FloatRefTarget")
  TheoremSugarTransform               # Div(.theorem) etc. -> CustomNode("Theorem")
  EquationLabelTransform              # DisplayMath + {#eq-..} -> labelled equation

CROSSREF PHASE (front-end, format-agnostic):
  CrossrefIndexTransform              # walks AST, assigns order+section numbers, builds index
  CrossrefResolveTransform            # resolves @fig-x Cite nodes -> Inline links w/ number

POST-CROSSREF PHASE (front-end, format-agnostic):
  CiteprocTransform (future)
  AppendixStructureTransform (existing)

FINALIZATION PHASE (back-end, format-specific):
  FloatRefTargetRenderTransform       # CustomNode -> format-specific AST (Figure, \ref{..} etc.)
  TheoremRenderTransform              # CustomNode -> format-specific AST
  ...
```

Front-end transforms operate on the canonical representation. Back-end transforms are plural and picked based on the output format. This is the **explicit fix for the `theorems.lua` antipattern**: the theorem node's structure and its "I am crossref target X" semantics live in the normalization and crossref phases; its LaTeX `\begin{theorem}` / HTML `<div class="theorem">` rendering lives in the finalization phase.

### D4. Index data structure (built to be mergeable)

```rust
pub struct CrossrefIndex {
    /// file_id this index was built for; used to namespace ids across files later
    pub file_id: FileId,
    /// All crossref targets declared in this file, keyed by identifier.
    pub entries: LinkedHashMap<String, CrossrefEntry>,
    /// Section numbering state (per-file): a stack of section counters.
    pub sections: Vec<u32>,
    /// Next-order counters, per ref_type.
    pub next_order: HashMap<String, u32>,
    /// Headings (for cross-file heading link fixup in book mode).
    pub headings: Vec<HeadingRecord>,
    /// Static manifest of ids promised by `output: asis` blocks (see D6).
    pub promised_ids: Vec<PromisedId>,
}

pub struct CrossrefEntry {
    pub identifier: String,
    pub ref_type: String,            // "fig", "tbl", "lst", "thm", "eq", ...
    pub parent: Option<String>,       // for subfloats
    pub order: Order,                 // {section: Vec<u32>, order: u32}
    pub caption: Option<Inlines>,     // for link text
    pub in_appendix: bool,
    pub source_info: SourceInfo,      // for diagnostics on duplicates / unresolved
}
```

For single-file mode, `CrossrefResolveTransform` reads this directly. For multi-file (future):

- Each per-file `CrossrefIndex` is serializable (JSON) — mirrors Q1's `.quarto/xref/*` files.
- A **project-level merge step** consumes a `Vec<CrossrefIndex>` and produces a `ProjectCrossrefIndex` keyed by `(file_id, identifier)`.
- `CrossrefResolveTransform` gains a `project_index: Option<&ProjectCrossrefIndex>` context handle. Missing local refs fall back to the project index; still-missing refs emit an "unresolved ref" span (like Q1's `.quarto-unresolved-ref`) that can be fixed up post-render, OR that is acceptable in HTML preview because we do not yet know the chapter context.

The hub-client preview path will **not** fix up unresolved cross-file refs live — it will render them as placeholders. A `StaticProjectAnalyzer` (future) can produce the merged index from pre-engine AST alone (see D6) for a fast, cross-file-aware preview.

### D5. `@ref` resolution

`@fig-myplot` parses today as a `Cite` Inline. `CrossrefResolveTransform` rewrites `Cite` nodes whose id is classified as a crossref by the `RefTypeRegistry` (see D7) into a resolved inline reference carrying the number ("Figure 3") and targeting `#fig-myplot` (or `chapter-2.html#fig-myplot` in book mode). Exact output representation is `CustomNode("CrossrefResolvedRef", ..)`; see O4.

**Ordering relative to citeproc.** Crossref resolution runs in the *crossref* phase; citeproc runs in the *post-crossref* phase. This ordering is load-bearing: the registry distinguishes crossref Cites from bibliographic Cites, crossref resolution consumes the crossref ones, and citeproc then sees a cleaned-up Cite set. Swapping the order would require citeproc to know about crossref prefixes to leave them alone — worse coupling.

### D7. `RefTypeRegistry` — authoritative ref-type set

`CrossrefIndexTransform` and `CrossrefResolveTransform` both need to know the *complete* set of valid ref-type prefixes: built-ins + `crossref.custom` + prefixes that appear in the `crossref.ids` manifest. The `PreEngineSugaringStage` also needs it to identify which `#| label:` cell options are crossref shorthand for user-declared categories.

```rust
// crates/quarto-core/src/crossref/registry.rs
pub struct RefTypeRegistry {
    entries: HashMap<String, RefTypeDef>,   // keyed by ref_type prefix ("fig", "tbl", ...)
}

pub struct RefTypeDef {
    pub ref_type: String,       // "fig"
    pub kind: String,           // "Figure"
    pub source: RefTypeSource,  // BuiltIn | CustomFromMetadata | Promised
    pub source_info: Option<SourceInfo>,  // where it was declared
}

impl RefTypeRegistry {
    /// Returns Some if `id` looks like "<prefix>-<rest>" and <prefix> is registered.
    pub fn classify_cite_id(&self, id: &str) -> Option<&RefTypeDef> {
        let (prefix, _rest) = id.split_once('-')?;
        self.entries.get(prefix)
    }
}
```

**Build order** — two phases inside `PreEngineSugaringStage`:

1. Seed with built-ins (`fig`, `tbl`, `lst`, `eq`, `thm`, `lem`, `cor`, `prp`, `exm`, `exr`, `def`, `rem`, `sol`), then extend from `crossref.custom` in merged metadata.
2. After the `crossref.ids` manifest is lifted, register any prefixes declared there that aren't already known as `Promised`.

After step 2 the registry is **frozen** for the rest of the pipeline.

**Threading.** The registry lives on the pipeline's context alongside `CrossrefIndex` — first on `StageContext` (populated by `PreEngineSugaringStage`), then moved into the transform-pipeline context at the start of `AstTransformsStage`. Front-end transforms borrow it by reference; back-end renderers also read it (for `kind` / display name), so it stays available through finalization.

**Disambiguation of `@`-references:**
- `@fig-myplot` — split at first `-`, lookup `fig` → crossref.
- `@smith2020` — no `-`, falls through to citeproc.
- `@mycustomfoo2020` — lookup of `mycustomfoo2020` returns `None` → citation.
- `@mycustom-foo` — crossref *iff* `mycustom` was registered; otherwise citation.
- `@fig-` — prefix registered, suffix empty; diagnostic "empty crossref id", leave as unresolved placeholder.

Registry correctness is load-bearing because crossref resolution runs before citeproc: a false positive makes a bibliography entry vanish silently; a false negative makes citeproc complain about a missing bib entry. Both are diagnose-able but both are user-visible, so the registry must be complete before the resolver fires.

**Registration-time diagnostics:**
- **Shadowing bib-key shapes.** A user-defined `ref_type: "smith"` will eat `@smith-2020` as a crossref. Warn at registration with source info pointing at `crossref.custom` in metadata.
- **Clash with built-ins.** Attempting to redefine a built-in `ref_type` is an error unless we explicitly want to support overriding display names (Q1 supports it; the decision lands here).

### D6. Static crossref id manifest for `output: asis`

Before engines run, we cannot know what an `output: asis` cell will emit. To keep the index cross-file-aware at static-analysis time, users opt in:

```yaml
---
crossref:
  ids:
    - tbl-dynamically-computed
    - fig-generated-plot
---
```

The pre-engine stage records these as `PromisedId` entries so that:
- The `CrossrefIndexTransform` can reserve numbering slots (or we defer numbering to post-engine; see O3).
- A future static project analyzer that runs **before any engine execution** can still produce a complete-enough `ProjectCrossrefIndex` for the hub-client live preview.

After engine execution, we validate: every `PromisedId` must be realized by a matching crossref target, and any target not in the manifest that came from an `output: asis` block triggers a diagnostic.

Other engine-emitted crossref targets (non-`asis`, e.g., the standard `fig-cap` shorthand) are **not** required to appear in the manifest because they are syntactically visible in the pre-engine AST — the manifest is only for truly dynamic, code-generated ids.

---

## Phases

### Phase 0 — Foundation (pipeline + canonical shape)

- [x] 0.1 Add `PreEngineSugaringStage` between `MetadataMergeStage` and `EngineExecutionStage`; empty implementation + wiring + test that pipeline runs unchanged.
- [x] 0.2 Define `CrossrefIndex` / `CrossrefEntry` / `Order` types in `crates/quarto-core/src/crossref/` (new module). Make them serializable.
- [x] 0.3 Define `CustomNode` type constant `"FloatRefTarget"` and a small inspection API (`fn ref_type_of(block: &Block) -> Option<&str>`) that front-end transforms share.
- [x] 0.4 Extend `StageContext` with `crossref_index: Option<CrossrefIndex>` and `ref_type_registry: Option<RefTypeRegistry>` slots; propagate both into the transform-pipeline context type in `crates/quarto-core/src/transforms/mod.rs`. *(Bridged in `AstTransformsStage` using the same `mem::take`/restore pattern as `includes`.)*
- [x] 0.5 Wire a `quarto-trace` `TraceEntry` emission for the crossref index at the end of the crossref phase. **Integration point:** extended `PipelineObserver` with `on_auxiliary_data(stage, index, kind, data)` (default no-op), implemented in `JsonTraceObserver` to record a `TraceEntry` with `stage: "aux:..."` and the JSON payload. Well-known kind tag `"CrossrefIndex"` is declared as `crossref::TRACE_KIND_CROSSREF_INDEX`. Phase 1.3's `CrossrefIndexTransform` will call this once the index is fully built; the pathway is tested end-to-end via `CountingObserver` unit tests.
- [x] 0.6 Define `RefTypeRegistry` + `RefTypeDef` + `RefTypeSource` in `crates/quarto-core/src/crossref/registry.rs`. Seed with built-ins; expose `extend_from_metadata(&Meta)` and `extend_from_promised(&[PromisedId])`.
- [x] 0.7 Round-trip fixture test: a synthetic `Div(#fig-1) > CodeBlock` serializes to QMD and re-parses identically (guards D2's reconciliation assumption). *Note during implementation:* pampa's reader strips the trailing newline that fenced code blocks produce. Shape comparison normalizes that — byte-exact equality on `CodeBlock.text` is not part of the round-trip contract; structural equality (id, classes, content up-to-terminator) is.

### Phase 1 — Floats, single file

- [x] 1.1 **Pre-engine code-block shorthand desugar** (D2): inside `PreEngineSugaringStage`, detect `CodeBlock` whose `#|` leading lines include a `label:` matching a registered ref-type prefix (via `RefTypeRegistry`), and wrap in a Div with the crossref scaffold. Rewrites `CodeBlock.text` to remove consumed options (`label`, `<reftype>-cap`, `<reftype>-scap`, `<reftype>-alt`) while leaving engine-relevant options (`echo`, `eval`, ...) in place. Lives in `crossref::codeblock_shorthand`.
- [x] 1.a **Metadata extraction** (added during Phase 1): `PreEngineSugaringStage` now reads `crossref.custom` and `crossref.ids` from merged metadata via `crate::crossref::metadata::read`. Errors become warnings on the stage context. A `CrossrefIndex` is seeded with `PromisedId`s for the transform pipeline to consume.
- [x] 1.2 **Post-engine FloatRefTarget sugaring transform** (D1): walks AST and wraps content into `CustomNode("FloatRefTarget", ..)` uniformly for:
  - `Div(#<reftype>-..)` containing arbitrary content (last Paragraph becomes caption_long).
  - `Figure` with a crossref id (Pandoc's native Figure caption lifted into slots).
  - `Div(#<reftype>-..) > Figure` — Div id wins; the inner Figure's content and caption are flattened into the custom node's slots.
  - `Div(#tbl-..) > Table` — Table kept as the sole content block; its caption is lifted to the target's caption slot.
  - Nested crossref targets inside other Divs/Callouts recurse correctly.
- [x] 1.3 **CrossrefIndexTransform** — single file scope: walks AST, maintains a section counter stack from `Header` blocks, assigns order+section to each `FloatRefTarget`, writes `plain_data.order` back into the node, populates `CrossrefIndex`. Duplicate ids emit a diagnostic and keep the first occurrence. At the end, publishes the index via `PipelineObserver::on_auxiliary_data` under `TRACE_KIND_CROSSREF_INDEX`.
- [x] 1.4 **CrossrefResolveTransform**: walks all inlines, classifies `Cite`s via `RefTypeRegistry::classify_cite_id`, rewrites crossref Cites into `CustomNode("CrossrefResolvedRef")` with `identifier`, `ref_type`, `kind`, `resolved`, `kind_source`, and (when resolved) `order`. Unknown crossref ids emit a diagnostic and produce an unresolved placeholder. Mixed bib+crossref Cite bundles emit a warning and are left alone (citeproc handles them). Single-ref and all-crossref multi-Cites resolve to the first id (multi-crossref ranges deferred).
- [x] 1.5 **CrossrefRenderTransform** (finalization phase): `CustomNode("FloatRefTarget")` with `ref_type=fig` → Pandoc native `Figure` with numbered caption (`"Figure 1: <caption>"`). Other ref_types → `Div` wrapping the content with a trailing numbered-caption `Paragraph`. `CustomNode("CrossrefResolvedRef")` → `Link` with `quarto-xref` class pointing at `#<identifier>`, text `"<Kind> <N>"` (or `"?id?"` for unresolved). End-to-end verified with a real `quarto render` invocation.
- [x] 1.6 Integration fixtures in `crates/quarto-core/tests/crossref_fixtures.rs`: 11 qmd-level fixtures that parse via pampa, run through the crossref transform pipeline, and assert over the resulting `CrossrefIndex` as structured data. Covers: Div-with-caption, Markdown `![](..){#fig-..}` native Figure, Div>Table, per-ref-type counters, section paths, non-crossref divs left alone, duplicate-id diagnostic, unresolved-ref diagnostic, `@`-disambiguation (fig-foo vs smith2020 vs mycustomfoo2020 vs smith-2020), `crossref.custom` → RefTypeRegistry, code-block shorthand end-to-end.

**Subfloats deferred.** Handling subfloat parent/child id assignment and nested numbering (e.g., "Figure 1a") is delicate enough that it gets its own follow-up plan rather than riding in Phase 1. Q1's `parsefiguredivs.lua:41-60` is the reference starting point for that future work. Phase 1 fixtures explicitly *exclude* subfloat inputs.

### Phase 2 — Block-level crossref targets

- [x] 2.1 Theorem sugaring: `Div(.theorem)` / `.lemma` / etc. → `CustomNode("Theorem", {kind, title, ...})`. Front-end `TheoremSugarTransform` handles 8 theorem-like classes (theorem, lemma, corollary, proposition, conjecture, definition, example, exercise). Extracts title from `name=` attr or first Header. Runs *before* `FloatRefTargetSugarTransform` to prevent greedy float classification. `crossref_target_view` extended to generically recognize any CustomNode carrying `plain_data.ref_type` + `kind` + non-empty identifier. `CrossrefIndexTransform` indexes Theorem nodes via `has_crossref_plain_data` predicate — no code change beyond the predicate.
- [x] 2.2 Callout integration: `CalloutTransform` now reads `ctx.ref_type_registry` and, when a callout's `attr.0` (the id) classifies as a crossref via `classify_cite_id`, injects the standard crossref triple (`ref_type`, `kind`, `identifier`) into `plain_data` alongside the existing callout-specific fields (`type`, `appearance`, etc.). No changes were needed to `crossref_target_view`, `CrossrefIndexTransform`, `CrossrefResolveTransform`, or `CrossrefRenderTransform` — the `plain_data` triple is the sole integration point, confirming the Phase 2.1 design. Callouts without an id or with a non-crossref id remain unnumbered (no implicit auto-labeling). The registry is threaded through the block walker via `Option<&RefTypeRegistry>` — callers that don't set it up (existing tests, WASM paths) get the same behavior as before. 5 new integration fixtures cover: indexed callout, non-indexed callout (no id), non-crossref id, multi-type numbering, and `@nte-foo` resolution. End-to-end verified: `@nte-key` renders as `<a href="#nte-key" class="quarto-xref">Note 1</a>`.
- [x] 2.3 Proofs: `ProofSugarTransform` converts `Div(.proof)` to `CustomNode("Proof")`. Proofs intentionally *don't* populate `plain_data.ref_type`, so they are **not numbered** and not indexed. Title from `name=` or first Header. Rendered with an italicized "*Proof.*" prefix.
- [x] 2.4 Back-end Theorem/Proof renderers: Theorem → `Div` with Strong "**Theorem N (Title).**" prepended to first Paragraph; classes `thm`/`theorem` etc. on the wrapper. Proof → `Div.proof` with Emph "*Proof.*" prepended. Both renderers share `prepend_theorem_label` for the Paragraph-insertion logic. CrossrefResolvedRef for `@thm-foo` resolves to `<a href="#thm-foo">Theorem 1</a>` as expected. End-to-end verified with `quarto render`.

### Phase 3 — Inline crossrefs: equations

Equations are **inline** elements, not block-level like floats/theorems. pampa already wraps `$$ ... $$ {#eq-xxx}` as `Span(id="eq-xxx", classes=["quarto-math-with-attribute"], [Math(DisplayMath, text)])`. The `@eq-xxx` reference is a standard `Cite` node, and "eq" is already a built-in ref type in the registry.

The key architectural extension: `CrossrefIndexTransform` currently only indexes block-level `CustomNode`s. Phase 3 extends it to also scan inlines within paragraphs for equation `Inline::Custom` nodes carrying the crossref triple.

- [x] 3.1 **EquationLabelTransform** (normalization phase): Walk paragraphs and convert `Span.quarto-math-with-attribute` wrapping `DisplayMath` into `Inline::Custom(CustomNode("Equation", {ref_type: "eq", kind: "Equation", identifier: "eq-xxx", content: Math(DisplayMath, text)}))`. Add constant `EQUATION` to `crossref/mod.rs`. Runs after `FloatRefTargetSugarTransform`, before `CrossrefIndexTransform`.
- [x] 3.2 **Indexing extension**: Extend `CrossrefIndexTransform` walker to scan inlines within paragraphs (and other inline-carrying blocks) for `Inline::Custom` nodes with the crossref triple. Resolution already works — `CrossrefResolveTransform` + `RefTypeRegistry` handle `@eq-xxx` → `CrossrefResolvedRef` with no changes needed.
- [x] 3.3 **HTML rendering**: In `CrossrefRenderTransform`, convert `Inline::Custom("Equation")` to a `Span(id="eq-xxx")` containing the original `Math(DisplayMath, text + "\\tag{N}")` where N is the equation number. This matches Q1's MathJax numbering approach. `CrossrefResolvedRef` for `@eq-xxx` already renders as a Link via the existing code path.
- [x] 3.4 **Integration fixtures**: Add equation-specific fixtures to `crossref_fixtures.rs` covering: basic equation indexing, equation numbering independent from figures, `@eq-xxx` resolution, multiple equations with section paths, equation + figure mixed numbering.

### Phase 4 — Multi-file foundations (design only in this plan)

- [ ] 4.1 Serialize per-file `CrossrefIndex` to `.quarto/xref/<file-id>.json`.
- [ ] 4.2 Sketch `ProjectCrossrefIndex` merge in `quarto-core::project::crossref`.
- [ ] 4.3 Sketch `StaticProjectAnalyzer` that parses all project files' **pre-engine AST** to produce a project-wide index consumable by `CrossrefResolveTransform` and by the hub-client preview.

Implementation of Phase 4 is out of scope for the initial crossref delivery — but Phases 0–3 must not foreclose it. Specifically: data model is serializable from day one; `CrossrefResolveTransform` takes the index by handle, not by construction.

---

## Additional pipeline suggestions (open for discussion)

- **S1. (Retracted.)** Tagging transforms with a phase label (`Normalization | Crossref | Finalization`) does not actually constrain them from making format-specific decisions — a transform can always read format metadata off the context. Mechanically enforcing "front-end transforms see no format info" would require scrubbing the document metadata and restricting the context API, which conflicts with Quarto's "metadata travels with the document" design. The normalization/crossref/finalization split therefore stays a **documentation and code-review convention**, not a machine-checked one. User-flagged — leaving behind for the record.
- **S2. Expose the canonical FloatRefTarget shape as the documented extension point.** Any future custom category authors should write transforms that produce/consume `CustomNode("FloatRefTarget")` (and, eventually, `CustomNode("BlockRefTarget")`), not new AST shapes.
- **S3. (Deferred.)** Generalizing `PreEngineSugaringStage` beyond crossrefs is premature — we'll add pre-engine stages as concrete needs arise (include-shortcode resolution being an almost-certain next one). For this plan, `PreEngineSugaringStage` is strictly the crossref-shorthand desugar.
- **S4. Annotate synthetic nodes with their originating pipeline stage.** The Div we wrap a code block into in D2 did not exist in the source; a diagnostic pointing "to" it needs to explain "this was created by PreEngineSugaringStage from `#| label: fig-1` at L.C". The precedent exists: `SourceInfo::FilterProvenance { filter_path, line }` (`crates/quarto-source-map/src/source_info.rs:49-55, 141`) is stamped onto every node created by a Lua filter via `filter_source_info()` (`crates/pampa/src/lua/types.rs:1504`) and every constructor in `crates/pampa/src/lua/constructors.rs`. See P1 below for the concrete proposal.

---

## Resolved Questions (notes for implementation)

- **O1. FloatRefTarget representation.** Resolved: `CustomNode("FloatRefTarget", ..)`.
- **O2. Custom categories.** Resolved: transforms read category definitions from document metadata (`crossref.custom`) via the normal metadata API. Because metadata merging runs early, the source of the metadata (document, project, extension) does not matter — the pipeline simply consults the merged metadata. Keep Q1's `crossref.custom` YAML schema verbatim as a starting point.
- **O3. `PromisedId` numbering.** Resolved: number promised ids only upon realization. Missing realizations produce precise diagnostics. **Revisit** this if engine failures with partial output turn out to be common in practice — in that case, reserving slots up front may give more stable numbering under errors.
- **O4. `@ref` output shape.** Resolved: `CustomNode("CrossrefResolvedRef", ..)` in the front-end. This also reduces the risk that user Lua filters targeting `Link` nodes accidentally pick up resolved crossref links. A back-end transform (running in the finalization phase of the HTML writer pipeline) converts the CustomNode to an actual `Link` so the HTML output is a normal `<a href=...>`; LaTeX's back-end converts it to a `\ref{..}` raw inline.
- **O5. Pre-engine stage scope.** Resolved: strictly crossref shorthand desugar for v1. Other pre-engine needs (include shortcodes, etc.) get their own stages when required (see S3).
- **O6. `output: asis` policy.** Resolved (strict): any crossref target produced by an `output: asis` block **must** be declared in `crossref.ids`. Undeclared dynamic ids produce a diagnostic and are not indexed — locally or project-wide. Rationale: Quarto 2's source-map-backed diagnostics are strong enough to make strictness actionable for users and coding agents; strictness also unifies single-file and multi-file behavior (what's visible to a `StaticProjectAnalyzer` for preview is the same set of ids that the full render honors). Q1's leniency here was driven by weak diagnostic capability, which no longer applies.
- **O7. Duplicate ids across files.** Resolved: a **diagnostic**, not a hard error (keeps preview usable). Implementation requirement: the diagnostic must be *good* — it needs to point at *all* occurrence locations (file + line/col), which means the `CrossrefIndex` must retain source-location-with-file-id for every entry and the project merge step must surface both locations in the diagnostic payload.
- **O8. Book-only appendix logic.** **Deferred** to the book-projects session — we don't yet have enough context to decide, and the impact isn't testable from our current vantage point. In-document appendix sections (Q1's `.appendix` heading class) are *also* deferred; Phase 1 does not implement appendix-aware numbering.
- **O9. Hub-client preview behavior.** In-file crossrefs (including `@fig-..` within the same document) resolve normally in the hub-client preview — same pipeline, same transforms. Only cross-file/book refs degrade to placeholders in preview (see D4).
- **O10. Engine reconciliation across synthetic wrappers.** *Not* a concern: `EngineExecutionStage` serializes the whole pre-engine AST to QMD and reconciles the post-engine parsed AST against it, so the synthetic Div appears on both sides at the same depth. Phase 0.7 pins this with a round-trip fixture before Phase 1 proceeds.

## Resolved (post-investigation): P1. Node-provenance mechanism for S4

The precedent is on `SourceInfo` itself: a `FilterProvenance { filter_path: String, line: usize }` variant (`crates/quarto-source-map/src/source_info.rs:49-55`) alongside `Original` / `Substring` / `Concat`. For Lua, `filter_source_info()` in `crates/pampa/src/lua/types.rs:1504` walks the Lua stack to capture the filter file and line, and every Pandoc constructor in `crates/pampa/src/lua/constructors.rs` stamps the result onto the new node.

**Proposal.** Add a new variant `StageProvenance` to `SourceInfo`, leaving the existing four variants (`Original`, `Substring`, `Concat`, `FilterProvenance`) untouched:

```rust
pub enum SourceInfo {
    Original { .. },
    Substring { .. },
    Concat { .. },
    FilterProvenance { filter_path: String, line: usize },  // unchanged
    StageProvenance {
        /// Name of the pipeline stage that created this node (e.g. "PreEngineSugaringStage").
        stage_name: String,
        /// Optional link back to the source location that *caused* this node to exist
        /// (e.g. the user's `#| label: fig-1` line for a sugaring-created Div).
        /// `None` for genuinely source-less synthesis.
        source: Option<Arc<SourceInfo>>,
    },
}
```

**JSON blast-radius containment.** `SourceInfo` uses serde's default externally-tagged enum representation (e.g. `{"FilterProvenance": {"filter_path": "...", "line": 42}}`), with no `#[serde(tag=..)]` or renames. Adding a new variant leaves the JSON serialization of every existing variant **byte-identical** to today. Any JSON document that does not contain a stage-synthesized node is indistinguishable from one produced before this change — so untyped consumers that key off `"Original"` / `"Substring"` / `"Concat"` / `"FilterProvenance"` are unaffected. Only JSON emitted from pipelines that newly use the crossref stages will contain `{"StageProvenance": {...}}` objects.

Considered and **rejected**: generalizing `FilterProvenance` and `StageProvenance` under a single `Synthesized { origin, source }` variant. That would rename `"FilterProvenance"` to `"Synthesized"` in the JSON and is observable to any untyped consumer — the precise ripple we want to avoid.

Why this design:

- **Single source of truth.** All synthetic-node provenance travels in `SourceInfo`, not in sidecar maps or reserved attributes — consistent with how Lua filter provenance already works.
- **Back-link for diagnostics.** The `source: Option<Arc<SourceInfo>>` is the key addition: a diagnostic on the synthetic crossref Div can say "created by PreEngineSugaringStage from `#| label: fig-1` at file.qmd:42:5", because we thread the originating `SourceInfo` (the original CodeBlock's) in when the stage creates the new Div.
- **No churn for existing code.** Lua constructors, `filter_source_info()`, and every pattern-match on `FilterProvenance` stay exactly as they are. New match sites only need to handle `StageProvenance` — and most pattern-match sites today already have `FilterProvenance { .. }` as a no-op / default arm, which covers the new variant by the same shape (all pattern sites get reviewed to confirm).

Minor downside: two variants carrying similar intent. Acceptable in exchange for zero JSON churn and zero surprise for consumers that parse source maps untyped.

## Outstanding Questions

_None at this time._

---

## Success Criteria (single-file)

1. `::: {#fig-foo} ![..](..) \n\n Caption :::` renders with the correct "Figure 1: Caption" structure in HTML, numbering continues across figures, `@fig-foo` resolves to a link with the caption number.
2. ```{python}` ... `#| label: fig-foo` + `#| fig-cap: ...``` produces the *same* output as the explicit div form, with engines blissfully unaware of crossref structure.
3. `theorems.lua`-style front-end/back-end mixing does not recur: the theorem structural transform lives in `transforms/` and the HTML renderer is a separate transform.
4. The `CrossrefIndex` is emitted as JSON through the `quarto-trace` infrastructure at the end of the crossref phase (concentrating tracing concerns rather than introducing a dedicated `--debug` flag). The intent is twofold: (a) **testability** — unit and integration tests assert over the index as structured JSON, rather than over rendered HTML, which is sensitive to many unrelated concerns; (b) **future-proofing** — the JSON shape is the same shape that per-file indices will take when persisted under `.quarto/xref/` for the multi-file merge step (D4, Phase 4.1). If this JSON already "would make sense" as the unit of merge for a `ProjectCrossrefIndex`, we have evidence that Phase 4 is an additive extension rather than a redesign.
5. A short end-to-end test confirms an unresolved `@nope-1` produces a precise source-located diagnostic and a visible placeholder.
