# Plan 7c — ipynb content processor

**Series root:** [2026-06-27-plan7-native-percent-spin-sourceinfo.md](2026-06-27-plan7-native-percent-spin-sourceinfo.md)
**Depends on:** [2026-07-08-plan7b-native-content-processors.md](2026-07-08-plan7b-native-content-processors.md) (the registry, the `ProcessorContext`, the source-file channel)
**Absorbs the design body of:** [2026-07-20-ipynb-surface-syntax-design.md](2026-07-20-ipynb-surface-syntax-design.md) (the source-location design, cell emission rules, stored-output decision, and implementation seams — re-homed here; that doc's *attachment point* is superseded, see below)
**Strand:** bd-19nc56ao (p1). Related: bd-xxul (discovery), bd-zlemoc6w (conversion provenance), k-zr88 (structured-format source info), bd-kik3s1vt (transcript surface)
**Date:** 2026-07-08 (rewritten 2026-08-17 — promoted from placeholder)
**Status:** PLAN — architecture settled; work items unstarted. Additive over 7b's seams.

---

## Why this plan is the home for `.ipynb`

Three designs converged on this. The merge runbook's **D3**
(`2026-08-13-ts-engine-extensions-merge-main.md`) settled the attachment
question: `SourceType` lost its `Ipynb` variant, and `.ipynb` now lands as **an
engine that claims `.ipynb`**, converted before the parser by
`SourceConversionStage` (renamed from `EngineClaimsFileStage` in that merge).
That is exactly 7b's model — an engine names a content processor on a
`claims-files` entry — so the ipynb converter belongs in 7b's registry rather
than in a bespoke pre-parse branch.

`2026-07-20-ipynb-surface-syntax-design.md` did the hard design work (below)
but keyed conversion off `SourceType::Ipynb` detection and declined a registry
as "speculative for one converter." The first is now gone; the second is
answered — 7b builds the registry for percent and spin, so ipynb is the third
entry, not the first. **That doc's body is absorbed here; only its
attachment point is superseded.**

The series invariant is what a bespoke path would lose: **zero engine launch in
Pass-1.** A wire path for ipynb conversion would reintroduce exactly the
Deno-in-the-indexing-pass bug 7b exists to fix, so the converter is native
Rust. That answers 7c's previously-open "native port vs wire path" question:
**native**, by the series invariant, not by preference.

---

## Corrections to the original 7c stub

The placeholder inherited two assumptions from
`2025-12-15-source-info-for-structured-formats.md` that the July-2026 audit
falsified. Both **remove** work:

1. **No `SourceInfo::NotebookCell` variant.** The stub listed one as an
   additive enum arm. Cell identity is per-**file**, not per-**span**: with one
   ephemeral `SourceFile` per cell, the existing `Original`/`Substring`/`Concat`
   compose fine. `SourceInfo` is a closed enum with ~8 upstream match sites
   (`map_offset`, `map_range`, `resolve_byte_range`, `preimage_in`, `length`,
   `remap_file_ids`, `root_file_id`, `collect_file_ids`) — not touching it is a
   material saving.
2. **No sidecar file.** The stub listed a `jupyter_notebook` arm of 7b's
   sidecar envelope. The envelope's premise — "the converted qmd is plain text
   on disk with nowhere to store mapping inline" — is false: `run_pipeline`
   is fully in-memory and no qmd intermediate is written. Conversion happens in
   front of the parser, in process, and the mapping lives in memory. **7b
   should therefore not land the sidecar envelope on 7c's behalf** (see the
   note added to 7b's forward-compatibility obligations).

---

## Source locations — the design

### Why the `.ipynb` bytes are the wrong coordinate root

A markdown cell's text lives in JSON string literals:

```json
{ "cell_type": "markdown", "source": ["# Hello\n", "some\ttext"] }
```

The logical text relates to the file bytes through a **non-affine** map: `\n`
is 2 file bytes → 1 logical byte, `é` is 6 → 2, and fragment boundaries
interleave with `", "` syntax. `quarto-source-map` cannot express that *by
explicit policy* — `Substring`/`Concat` compose only affine (constant-shift)
maps over byte-identical slices, and a `Transformed` variant was removed as
unused.

### The technique: each cell is its own file

Register each cell's **logical** content as its own ephemeral in-memory
`SourceFile` and make that the coordinate root. Unescaping happens once, at
ingestion, *before* source tracking begins, so no non-affine map ever needs
representing. The converter:

1. parses the notebook with plain `serde_json` (no spans needed — a pleasant
   consequence);
2. per cell, joins + unescapes `source` and registers it as a virtual file;
3. assembles the qmd cell by cell, building a `SourceInfo::Concat` of
   `Substring`/`Original` pieces over the per-cell files for verbatim content,
   and `Generated { by: By::raw("ipynb/scaffold", …), from: [anchor to cell] }`
   for synthesized text (fences, separators).

### Cell emission (Q1-compatible)

- **Markdown cells** — logical text verbatim (one `Substring` covering the
  cell); `\n\n` separators as `Generated`.
- **Raw cells** — wrapped in the raw block indicated by the mime hint
  (`Generated` fences around a `Substring` body).
- **Code cells** — ` ```{lang} ` fence (`Generated`, anchored to the cell) +
  source verbatim (`Substring`) + closing fence. `lang` from
  `nb.metadata.kernelspec.language`. `#|` option lines inside the cell source
  flow through the **existing** cell-options machinery, whose concat's parent
  is our concat — so YAML option diagnostics land in the right cell
  automatically. This composition is the flagship win; test it explicitly.
- **Front matter** — Q1 semantics: a leading raw/markdown cell starting with
  `---` YAML becomes document front matter, and because it is cell content it
  stays source-mapped (YAML errors point at `foo.ipynb[cell 1]` with real
  squiggles). Notebook-level `nb.metadata` merges via the config layer, not by
  synthesizing YAML text.

### Presentation

- **(P1, prototype)** encode the label in the virtual file's path —
  `foo.ipynb[cell 3, markdown]`. Zero upstream change, exact target UX in text
  output. Costs: the OSC-8 hyperlink points at a non-existent path (suppress,
  or link plain `foo.ipynb`), and `--json-errors` reports the pseudo-path as
  `file`.
- **(P2, before "done")** structured origin upstream: a `FileMetadata`
  extension in `quarto-source-map` (`origin: Option<FileOrigin>` with
  `FileOrigin::NotebookCell { notebook_path, cell_index, cell_id, cell_type }`)
  plus `quarto-error-reporting` rendering — report titles, structured JSON
  location, real-notebook hyperlinks. Both crates are posit-dev-owned; the cost
  is two upstream releases + version bumps.

Ship P1, land P2 before calling the feature done — pseudo-paths in the JSON
wire shape are the kind of thing downstream tooling starts depending on.

Cell numbering: 1-based over *all* cells, qualified with cell type; carry
nbformat ≥4.5 `cell.id` in JSON output only.

### What this gives up

No physical byte offsets into the `.ipynb`. That matters only to a consumer
underlining raw JSON — essentially an LSP session with the notebook open *as
JSON*. Cell-aware consumers (Jupyter, Positron, VS Code notebooks) address
positions as (cell, line, col), which is exactly what we produce. Two
escalation paths if raw offsets are ever needed: a converter-level decode run
table stored beside the virtual file (no `SourceInfo` change), or reintroducing
`Transformed { parent, runs }` upstream (which would also fix `quarto-yaml`'s
quoted/block-scalar imprecision — same problem class). Neither blocks this
feature; both need a concrete consumer first.

---

## Stored outputs

Q1 renders code cells *with their stored outputs* — that is the point of
rendering a notebook. Two options:

- **(A)** bake outputs into the qmd at conversion (Q1's approach) — simple and
  single-pass, but injects large `Generated` regions into the source-mapped
  document and duplicates output formatting the jupyter engine already has;
- **(B)** stored-output replay at the engine layer — the converter emits clean
  qmd (code cells as plain fenced blocks) and a small "stored notebook outputs"
  engine formats each cell's stored `outputs` array through the same
  `format_outputs`/`render_cell` machinery `text_execute.rs` uses for live
  kernel results (nbformat stored outputs and Jupyter wire results share the
  mime-bundle shape).

**Choose B.** It reuses the one mime-bundle→markdown implementation we
maintain, keeps the converter a pure function of bytes (which is what makes it
a *content processor*), and mirrors the existing `ReplayEngine`/capture-splice
precedent. Known inherited limitation: image outputs currently render as
placeholders (bd-5t6wvu7m) — acceptable for a first cut, and fixed in both
paths at once.

`.ipynb` renders **without execution by default** (Q1 parity — stored outputs
are the document). `--execute` / `execute.enabled: true` routes through the
existing jupyter engine unchanged, since it already executes fenced blocks from
qmd text. `suggested_engine` comes from kernelspec.

---

## What 7b must provide (forward-compatibility)

7b's registry makes ipynb additive on four axes (name-keyed registry, open
`processor:` schema, general `SourceInfo` enum, `ProcessorContext` for asset
writing). This plan adds a fifth requirement that 7b's current shape does
**not** meet, and that percent/spin will never surface:

> **`Converted` needs a channel for ephemeral source files.** 7b defines
> `Converted { markdown, source_info }`. Percent and spin map back into the
> *original* file, which the caller already registered — they need nothing
> more. ipynb's pieces point at **virtual per-cell files that do not exist on
> disk**, so the processor must hand them back for registration:
> `files: Vec<(String, String)>` (label, logical content) on `Converted`, or a
> registration handle on `ProcessorContext`.

Without it, 7c forces a change to the trait's return type — precisely what
7b's obligations exist to prevent. It is cheap now and expensive later.

---

## Implementation seams that need closing

Found in the July-2026 audit of the live code; each is a work item:

1. **Syntax-error diagnostics bypass `parent_source_info`.**
   `produce_diagnostic_messages(input_bytes, …, &context.source_context)`
   (`readers/qmd.rs`) builds locations in the parse buffer's own coordinates and
   never consults `context.parent_source_info`. qmd *syntax errors* are the
   headline use case, so this must be threaded through (wrap produced locations
   as `Substring` of the parent). Likely benefits the existing cell-options path
   too.
2. **Source-context plumbing on every path.** The per-cell virtual files must be
   registered in **every** `SourceContext` that reaches the diagnostic renderer:
   pampa builds its own inside `read`, and `run_pipeline` *rebuilds* one on the
   error path. Miss either and squiggles silently drop.
3. **Cross-cell error ranges.** A parser span can straddle a `Concat` piece
   boundary (an unclosed fence in cell 2 swallowing cell 3); the renderer
   resolves `root_file_id()` to one file and degrades. Mitigation (k-zr88's
   "require well-formed cells"): a cheap pre-parse gate that parses each
   markdown cell standalone and reports ill-formedness per cell *before* the
   concatenated parse. Then a cross-piece span indicates a Quarto bug, and the
   renderer can fall back to "cell N through cell M" without a snippet.

---

## Phased checklist (TDD — tests first)

### Phase 0 — prerequisites
- [ ] 7b landed (registry, `processor:` schema, `ProcessorContext`), including
      the `Converted` source-file channel above.
- [ ] Confirm the stored-output replay decision (option B) with Gordon.

### Phase 1 — converter core + source mapping (the design's proof)
- [ ] Fixture notebooks (tiny, per-feature) under a converter test dir.
- [ ] TDD: markdown/raw cell conversion; front-matter extraction; assembled-qmd
      snapshot tests.
- [ ] TDD — *the* source-location test: malformed markdown in cell N produces a
      diagnostic labeled `foo.ipynb[cell N]`, correct in-cell line/col, snippet
      showing logical (unescaped) text. Must fail before the seam fixes.
- [ ] Thread `parent_source_info` through `produce_diagnostic_messages` (seam 1).
- [ ] Per-cell well-formedness pre-check (seam 3).

### Phase 2 — registry wiring
- [ ] `ipynb` processor entry in `content_processors/`; jupyter declares
      `claims-files: [{extension: .ipynb, processor: ipynb}]` as static data.
- [ ] `SourceContext` plumbing on success **and** error paths (seam 2).
- [ ] Cell-options composition test: a `#|` YAML error inside a code cell lands
      in the right cell.
- [ ] Pass-1 discovery admission via the processor's `sniff` (7b's tier), and a
      **launch-free assertion**: a project of N notebooks issues zero engine
      launches in Pass-1.
- [ ] End-to-end per CLAUDE.md: `cargo run --bin q2 -- render fixture.ipynb`,
      inspect the output, inspect a deliberately-broken fixture's terminal
      diagnostic; record invocation + snippets here.

### Phase 3 — code cells with stored outputs
- [ ] Stored-output replay engine (option B), reusing `format_outputs`.
- [ ] `--execute` route-through test (existing jupyter engine).
- [ ] Q1 comparison render on a real-world notebook.

### Phase 4 — presentation hardening (upstream)
- [ ] `FileOrigin` structured metadata in `quarto-source-map` +
      `quarto-error-reporting` rendering (replaces the P1 pseudo-path).
- [ ] `--json-errors` structured cell locations.
- [ ] Hyperlink behaviour for virtual files.

### Phase 5 — coordination
- [ ] Point bd-19nc56ao, bd-xxul, k-zr88 at this plan; record the
      supersession of the July-20 doc's attachment point.
- [ ] User docs (usage, not internals): rendering `.ipynb` inputs.

---

## Open questions

1. **Per-cell file scaling.** One ephemeral `SourceFile` per cell means a
   500-cell notebook registers 500 files in `SourceContext`. Nothing in the
   design sizes this. Measure before Phase 2 — it is the one place the
   technique could fail to scale.
2. **Cell numbering** — 1-based over all cells (recommended) vs. per-type
   counters vs. Jupyter execution counts. `cell.id` in JSON only?
3. **Pseudo-path MVP** — acceptable intermediate state given `--json-errors`
   consumers would briefly see it as `file`?
4. **Concatenated-document semantics** — Q1 concatenates all cells into one
   markdown document (cross-cell footnotes/links work); Jupyter renders cells
   independently (they don't). Recommendation: concatenate (Q1 parity) + the
   per-cell well-formedness gate. Confirm?
5. **k-zr88's remaining scope.** Its ipynb half is superseded here; its
   plain-text half is superseded by 7b (which lands the format and defers
   persistence). Does anything remain, or does it close?

## Deferred / explicitly out of scope

- ipynb-filters (`2026-04-23-ipynb-filters-and-engine-partitioning.md`).
  Note for later: filters rewrite notebook JSON pre-conversion, so all mapping
  targets the *filtered* notebook; cell-coordinate reporting stays meaningful
  as long as filters preserve cell identity — all Q1 promises either.
- Raw-JSON offset mapping (the `Transformed` run-table escape hatch).
- LSP integration; WASM/hub exposure (the converter is pure Rust + `serde_json`,
  so it ports without new surface area, but not in the first cut).
- Project-discovery policy for `.ipynb` as a *default* input — that is bd-xxul;
  7b's admission tier supplies the mechanism, the policy is a separate call.

## References

- 7b (registry, seams): `2026-07-08-plan7b-native-content-processors.md`
- Design body absorbed from: `2026-07-20-ipynb-surface-syntax-design.md`
- Attachment decision: `2026-08-13-ts-engine-extensions-merge-main.md` §D3
- Conversion provenance prerequisite: bd-zlemoc6w
- Q1 source: `external-sources/quarto-cli/src/core/jupyter/` (`jupyterToMarkdown`,
  `mdFromCodeCell`); port reference `ts-packages/quarto-api/src/jupyter/`
