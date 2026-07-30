# .ipynb Surface Syntax for Quarto 2 — Feasibility and Design

**Date**: 2026-07-20
**Strand**: bd-19nc56ao (related: k-zr88, bd-xxul, bd-kik3s1vt)
**Status**: Design discussion / plan skeleton — awaiting review

## Overview

Goal: `q2 render notebook.ipynb` produces HTML, with Quarto-1-comparable
semantics (markdown + raw cells rendered, code cells shown with their *stored*
outputs, no execution by default), **and with source-tracked diagnostics that
speak the user's coordinate system**:

```
Error: Invalid YAML syntax
  --> notebook.ipynb[cell 3] 2:9
   |
 2 | format: htlm
   |         ^^^^ unknown format 'htlm', did you mean 'html'?
```

— i.e. squiggles are drawn over the *logical* markdown text of the cell
(unescaped, joined), never over the raw JSON with its `\n`-escaped string
literals, and never as `notebook.ipynb:847:3869`.

This doc revisits two earlier designs against the July-2026 codebase, resolves
the source-location question (the hard part), and lays out a phased plan.

## Prior art

1. **`claude-notes/surface-syntax-converter-design.md`** (2025-10-13):
   `SourceConverter` trait + `ConverterRegistry`, converters orthogonal to
   engines, `ConvertedSource { qmd, source_map, suggested_engine, metadata,
   original_format }`. Never implemented; still the right *shape*.
2. **`claude-notes/plans/2025-12-15-source-info-for-structured-formats.md`**
   (k-zr88, "design finalized"): sidecar source-map files next to on-disk qmd,
   plus a new `NotebookCell` variant in `SourceInfo`. Several of its load-bearing
   constraints no longer hold (audit below); this doc supersedes its ipynb half.
3. **bd-kik3s1vt** (posit-assistant transcript experiment, unmerged branch
   `beads/bd-kik3s1vt-transcript-surface`): validated the "foreign format → qmd
   string → normal pipeline" flow with a standalone converter crate and no
   quarto-core changes. It punted on source mapping (validated turns by
   re-parsing through pampa instead). Note: its plan file
   `2026-06-08-transcript-surface-syntax-experiment.md` was never committed —
   the reference in the strand is dangling; details live in braid comments.
4. **Quarto 1** (`external-sources/quarto-cli/src/core/jupyter/`):
   `jupyterToMarkdown()` concatenates cells into markdown (markdown/raw cells
   verbatim, code cells as `.cell` divs with formatted stored outputs).
   **Preserves no source locations at all.** So anything we ship here is
   strictly better than Q1 on diagnostics.

### Assumption audit: Dec 2025 design vs. July 2026 codebase

| k-zr88 assumption (Dec 2025) | Reality now | Consequence |
|---|---|---|
| "qmd files must exist on disk" (intermediate for engines) | False. `run_pipeline(content: &[u8], source_name)` is fully in-memory (`quarto-core/src/pipeline.rs`); the Q2 jupyter engine executes fenced blocks from the qmd *string* (`engine/jupyter/text_execute.rs`), no notebook or qmd intermediary on disk | **The entire sidecar-file mechanism is unnecessary.** Conversion can happen in-process, in front of the parser |
| "SourceContext serialization is limited (PandocAST JSON only)" | False since the k-44 pool work: `SourceContext`/`SourceInfo` are plain serde types | No cross-process handoff problem to design around |
| Needs new `SourceInfo::NotebookCell` variant carrying cell metadata + content file id per span | Cell identity is per-*file*, not per-*span*: with one ephemeral `SourceFile` per cell, existing `Original`/`Substring`/`Concat` compose fine | **No `SourceInfo` enum change needed** (the enum is closed, with ~8 match sites upstream — avoiding this is a big deal) |
| `FilterProvenance` variant exists | Extracted crate has `Generated { by: By, from: [Anchor] }` instead | Use `Generated` for synthesized scaffolding (fences etc.) |

What *hasn't* changed: no converter infrastructure exists; `SourceType::Ipynb`
is detected (`quarto-core/src/stage/data.rs:170-201`) but never acted on —
`ParseDocumentStage` unconditionally calls the qmd reader; project discovery is
`.qmd`-only (bd-xxul open).

## The core design question: source locations through JSON strings

### Why pointing into the .ipynb bytes is the wrong root

A markdown cell's text lives in JSON string literals:

```json
{ "cell_type": "markdown",
  "source": ["# Hello\n", "some\ttext"] }
```

The logical text (`# Hello\n` + `some<TAB>text`, unescaped, joined) relates to
the file bytes through a **non-affine** map: `\n` is 2 file bytes → 1 logical
byte, `é` is 6 → 2, fragment boundaries interleave with `", "` syntax.

`quarto-source-map` cannot express this, *by explicit policy*:

- `Substring`/`Concat` compose only **affine** (constant-shift) maps over
  byte-identical slices of a registered file. `map_offset` is pure additive
  arithmetic (`mapping.rs`).
- A `Transformed` variant existed once and was **removed as unused**; the doc
  comment at `source_info.rs:21` states the crate's policy: transformations
  point at pre-transformation text, "accepting that the byte offsets are
  approximate".
- The closest analog — YAML quoted/block scalars in `quarto-yaml` — has the
  same problem and currently punts (`compute_scalar_len` uses the *unescaped*
  length against the *raw* start offset; acknowledged `TODO`).

We could add a run-table variant upstream (see "escape hatch" below). But
first, observe that we don't actually need it:

### Key insight: make the logical cell text the coordinate system

Nobody reads a notebook as raw JSON — not the user (who sees cells in Jupyter
or a cell-aware editor), not our renderer. The JSON byte layout is an
implementation detail (Jupyter itself rewrites it freely — fragment
splitting is unstable across saves). So instead of treating `foo.ipynb`'s
bytes as the root and fighting the escape problem, **register each cell's
logical content as its own ephemeral in-memory `SourceFile`, and make that the
root coordinate system.** The unescaping happens once, at ingestion, *before*
source tracking begins — so no non-affine mapping ever needs to be
represented.

Concretely, the converter:

1. Parses the notebook with plain `serde_json` (no spans needed! — a pleasant
   consequence of this choice).
2. For each cell, joins + unescapes `source` (serde does the unescaping) and
   registers it: `ctx.add_file("foo.ipynb[cell 3]", Some(logical_text))`.
3. Assembles the qmd string cell by cell, building in parallel a
   `SourceInfo::Concat` whose pieces are:
   - `Substring`/`Original` spans of the per-cell virtual files, for cell
     content copied verbatim;
   - `Generated { by: By::raw("ipynb/scaffold", …), from: [anchor to cell] }`
     pieces for synthesized text (code fences, separators, generated YAML).
4. Hands the qmd string to the parser **with the concat as
   `parent_source_info`**.

Step 4 is the part that already exists: `pampa::readers::qmd::read` takes
`parent_source_info: Option<SourceInfo>` (`readers/qmd.rs:56`), and when set,
*every AST node's* SourceInfo is built as `Substring { parent, node_bytes }`
instead of `Original { file_id, … }` (`pandoc/location.rs:214-218`). This is
the same mechanism `quarto-core/src/cell_options/mod.rs` uses for `#|` option
blocks — the one place in the tree that already does precise
"reassembled-virtual-document" mapping, and whose module doc states the exact
invariant we're relying on: every mapped byte of the virtual string is a real
source byte, so affine `map_offset` resolves exactly.

End-to-end, an error at qmd offset *k* resolves: `Substring(parent)` →
`Concat` piece containing *k* → per-cell virtual file → `map_offset` computes
line/col **within the logical cell text**, and the ariadne renderer draws the
squiggle over `SourceFile.content` — which *is* the unescaped markdown. Both
of the user-facing requirements (cell-coordinate positions, squiggles over
real text) fall out of existing machinery.

### Presentation

`quarto-error-reporting` titles reports with `SourceFile.path` verbatim and
computes line/col from the resolved file. Two options:

- **(P1) Encode the label in the path** — name the virtual file
  `foo.ipynb[cell 3]` (optionally `foo.ipynb[cell 3, markdown]`). Zero
  upstream changes; text output is exactly the target UX. Costs: the OSC-8
  `file://` hyperlink gets a non-existent path (should suppress or link plain
  `foo.ipynb`), and `--json-errors` reports the pseudo-path as `file`.
- **(P2) Structured origin upstream** — add to `quarto-source-map` a
  `FileMetadata` extension (e.g. `origin: Option<FileOrigin>` with
  `FileOrigin::NotebookCell { notebook_path, cell_index, cell_id, cell_type }`),
  and teach `quarto-error-reporting` to (a) title reports from it, (b) emit a
  structured JSON location (`{"file": "foo.ipynb", "cell": {"index": 3, "id":
  "abc123", "type": "markdown"}, "line": 2, "column": 9}`), (c) point
  hyperlinks at the real notebook. Both crates are posit-dev-owned; the cost
  is two upstream releases + version bumps.

Recommendation: **P1 for the prototype phase, P2 before calling the feature
done** — pseudo-paths in the JSON wire shape are the kind of thing downstream
tooling starts depending on.

Cell numbering: 1-based over *all* cells, qualified with the cell type
(`[cell 3, markdown]`), matching the k-zr88 decision; carry nbformat ≥4.5
`cell.id` in the JSON output for tools. (Open question below.)

### What we give up, and the escape hatch

This design has **no physical byte offsets into the .ipynb file**. That only
matters if some consumer wants to underline the raw JSON — essentially: a
text-editor/LSP session with the `.ipynb` open *as JSON*. Cell-aware
consumers (Jupyter, Positron, VS Code notebooks) address positions as
(cell, line, col) — exactly what we produce.

If raw-file offsets are ever needed, two escalation paths, in increasing
order of ambition:

1. **Converter-level decode maps**: while reading the notebook with a
   span-aware JSON reader (we own `pampa`'s raw-json reader as a starting
   point), record per cell a run table `[(logical_range, file_range), …]`
   breaking at every escape sequence and fragment boundary. Store it next to
   the virtual file (converter output struct — no `SourceInfo` change), and
   let an LSP layer compose it on demand.
2. **Upstream `Transformed { parent, runs }` variant** in `quarto-source-map`,
   reintroducing the removed variant with an explicit run table. This is the
   principled fix and would *also* solve the `quarto-yaml`
   quoted-scalar/block-scalar imprecision (same problem class). It touches
   every match on the closed enum (`map_offset`, `map_range`,
   `resolve_byte_range`, `preimage_in`, `length`, `remap_file_ids`,
   `root_file_id`, `collect_file_ids`) — worth doing only when a concrete
   consumer exists.

Neither blocks the render feature. Deliberately deferred.

### Implementation seams that need closing

These are the real gaps found in the current code (each becomes a work item):

1. **Syntax-error diagnostics bypass `parent_source_info`.**
   `produce_diagnostic_messages(input_bytes, …, &context.source_context)`
   (`readers/qmd.rs:131`) builds locations in the parse buffer's own file
   coordinates; it never consults `context.parent_source_info`. Since qmd
   *syntax errors* are the headline use case, this must be threaded through
   (wrap produced locations as `Substring` of the parent, mirroring
   `location.rs:214`). Likely also benefits the existing cell-options path.
2. **`ParseDocumentStage` wiring.** Branch on `LoadedSource.source_type ==
   SourceType::Ipynb`: run the converter, pass the assembled qmd +
   parent concat to `pampa::readers::qmd::read`, and make sure the per-cell
   virtual files are registered in **every** `SourceContext` that reaches the
   diagnostic renderer (pampa builds its own inside `read`, and
   `run_pipeline` *rebuilds* one on the error path at `pipeline.rs:795-808` —
   both must contain the cell files, or squiggles silently drop).
3. **Cross-cell error ranges.** A span produced by the parser can straddle a
   `Concat` piece boundary (e.g. an unclosed fence in cell 2 swallowing cell
   3); the renderer resolves `root_file_id()` to one file and would degrade.
   Mitigation, per the k-zr88 decision "require well-formed cells": a cheap
   pre-parse gate that parses each markdown cell *standalone* and reports
   ill-formedness (unclosed fence/div) as a per-cell error before the
   concatenated parse. Then cross-piece spans indicate a Quarto bug, and the
   renderer can fall back to "cell N through cell M" without a snippet.

## Conversion architecture

**Where it sits**: in front of the parser, keyed off the already-existing
`SourceType` detection — *not* a new pipeline stage, and *not* (yet) the full
`ConverterRegistry` of the 2025-10 design. Concretely: a
`quarto-core`-adjacent module (or small crate) exposing

```rust
pub struct ConvertedSource {
    pub qmd: String,
    pub parent: SourceInfo,              // Concat over cell virtual files
    pub files: Vec<(String, String)>,    // (virtual path/label, logical content)
    pub notebook_meta: ConfigValue,      // from nb.metadata (kernelspec, etc.)
    pub suggested_engine: Option<String>,
}
pub fn convert_ipynb(path: &Path, bytes: &[u8]) -> Result<ConvertedSource, DiagnosticMessage>
```

matching the 2025-10 `ConvertedSource` shape closely enough that a registry
can be slotted in unchanged when a *second* converter (percent scripts)
arrives. Building the registry for one converter would be speculative.

**Cell emission** (Q1-compatible):

- *Markdown cells*: logical text verbatim (one `Substring` piece covering the
  whole cell), `\n\n` separators as `Generated`.
- *Raw cells*: wrapped in the appropriate raw block by mime hint
  (`Generated` fences around a `Substring` body).
- *Code cells*: `` ```{lang} `` fence (`Generated`, anchored to the cell) +
  source verbatim (`Substring`) + closing fence. `lang` from
  `nb.metadata.kernelspec.language`. `#|` option lines inside the cell source
  then flow through the *existing* cell-options machinery, which composes:
  its concat's parent is our concat, and YAML option diagnostics land in the
  right cell automatically. (Worth an explicit test — it's the flagship
  composition win.)
- *Front matter*: Q1 semantics — if the first cell is raw/markdown and starts
  with `---` YAML, it becomes the document front matter. Because it's cell
  content, it stays **source-mapped**: YAML validation errors point at
  `foo.ipynb[cell 1]` with real squiggles. Notebook-level metadata
  (`nb.metadata`) merges in via the config layer (like Q1's kernelspec
  handling), not by synthesizing YAML text.

**Engine/execution defaults**: `suggested_engine` from kernelspec; ipynb
renders **without execution by default** (Q1 parity — stored outputs are the
document). `--execute`/`execute.enabled: true` routes through the existing
jupyter engine unchanged (it already executes fenced blocks from qmd text).

**ipynb-filters** (`2026-04-23-ipynb-filters-and-engine-partitioning.md`): out
of scope here. Note for later: filters rewrite notebook JSON pre-conversion,
so all mapping targets the *filtered* notebook; cell-coordinate reporting
stays meaningful as long as filters preserve cell identity, which is all Q1
promises either.

## Stored outputs

Q1 renders code cells *with their stored outputs* (`mdFromCodeCell`); that is
the point of rendering a notebook. Two options:

- **(A) Bake outputs into the qmd at conversion** (Q1's approach): format
  each output's mime bundle into markdown/divs during conversion. Simple,
  single-pass; but injects large `Generated` regions into the source-mapped
  document and duplicates output-formatting logic that the jupyter engine
  already has.
- **(B) Stored-output replay at the engine layer**: the converter emits clean
  qmd (code cells as plain fenced blocks); a small "stored notebook outputs"
  engine renders each code cell by formatting the cell's stored `outputs`
  array through the same `format_outputs`/`render_cell` machinery
  `text_execute.rs` uses for live kernel results — nbformat stored outputs
  and Jupyter wire results share the mime-bundle shape. This mirrors the
  existing `ReplayEngine`/capture-splice precedent (replayed results spliced
  in lieu of execution) and keeps conversion pure.

Recommendation: **B**. It reuses the one implementation of mime-bundle →
markdown we maintain, keeps the converter testable as a pure function, and
gets image-output fixes (bd-5t6wvu7m) for free in both paths. Known limitation
inherited: image outputs currently render as placeholders — acceptable for
the first cut, tracked already.

## Project integration (bd-xxul)

Single-file `q2 render foo.ipynb` lands first (output naming: `foo.html`).
Extending *project discovery* to treat `.ipynb` as renderable is exactly
bd-xxul and stays a separate decision (it needs the `.md`/`.Rmd` semantics
conversation too); this design gives it the mechanism it was waiting on.
Hub/WASM: conversion is pure Rust + serde_json, so it ports to the WASM
pipeline without new surface area (not in scope for the first cut).

## Phases

### Phase 1 — converter core + source mapping (the design's proof)
- [ ] Fixture notebooks (tiny; per-feature) under a converter test dir
- [ ] TDD: markdown/raw cell conversion; front-matter extraction; assembled
      qmd snapshot tests
- [ ] TDD: *the* source-location test — malformed markdown in cell N →
      diagnostic labeled `foo.ipynb[cell N]`, correct in-cell line/col,
      snippet showing logical (unescaped) text. Must fail before seam fixes
- [ ] Thread `parent_source_info` through `produce_diagnostic_messages`
- [ ] Per-cell well-formedness pre-check (standalone parse gate)

### Phase 2 — pipeline wiring
- [ ] `ParseDocumentStage` branch on `SourceType::Ipynb`
- [ ] SourceContext plumbing on both success and error paths (incl.
      `run_pipeline`'s rebuilt context)
- [ ] Cell-options composition test (`#|` YAML error inside a code cell of a
      notebook lands in the right cell)
- [ ] End-to-end: `cargo run --bin q2 -- render fixture.ipynb` + inspect
      output + inspect a deliberately-broken fixture's terminal diagnostic
      (per CLAUDE.md end-to-end verification policy)

### Phase 3 — code cells with stored outputs
- [ ] Stored-output replay engine (option B), reusing `format_outputs`
- [ ] `--execute` route-through test (existing jupyter engine)
- [ ] Q1 comparison render on a real-world notebook

### Phase 4 — presentation hardening (upstream)
- [ ] `FileOrigin`-style structured metadata in quarto-source-map +
      quarto-error-reporting rendering (replaces pseudo-path P1)
- [ ] `--json-errors` structured cell locations
- [ ] Hyperlink behavior for virtual files

### Deferred / explicitly out of scope
- ConverterRegistry (until percent-script converter exists)
- ipynb-filters
- Raw-JSON offset mapping (`Transformed` run-table upstream; also fixes
  quarto-yaml scalar `TODO`) — needs a concrete consumer first
- LSP integration; project discovery (bd-xxul decision); WASM/hub exposure

## Open questions

1. **Cell numbering**: 1-based over all cells (recommended, matches k-zr88)
   vs. per-type counters vs. Jupyter execution counts? Include `cell.id` in
   text output or JSON only (recommended: JSON only)?
2. **Pseudo-path MVP** (`foo.ipynb[cell 3]` as `SourceFile.path`): acceptable
   as an intermediate state, given `--json-errors` consumers would briefly see
   it as `file`?
3. **Concatenated-document semantics**: Q1 concatenates all cells into one
   markdown doc (cross-cell footnotes/links work); Jupyter renders cells
   independently (they don't). Recommendation: concatenate (Q1 parity) +
   per-cell well-formedness gate. Confirm?
4. **Stored-output replay as an "engine"** (option B): agree with modeling it
   at the engine layer, or prefer conversion-time baking (option A)?
5. Does k-zr88 stay open for the *percent-script* half (its plain-text-format
   design is still valid and untouched by this doc), with the ipynb half
   superseded here?
