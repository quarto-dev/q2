# Plan 7c — ipynb content processor

**Series root:** [2026-06-27-plan7-native-percent-spin-sourceinfo.md](2026-06-27-plan7-native-percent-spin-sourceinfo.md)
**Depends on:** [2026-07-08-plan7b-native-content-processors.md](2026-07-08-plan7b-native-content-processors.md) (the registry, the `ProcessorContext`, the source-file channel)
**Absorbs the design body of:** [2026-07-20-ipynb-surface-syntax-design.md](2026-07-20-ipynb-surface-syntax-design.md) (the source-location design, cell emission rules, stored-output decision, and implementation seams — re-homed here; that doc's *attachment point* is superseded, see below)
**Strand:** bd-19nc56ao (p1). Related: bd-xxul (discovery), k-zr88
(structured-format source info — closed superseded 2026-09-24, open
question 5), bd-kik3s1vt (transcript surface). (bd-zlemoc6w, previously
listed here as a dependency, was closed obsolete 2026-09-24 — see
§ Review corrections.)
**Date:** 2026-07-08 (rewritten 2026-08-17 — promoted from placeholder)
**Status:** IN EXECUTION — Phase 0 complete (2026-09-24); Phase 1 in-repo work
complete (2026-09-24: converter core green, flagship mapping half green,
clippy + full-crate gates green). Remaining Phase 1 work is the upstream
quarto-error-reporting engagement (needs Gordon coordination). Architecture
settled; see § Execution decisions.
**Reviewed for staleness/consistency 2026-09-24** (see § Review corrections below);
second pass — clarity/consistency/implementability — same day (see § Review-2 pass);
third pass — round-2 claim verification — same day (see § Review-3 pass).
**Blocked, not ready to execute:** ~~Plan 7b is itself 0/33, not started.~~
**Superseded 2026-09-24:** 7b landed on `plan7b-content-processors` (see Phase 0);
this plan is in execution.

---

## Review corrections (2026-09-24, session "plan-7c-review")

Staleness/consistency review, not execution — mirrors the review just completed on
sibling Plan 7b. All three findings below are now resolved: two were stale-claim
corrections; the third was a genuine dependency question surfaced to Gordon,
resolved by closing bd-zlemoc6w as obsolete.

1. **"Implementation seams that need closing" §1 is partly stale — the general
   mechanism already landed.** The plan says `produce_diagnostic_messages`
   "never consults `context.parent_source_info`" and that this "must be threaded
   through." Code-checked 2026-09-24: `crates/pampa/src/readers/qmd.rs` already
   reroots Err-path diagnostics through `parent_source_info` via
   `reroot_diagnostics_into_parent` (wraps each diagnostic location as
   `SourceInfo::substring(parent, start, end)`, exactly the fix this section asks
   for), landed **2026-08-20** in `4e116ba50` ("Forward the underlying markdown
   diagnostic through Q-1-20 for config values") — after this plan's last rewrite
   (2026-08-17) and covered by its own test,
   `err_path_diagnostics_reroot_through_parent_source_info`. The mechanism was built
   for a different recursive-parse case (embedded config-value strings), but it is
   general: any top-level `read()` call that supplies a `parent_source_info` gets
   Err-path rerooting for free. Phase 1's "Thread `parent_source_info` through
   `produce_diagnostic_messages` (seam 1)" task is downgraded accordingly — see the
   phased checklist.
2. **"What 7b must provide (forward-compatibility)" is stale — 7b already provides
   it.** The section frames the `Converted.files` ephemeral-source-file channel as
   "a fifth requirement that 7b's current shape does not meet." Checked against 7b's
   plan file directly: `Converted.files: Vec<(String, String)>` and its doc comment
   naming 7c's ipynb processor by name have been in 7b's plan since **2026-08-18**
   (`5f8c0338c4`, the day after this plan's last rewrite), with matching checklist
   items. Nothing to ask 7b for; the section now reads as confirmation, not a request.
3. **Dependency question — bd-zlemoc6w, resolved: closed as obsolete (2026-09-24).**
   The grand-plan table listed `bd-zlemoc6w (provenance)` as a 7c dependency, and
   bd-zlemoc6w's own text asserted "Plan 7c (.ipynb) depends on it for provenance."
   But bd-zlemoc6w is about the **wire path** (`TsMappedStringWithMap`/
   `markdown_for_file`'s dropped `source_map`, for a TS engine converting a file
   over the wire), and this plan's ipynb converter is **native Rust, never touches
   the wire path at all** (plain `serde_json` + per-cell virtual `SourceFile`s
   built directly in-process). Gordon's call: the dependency claim came from an
   earlier point when we still expected TS engines to do input-format conversion
   over the wire and worried about that path's provenance; we since established
   that TS engines can't do input-format conversion at all — that's exactly the
   Pass-1 engine-launch bug 7b/7c exist to avoid — so the wire path bd-zlemoc6w
   targets is being retired, not fixed. 7b and 7c both convert natively and produce
   their own in-process `SourceInfo`, which **deletes** the dependency on that wire
   path rather than needing it (the same logic bd-zlemoc6w's own comment already
   applied to 7b's percent/spin processors, just not carried through to 7c).
   bd-zlemoc6w is now closed obsolete; the grand-plan table's dependency column for
   this row is corrected below to drop it.

---

## Execution decisions (2026-09-24, session "impl-7c")

Phase 0 confirmations from Gordon, recorded before Phase 1 begins:

1. **Stored outputs = option B, confirmed.** Engine-layer replay reusing
   `format_outputs`/`render_cell` (Phase 0 item 2).
2. **Document semantics: concatenate, confirmed** — resolves open question 4.
   All cells concatenate into one qmd document, and **cross-cell structure
   must work**: a markdown cell may open a fence or div that a later cell
   closes (Q1 builds panel/tabset structure this way). The converter emits
   cells verbatim and treats the *concatenated parse* as the source of
   document structure.
3. **Seam 3 amended: the hard per-cell standalone-parse gate is dropped**
   (Gordon: "yes we need to drop the hard per-cell gate. my point exactly.").
   The gate would hard-error on exactly the legitimate cross-cell structure
   decision 2 protects. What remains of seam 3 is the renderer fallback: a
   parser span straddling a `Concat` piece boundary is detected explicitly and
   rendered as "cell N through cell M" with no snippet — never silently
   clamped to one piece. May need a small upstream accommodation in
   `quarto-error-reporting` (posit-dev-owned). § Implementation seams item 3
   and the Phase 1 checklist are rewritten accordingly.
4. **Title snipping: yes** ("yes on title") — port Q1's `fixupFrontMatter`
   (`external-sources/quarto-cli/src/core/jupyter/jupyter-fixups.ts:146`;
   semantics re-verified 2026-09-24). The first raw *or* markdown cell whose
   text starts with YAML front matter becomes the front-matter cell; if that
   YAML has no `title`, the first markdown cell that is *only a heading*
   (nothing before the heading) has that heading snipped into `title`
   (remaining lines stay in the body). Note: the TS port
   (`ts-packages/quarto-api/src/jupyter/`) does **not** carry the fixups —
   port from Q1 directly. When snipping with no front-matter cell present, Q1
   synthesizes a front-matter cell; 7c does the same but as **`Generated`
   content** anchored to the heading cell — distinct from the nb.metadata
   rule, which forbids synthesizing YAML for *notebook metadata* (that merges
   via the config layer). **Numbering interaction (review-2):** Q1 implements
   the synthesis by unshifting a new raw cell at position 0
   (`jupyter-fixups.ts:216-221`), so the synthesized cell occupies slot 1 and
   shifts every later cell's index. 7c matches (Q1 parity is this decision's
   own rationale): the synthesized cell takes a number in the 1-based
   all-cells count and shifts the rest. Its content is `Generated`, so a
   diagnostic anchored in it resolves through the heading-cell anchor — the
   pseudo-path label is only reachable via that anchor.
5. **Cell numbering: adopted as planned** — 1-based over all cells, qualified
   with cell type (resolves open question 2).
6. **Per-cell FileId contract** (converter ↔ caller): the converter builds
   each cell's `SourceInfo` pieces assuming cell file ids
   `ORIGINAL_FILE_ID + 1 + i` in `Converted.files` order;
   `ParseDocumentStage` (and every `SourceContext` rebuild site) must register
   the original file at `ORIGINAL_FILE_ID` and each ephemeral cell file at its
   assumed id, in order. Additive: percent/spin return empty `files` and are
   unaffected.
   **Collision analysis (review-2, code-checked):** pampa's `read` registers
   the converted buffer first via sequential `add_file` → `FileId(0)`
   (`readers/qmd.rs:117-120`; `add_file` assigns `FileId(len)`, 0.2.0
   `context.rs:59`), and `ORIGINAL_FILE_ID` is `FileId(1)`. Cell ids `2..=N+1`
   are contiguous with that prefix, which is load-bearing: a later sequential
   registration (e.g. `IncludeExpansionStage`) computes `FileId(len)`, and
   `get_file` consults the sparse `file_id_map` *before* vec position
   (0.2.0 `context.rs:155-162`) — a gapped cell id space would make that next
   id land on a mapped cell and **silently misbind**. Register in order, no
   gaps. Quarto-yaml hash ids are registered after parse in pipeline order and
   skip already-taken ids (`metadata_merge.rs:322-328`), so cells-first is
   safe. `add_file_with_id` itself panics on duplicates (0.2.0
   `context.rs:118-122`) — rebuild sites that can re-enter must guard like
   metadata_merge does. **Transport gap (resolved 2026-09-24, open
   question 7 — option (a)):** `Converted.files` is currently dropped at the
   engine-trait bridge (`traits.rs:281`), but with the stage calling
   `convert` directly for processor-bearing claims that bridge is never in
   the path — the files flow stage → `LoadedSource.files` → registration
   sites, and the trait/wire infra stays untouched.

Phase 0 checklist items both check off below. Baseline at branch HEAD: full
`cargo xtask verify --skip-hub-build --skip-hub-tests` green — **14793 passed
/ 200 skipped / 0 failed** (`/tmp/plan7c-verify-head.log`, re-read from the
log 2026-09-24); this is the phase-delta baseline.

---

## Review-2 pass (2026-09-24, session "review-7c-3" — clarity / consistency / implementability)

Second review pass, after the Phase 0 decisions were recorded. Results:

**Verified sound.** The post-amendment sweep is clean — every surviving mention
of the dropped per-cell gate (§ Execution decisions item 3, § Implementation
seams item 3, Phase 1 checklist, open question 4) *describes* the drop; nothing
still assumes the old semantics. Every `quarto-source-map` 0.2.0 construction in
this plan verifies against the published source: `original`/`substring`/
`concat(Vec<(SourceInfo, usize)>)`, `Generated` (`from` is a
`SmallVec<[Anchor; 2]>`; `generated_with` accepts `impl Into<…>`), `Anchor {
role, source_info: Arc<SourceInfo> }`, `By::raw(impl Into<String>,
serde_json::Value)`, `ProvenanceBuilder::{in_file, in_parent, verbatim,
replacement, finish}`. Q1's `fixupFrontMatter` semantics match § Execution
decisions item 4 as written (first raw-or-markdown cell with YAML; heading-only
snip; synthesized front-matter cell when none exists).

**Finding A — the flagship test's *rendering* half cannot pass in-repo (open
question 6).** quarto-error-reporting 0.3.0 resolves the report's *file* via
`root_file_id()`, which on a `Concat` returns the **first rooted piece**
(`diagnostic.rs:819`), while offsets resolve piece-aware via `map_offset`
(`diagnostic.rs:838-849`). For ipynb's multi-file `Concat`, every diagnostic
rooted in cell > 1 renders with **cell 1's** path label and snippet text at
cell-N offsets. Percent/spin are unaffected — all their pieces root to the same
`ORIGINAL_FILE_ID`. Seam 3's scope is widened accordingly; the fix is small but
upstream, and it lands in **Phase 1**, not Phase 4.

**Finding B — `Converted.files` has no transport past the conversion boundary
(open question 7).** The only production chain is `SourceConversionStage` →
`markdown_for_file` → `native_markdown_for_file`, which maps the `Converted`
away — `.map(|converted| (converted.markdown, converted.source_info))` drops
`files` at `traits.rs:281` — and neither the engine-trait return type nor
`LoadedSource` can carry them (`LoadedSource.conversion` holds only the engine
name, `data.rs:223-228`). `convert`'s signature stays fixed; the transport from
its caller to `ParseDocumentStage` and the error-path rebuild sites is the
missing piece. Until it is chosen, decision 6's registration obligation is
unimplementable.

**Editorial corrections folded into the sections below:** stale "Blocked, 7b is
0/33" header note (7b has landed); the flagship test never said its Phase 1
harness builds its own `SourceContext`; § Cell emission's "stays source-mapped"
overclaimed for the *synthesized* front-matter cell; Q1 synthesizes that cell by
**unshifting at position 0**, which shifts every later cell number (decision 4 ×
item 5 interaction, resolved by Q1 parity); raw cells without a mime hint fall
back to plain content in Q1; seam 2 now names all four `SourceContext` sites
(`pipeline.rs:896` registers nothing today — a live percent/spin latent gap);
decision 6 now records the FileId collision/contiguity analysis; Phase 1
fixtures are enumerated and pinned to `tests/integration/`; the scaling gate is
now a Phase 2 checklist item.

**Both findings resolved same day (Gordon):** Finding A — the upstream fix
lands as a PR to `posit-dev/quarto-error-reporting` (we own the repo; PR
first, release when needed) and is released + version-bumped **inside
Phase 1** (open question 6, resolved). Finding B — transport via the stage
calling `content_processors::convert` directly plus an additive
`LoadedSource.files` field (open question 7, resolved — option (a)).
Open question 5's strand (k-zr88) closed superseded the same day.

---

## Review-3 pass (2026-09-24, session "review-7c-4" — final pre-execution verification)

Post-resolution verification: every load-bearing claim the round-2 edits
(`d0883a672`) added was re-checked against primary sources, not the plan's
citations. **All verified sound.** Upstream `quarto-error-reporting`:
`origin/main` tip is `287d645` and its `src/` files are byte-identical to the
registry 0.3.0 q2 pins; `diagnostic.rs:819` (ariadne `root_file_id`),
`:925`/`:930` (detail filter silently skipping other-file details), the same
two patterns in the `#[cfg(feature = "annotate-snippets")]` copy at
`:1022`/`:1077` (ariadne is its own separately-gated default feature), and the
piece-aware `map_offset` resolution at `:838-849` all read as claimed;
`coalesce.rs:170` plus the module-doc contract confirm the benign singleton
pass-through; source-map 0.2.0 behaves as Finding A assumes
(`resolve_byte_range` → `None` for `Concat`, `root_file_id` → first rooted
piece, `MappedLocation.file_id` available to the proposed fix); the
Trusted-Publishing workflow (`4865082`), the verify-mirror/braid CLAUDE.md,
and the v0.2.1 → v0.2.2 → v0.3.0 tags all exist; the local docs branch has no
origin counterpart (genuinely unpushed). q2 side: `source_conversion.rs`
claimer tuple (:156), `native_claims_file` match (:205-212),
`markdown_for_file` (:229), the exactly-once test (:891 — which models a
non-processor `.echo` claimer, so the Phase 2 reroute of the processor arm
doesn't disturb it); only TsEngine overrides `markdown_for_file` in
production; `traits.rs:281` drops `files`; the file is read once in the sniff
(`native_claims_file`) and once in the convert path (`native_markdown_for_file`);
`LoadedSource` has no `files` field yet; `ts_engine.rs:1106` native-first
override with the wire cache returning `Generated(By::unknown())` placeholders
and the `(String, SourceInfo)` trait signature unable to carry files at all;
workspace req `"0.3.0"` is caret (a 0.3.1 satisfies it); k-zr88 closed with a
close reason matching open question 5's text. Coherence sweep of every
open-question-5/6/7 and k-zr88 mention: clean; the § Review-2 editorial list
all present; the two upstream engagements (Phase 1's error-reporting 0.3.1
patch vs Phase 4's source-map+error-reporting `FileOrigin` minor) do not blur.

**No design findings. Three editorial fixes applied:** the "Research-2"
shorthand in open question 5 had no antecedent (now "Review-2's research");
seam 2's "every rebuild site receives them the same way it already receives
`source_info`" overclaimed for `pipeline.rs:896`, which receives neither
`source_info` nor files today (reworded: sites that receive neither gain
both); the coalesce contract cite `:43-45, 61-65` corrected to `:50-61` /
`:182-184` where the contract actually sits.

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
  (`Generated` fences around a `Substring` body). With **no** mime hint, Q1
  emits the cell as plain content, not a raw block (`mdFromRawCell`,
  `jupyter.ts:1082-1100`) — port that fallback.
- **Code cells** — ` ```{lang} ` fence (`Generated`, anchored to the cell) +
  source verbatim (`Substring`) + closing fence. `lang` from
  `nb.metadata.kernelspec.language`. `#|` option lines inside the cell source
  flow through the **existing** cell-options machinery, whose concat's parent
  is our concat — so YAML option diagnostics land in the right cell
  automatically. This composition is the flagship win; test it explicitly.
- **Front matter** — Q1 semantics: a leading raw/markdown cell starting with
  `---` YAML becomes document front matter, and because it is cell content it
  stays source-mapped (YAML errors point at the owning cell with real
  squiggles). That mapped claim holds for a *genuine* YAML cell; the
  title-snipping *synthesized* cell is `Generated` anchored to the heading
  cell instead (§ Execution decisions item 4, including its numbering
  shift), plus `fixupFrontMatter` title snipping. Notebook-level
  `nb.metadata` merges via the config layer, not by synthesizing YAML text.

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

### Phase 3 wiring decision (finalized 2026-09-25)

The execute-vs-replay choice lives in **`EngineExecutionStage`**, before the
policy gate and before Step 2's availability resolution (P2-12) — that
placement is forced by the Phase 2 e2e finding: resolution for `.ipynb` is
claim-short-circuited to jupyter *even for zero-code-cell notebooks*, and
P2-12 hard-errors when jupyter is registered but unavailable. Any decision
made *inside* `JupyterEngine::execute` is too late; the default path must
never touch engine availability.

- **Replay engine**: `IpynbReplayEngine` in
  `engine/jupyter/stored.rs` — a real `ExecutionEngine` (name `"ipynb"`,
  `is_available()` always true, `intermediate_files` mirrors jupyter's
  `<stem>_files` declaration) but **not registered in `EngineRegistry`**;
  the stage selects it explicitly. It rides the stage's existing per-engine
  loop unchanged (mask → serialize → execute → unmask → capture → reparse →
  reconcile), so capture/splice works for replayed notebooks for free.
- **Trigger**: input extension is `.ipynb` (case-insensitive) **and**
  merged metadata `execute.enabled` is not `true`. Replayed documents are
  complete (stored outputs *are* the document): the policy gate is skipped
  too (replay is not execution — it is kernel-free and cheap, so
  `preview --static` / `engine: off` still shows stored outputs, closing
  e2e finding 3), and `execution_skipped` stays `false`.
- **Alignment without order-guessing**: `parse_code_blocks` finds each
  `{lang}` fence in the serialized qmd; mapping `block.code_start` through
  `ctx.source_info.map_offset` lands in the owning cell's virtual file —
  `FileId(ORIGINAL_FILE_ID.0 + 1 + i)` by the converter's construction —
  which *is* the notebook cell index. A fence that maps into a **markdown**
  cell (a literal ```{python} fence in prose) stays inert. The notebook
  bytes come from `source_context.get_file(ORIGINAL_FILE_ID)` (registered
  by `ParseDocumentStage`; missing = broken invariant → loud error), so no
  re-reading and no path guessing.
- **Execution demanded** (`execute.enabled: true` from the notebook's
  front-matter cell, merged by MetadataMergeStage): falls through to
  today's path untouched — policy gate, Step 2, P2-12, jupyter live
  execution. A CLI `--execute` flag does not exist yet and is out of Phase 3
  scope; metadata is the only demand route.
- **Stored error outputs** render as `-error` divs subject to output
  visibility; the `error:` abort policy is live-execution semantics and does
  not apply to content that already happened.

---

## What 7b must provide (forward-compatibility)

7b's registry makes ipynb additive on four axes (name-keyed registry, open
`processor:` schema, general `SourceInfo` enum, `ProcessorContext` for asset
writing). This plan identified a fifth requirement, that percent/spin will
never surface:

> **`Converted` needs a channel for ephemeral source files.** 7b defines
> `Converted { markdown, source_info }`. Percent and spin map back into the
> *original* file, which the caller already registered — they need nothing
> more. ipynb's pieces point at **virtual per-cell files that do not exist on
> disk**, so the processor must hand them back for registration:
> `files: Vec<(String, String)>` (label, logical content) on `Converted`, or a
> registration handle on `ProcessorContext`.

**Already satisfied (confirmed 2026-09-24).** 7b's plan added exactly this —
`Converted.files: Vec<(String, String)>`, with a doc comment naming this
processor by name — on 2026-08-18, the day after this plan's last rewrite (see
§ Review corrections). Nothing outstanding here; this section is now a record
of why the field exists, not an ask.

---

## Implementation seams that need closing

Found in the July-2026 audit of the live code; each is a work item:

1. **Syntax-error diagnostics bypass `parent_source_info` — partially closed
   already (confirmed 2026-09-24, see § Review corrections).**
   `produce_diagnostic_messages(input_bytes, …, &context.source_context)`
   (`readers/qmd.rs`) builds locations in the parse buffer's own coordinates and
   used not to consult `context.parent_source_info` at all. As of `4e116ba50`
   (2026-08-20), the caller in `readers/qmd.rs` now rewraps every Err-path
   diagnostic through `reroot_diagnostics_into_parent` whenever
   `context.parent_source_info` is `Some` — general, not ipynb-specific, and
   already exercised by `err_path_diagnostics_reroot_through_parent_source_info`.
   Remaining work for this plan is to **confirm** the mechanism covers the
   ipynb per-cell case end-to-end (a `Concat` of per-cell `Substring`/`Original`
   pieces as the supplied `parent_source_info`, not a single `Original`) rather
   than to build the threading fresh. Concretely (review-2): the reroot itself
   (`qmd.rs:274-291`) rewraps raw-buffer locations as
   `substring(parent, …)` and composes for *any* parent shape, so "confirm"
   means: drive `read()` with an ipynb-shaped multi-piece `Concat` parent and a
   harness-built `SourceContext` registering the per-cell files, then assert
   each rerooted diagnostic's location `map_offset`s into the owning cell
   (right file id, in-cell row/col). "Extend" only if a pass between
   `produce_diagnostic_messages` and the reroot bakes in a single-file
   assumption. Likely still benefits the existing cell-options path too.
2. **Source-context plumbing on every path.** The per-cell virtual files must be
   registered in **every** `SourceContext` that reaches the diagnostic renderer.
   Site inventory (review-2): `parse_document.rs:163/171` (the two contexts,
   kept in lockstep — already register `ORIGINAL_FILE_ID` for percent/spin;
   cells extend these, extending the existing `lint:allow` markers);
   `pipeline.rs:896` (the `StageError` rebuild — registers **nothing** today,
   not even `ORIGINAL_FILE_ID`, so percent/spin provenance already drops
   there); `pipeline.rs:934` (`parse_qmd_to_ast`'s output context);
   `pipeline.rs:1108` (q2-preview). Miss any and squiggles silently drop. The
   `pipeline.rs:896` gap is a live 7b-era latent bug — fixing it for cells
   fixes percent/spin too. How the files *reach* these sites is settled
   (open question 7, resolved — option (a)): the stage stamps
   `LoadedSource.files`; rebuild sites receive them alongside
   `source_info` — a site that receives neither today (`pipeline.rs:896`)
   gains both.
3. **Cross-cell error ranges — amended 2026-09-24 (Gordon): gate dropped.**
   The original mitigation (a pre-parse gate requiring each markdown cell to
   parse standalone) is **rejected**: legitimate notebooks open a fence or div
   in one markdown cell and close it in a later one (Q1 builds tabset/panel
   structure that way — § Execution decisions item 2), and the gate would
   hard-error on exactly those. What remains, and review-2 code-checked the
   render side to be *already* piece-hostile beyond straddles:
   `render_with_ariadne_in_context` takes the report's file from
   `root_file_id()` — first rooted `Concat` piece (quarto-error-reporting
   0.3.0 `diagnostic.rs:819`) — while offsets resolve piece-aware via
   `map_offset` (`diagnostic.rs:838-849`), so a diagnostic rooted **wholely
   within cell N > 1** still renders cell 1's label and snippet at cell-N
   offsets. The upstream accommodation covers both: (a) take the per-span file
   from the mapped start (`start_mapped.file_id`, which `map_offset` already
   returns), and (b) a span straddling a piece boundary must be **detected
   explicitly** (`start_mapped.file_id != end_mapped.file_id`) and rendered as
   "cell N through cell M" with no snippet — never silently clamped to one
   piece. Upstream status (review-2 follow-up, code-checked against the
   upstream repo — origin/main `287d645` = released 0.3.0 = q2's registry
   pin): the bug is in **both** renderer copies — ariadne
   (`diagnostic.rs:819` main, `:925` detail filter) and the feature-gated
   annotate-snippets copy (`:1022`, `:1077`) — and the detail filter
   additionally **silently skips** any detail rooted in a file other than
   the report's root. `coalesce.rs:170` checked benign: `Concat`-parented
   locations return `None` from `resolve_byte_range` and pass through as
   singleton groups that print exactly once (documented contract,
   `coalesce.rs:50-61` and `:182-184`) — no fix needed there. The fix lands as an
   upstream PR + release in Phase 1 (open question 6, resolved — Gordon:
   full upstream access, PR first, release when needed).

---

## Phased checklist (TDD — tests first)

### Phase 0 — prerequisites
- [x] 7b landed (registry, `processor:` schema, `ProcessorContext`), including
      the `Converted` source-file channel above. (2026-09-24: landed on
      `plan7b-content-processors`, tip `95a6dad36`; 7c branched off it.)
- [x] Confirm the stored-output replay decision (option B) with Gordon.
      (2026-09-24: confirmed — engine-layer replay reusing
      `format_outputs`/`render_cell`; see § Execution decisions item 1.)

### Phase 1 — converter core + source mapping (the design's proof)
- [x] Fixture notebooks (tiny, per-feature) as converter-test fixtures under
      `crates/quarto-core/tests/integration/` (new module registered in
      `tests/integration/main.rs`, per the repo's integration-test rule; pure
      converter pieces may also carry `#[cfg(test)]` unit tests beside the
      processor, as percent/spin do). Cover: markdown cell; raw cell with mime
      hint; raw cell without (plain-content fallback); code cell with `#|`
      options; genuine YAML front-matter cell; title snipping with front
      matter lacking `title`; title snipping with no front matter (synthesized
      cell — numbering shift, § Execution decisions item 4); cross-cell fence
      or div open/close; escaped text (`\n`, tab, non-ASCII) in a later cell;
      malformed markdown in cell N > 1 (flagship input).
      *(2026-09-24: 10 fixtures under `fixtures/ipynb/`; 26 unit tests in
      `ipynb.rs` + 4 integration tests in `ipynb_content_processor.rs`.)*
- [x] TDD: markdown/raw cell conversion; front-matter extraction; assembled-qmd
      snapshot tests. *(RED first confirmed — all new tests failed on the
      stub; then GREEN per the pinned expected strings.)*
- [x] TDD — the **mapping** half of the source-location test (the
      **rendering** half landed 2026-09-25 as
      `flagship_rendering_half_labels_owning_cell_with_snippet`, same file): malformed markdown in cell N > 1 flows
      through `read()` with the converter's `Concat` as `parent_source_info`;
      the **harness builds its own `SourceContext`** (per-cell registration is
      seam 2 / Phase 2, so Phase 1 registers `ORIGINAL_FILE_ID` and cells
      `2..=N+1` itself, in order per decision 6) and asserts each rerooted
      diagnostic's location `map_offset`s into the owning cell — right file
      id, in-cell line/col, snippet text logical (unescaped).
      *(2026-09-24: `flagship_malformed_cell_maps_diagnostics_into_owning_cell`
      green; all diagnostics + details map to FileId(3), row 0.)*
- [x] Confirm the already-landed `reroot_diagnostics_into_parent` mechanism
      (`4e116ba50`, 2026-08-20) covers the ipynb per-cell `Concat` case
      end-to-end; extend it if the general mechanism doesn't (seam 1 — see
      § Implementation seams and § Review corrections; downgraded from "build"
      to "confirm/extend" on 2026-09-24; the flagship test's mapping half is
      the confirming harness). *(2026-09-24: confirmed — no extension needed.)*

      **Two test-authoring corrections made during GREEN** (the tests were
      written pre-implementation; both corrections verified against upstream
      authorities, not against the implementation):
      1. `Location.row` is **0-indexed** (upstream quarto-source-map's own
         test is the authority); five assertions in the new tests claimed
         1-indexed rows and were fixed.
      2. `heading_with_content_before_it_is_not_snipped`'s expected string
         was unproducible — it claimed a blank line inserted by the
         separator *inside* a single verbatim cell, which no 1/2/3-cell
         reading produces. Q1 semantics (contentBeforeHeading ⇒ verbatim)
         pin the expectation at `"lead text\n# Not Snipped\n\nbody\n"`.
- [x] Cross-piece span fallback: detect spans straddling `Concat` piece
      boundaries and render "cell N through cell M" with no snippet (seam 3,
      amended 2026-09-24 — the hard per-cell gate was dropped, § Execution
      decisions item 3; lands via the upstream PR — open question 6,
      resolved — so this is in-phase).
      *(2026-09-24: landed upstream — posit-dev/quarto-error-reporting PR
      #7, merged `a7821b1d`, released 0.3.1.)*

### Phase 1 (upstream) — quarto-error-reporting fix

The upstream half of the flagship test, per Gordon's resolution of open
question 6 (2026-09-24: full access to `posit-dev/quarto-error-reporting`;
PR first, release when needed). All work happens on that repo's
conventions — its own `cargo xtask verify` mirror, braid tracking, CI across
feature sets.

- [x] Branch off `origin/main` (`287d645` — released 0.3.0, exactly q2's
      registry pin). Caution: the local checkout `~/src/quarto-error-reporting`
      is sitting on an **unpushed docs branch**
      (`docs/snap-span-char-boundaries-rationale`, ahead 1 of origin) —
      branch fresh from `origin/main`; do not disturb that working state.
      *(2026-09-24: resolved differently — Gordon identified the docs branch
      as his own stranded work and authorized renaming it for the fix while
      keeping the docs commit; branch renamed to
      `fix/concat-renderer-cross-piece`, rebased onto `287d645` conflict-free,
      docs commit now `384982e`.)*
- [x] Fix **both** renderer copies (§ Implementation seams item 3 has the
      full evidence): report file from `start_mapped.file_id` (which
      `map_offset` already returns) instead of `root_file_id()`; explicit
      straddle detection (`start_mapped.file_id != end_mapped.file_id`) →
      "cell N through cell M" label, no snippet; per-detail mapped file with
      the same-file filter corrected (it currently **silently skips**
      details rooted in other files). Sites: ariadne
      `diagnostic.rs:819`/`:925`, annotate-snippets `:1022`/`:1077`.
      *(2026-09-24: done; foreign-piece details render as their own source
      block in both renderers — ariadne multi-source `Cache` + order-1M
      labels, annotate-snippets extra group element; QER plan
      `claude-notes/plans/2026-09-24-concat-renderer-cross-piece.md`.)*
- [x] Tests in QER covering **both feature gates** (ariadne and
      annotate-snippets are separately `#[cfg]`'d): single-file passthrough
      (the percent/spin shape — all pieces root to one file), diagnostic
      rooted in the first `Concat` piece, in a later piece (today:
      wrong file label + snippet), a straddling span (today: silently
      clamped), and a detail in another piece (today: silently dropped).
      *(2026-09-24: 10 tests (5 per renderer), RED verified pre-fix, green
      both gates; `cargo xtask verify` all 6 checks green.)*
- [x] PR → merge → release via the repo's Trusted-Publishing workflow
      (expect a patch — 0.3.1: behavior fix, no API change; the 0.2.1 →
      0.2.2 → 0.3.0 cadence shows mid-phase releases are routine there).
      *(2026-09-24: PR #7 filed with Gordon's review of the draft, CI 4/4
      green, squash-merged `a7821b1d`; the merge itself triggered
      release.yml, which published 0.3.1 to crates.io (verified HTTP 200)
      and tagged `v0.3.1` — no manual tag step exists.)*
- [x] q2 side: `cargo update -p quarto-error-reporting` (the workspace req
      `"0.3.0"` is caret, so a 0.3.1 satisfies it; bump the req string too
      if the release lands a new minor). Then the flagship's rendering-half
      assertions go green in-phase. *(2026-09-25: lock 0.3.0 → 0.3.1; new
      test `flagship_rendering_half_labels_owning_cell_with_snippet` went
      RED on 0.3.0 — renderer drew `notebook.ipynb[cell 1, markdown]:1:17`
      with cell 1's "Some intro text" at cell-2 offsets — and GREEN on
      0.3.1: `╭─[ notebook.ipynb[cell 2, markdown]:1:39 ]` with cell 2's
      in-cell snippet `![logo](images/logo.svg){width="65px"
      .light-content}`. Test-side gotcha: `enable_hyperlinks: false`
      disables OSC-8 only; ariadne still emits SGR codes, so the test
      strips ANSI before asserting (nextest strips them when displaying,
      which masks the mismatch).)*

### Phase 2 — registry wiring
- [x] `ipynb` processor entry in `content_processors/`; jupyter declares
      `claims-files: [{extension: .ipynb, processor: ipynb}]` as static data.
      *(Done 2026-09-24: `ProcessorSpec::Ipynb` + `ProcessorParams::Ipynb`,
      registry insert, bare/map parse arms with "known processors: percent,
      spin, ipynb" errors, jupyter `.ipynb` static claim — builtin claims now
      6. Sniff left as the Phase-1 stub: admission is the separate Pass-1
      item below. TDD: 5 new/updated tests RED on compile, then GREEN;
      clippy + per-crate nextest 4973 passed / 32 skipped.)*
- [ ] `Converted.files` transport (open question 7, **resolved 2026-09-24 —
      option (a)**): `SourceConversionStage`'s processor-bearing arm (the
      `native_claims_file` → `Some(true)` branch, `source_conversion.rs:~205`)
      calls `content_processors::convert` directly instead of
      `engine.markdown_for_file` — today that trait hop's only job is to call
      `convert`, and the file is already read twice on this path (sniff,
      then convert), so nothing is added. The claimer tuple
      (`source_conversion.rs:~155`) becomes a small struct carrying
      `engine_name` / `qmd_text` / `source_info` / `files`; additive
      `LoadedSource.files: Vec<(String, String)>` (default empty — the wire
      path can never produce files; it returns `Generated(By::unknown())`
      placeholders by construction). Optional 6-line default trait helper
      `native_processor_for_file -> Option<ProcessorSpec>`, with
      `native_claims_file` refactored onto it, so the claim→spec lookup
      exists exactly once. `convert`'s signature untouched; TsEngine, the
      wire path, knitr/jupyter (which override nothing), and the 7b trait
      tests untouched.
      *(Done 2026-09-24: `ClaimedConversion` struct; `Some(true)` arm calls
      `content_processors::convert` directly, dynamic `None` arm keeps
      `markdown_for_file` + empty files; additive `LoadedSource.files`
      defaulting empty in both constructors; `native_processor_for_file`
      default helper with `native_claims_file` AND `native_markdown_for_file`
      refactored onto it. Sequencing note: the transport test forced the
      sniff body to land first — no `.ipynb` is ever claimed while
      `has_cells_array` was the `false` stub, so the stage test went
      "Can't determine execution engine". Sniff body implemented as JSON
      parse + top-level `cells` array with its own 4 RED→GREEN tests; the
      dispatch test's deferred sniff assertions re-enabled. TDD: transport +
      helper tests RED (E0609/E0599) before the change; clippy clean
      (one `is_ok_and` fix); per-crate nextest 4979 passed / 32 skipped.)*
- [x] `SourceContext` plumbing on success **and** error paths (seam 2 — site
      inventory in § Implementation seams item 2, including the
      `pipeline.rs:896` percent/spin latent gap). *(Done 2026-09-25: new
      `ConversionStash` on `StageContext` (engine/converted/source_info/files),
      set by `ParseDocumentStage` at the top of its run — before the parse, so
      parse failures carry it too. All four sites closed: (1) parse_document
      success block now also registers per-cell files into BOTH contexts at
      `FileId(ORIGINAL_FILE_ID.0 + 1 + i)` via `add_file_with_id` (same
      lint-allow rationale as the original-file registration); (2) the
      `run_pipeline` StageError arm rebuilds from the stash — converted buffer
      under the same synthetic name at FileId(0), then original, then cells,
      gated on `source_info.is_some()` exactly like the success path. Sequential
      `add_file` lands on the decision-6 ids by construction (qsm assigns
      `FileId(files.len())`), which also fixes the live 7b-era latent bug where
      a converted doc's parse error labeled raw notebook JSON as
      `notebook.ipynb` at FileId(0); (3)+(4) `parse_qmd_to_ast` and
      `render_qmd_to_preview_ast` carry `ast.source_context` instead of a fresh
      single-file context — byte-identical for .qmd, and the preview ASTContext
      keeps `filenames: vec![source_name]` verbatim (pampa's JSON writer interns
      Substring parent chains from node Arcs, not from that list). TDD: 3 tests
      RED (missing cell registration / FileId(0) name / preview context) then
      GREEN: T1 unit `test_parse_document_a_plus_registers_per_cell_files`;
      T2 stage-error integration asserting the rendered diagnostic carries
      `notebook.ipynb[cell 2, markdown]:1:` and cell 2's `![logo](images/logo.svg)`
      line; T3 preview integration through `render_qmd_to_preview_ast`. T3
      harness note: CROSS_CELL_FENCE_NOTEBOOK declares a kernelspec, so
      engine resolution routes to jupyter and fails on availability (jupyter
      not installed) before any assertion — set `ExecutionPolicy::None` on the
      test's RenderContext (the policy gate returns before the availability
      check; this test is about plumbing, not execution). Gates: clippy clean;
      per-crate nextest 4982 passed / 32 skipped (+3 vs 4979 baseline = the
      three new tests).)*
- [x] Per-cell file scaling gate (open question 1): measure a ~500-cell
      notebook's `SourceContext` registration + diagnostic-render cost before
      wiring; record the numbers here. *(2026-09-25: harness
      `scaling_gate_per_cell_registration_and_render` (ignored; run with
      `--run-ignored ignored-only --nocapture`), geometric 125/250/500/1000
      cells, ~150 B/cell. register: 249/255/522/532 µs — linear, trivial.
      map_offset × N calls: 46/156/566/1357 µs — quadratic in N (Concat
      walks pieces linearly per call), but the per-call constant is ~1.4 µs
      at 1000 cells, so one diagnostic's start+end mapping is ~3 µs; callers
      map each diagnostic's endpoints once, so the quadratic shape needs
      thousands of diagnostics to matter (≈3 ms at 1000 — still fine).
      render rooted in the last cell (worst-case walk): 264/72/78/44 µs —
      flat within noise. Verdict: the technique scales; no design change
      needed. If a future consumer ever maps O(N) locations per render, a
      piece-start binary search in qsm is the ready fix.)*
- [x] Cell-options composition test: a `#|` YAML error inside a code cell lands
      in the right cell. *(2026-09-25: fixture `code-cell-bad-options.ipynb`
      (markdown intro + code cell whose `#| error: [unclosed` is a scan-level
      YAML failure); test `cell_option_yaml_error_lands_in_owning_cell` in
      `ipynb_content_processor.rs`. The anchor is computed exactly as the
      production execute path computes it (`text_execute.rs:305-321`):
      `body_source = Substring(document source_info, code_start, …)` from the
      converter's Concat; the error's own location preferred, else the body
      source — the fallback is the live route because quarto-yaml's
      `From<ScanError>` carries `location: None`. Both routes must map into
      the owning cell; asserted FileId(3) + row 0, and the rendering half
      asserts `notebook.ipynb[cell 2, code]:1:` with the `#| error: [unclosed`
      snippet and a negative on `[cell 1`. GREEN on first run — the
      composition held as the design predicted ("whose concat's parent is our
      concat"), so this is a regression guard, not a bug fix; the "verify the
      test fails" TDD step doesn't apply (no bug). Clippy clean.)*
- [x] Pass-1 discovery admission via the processor's `sniff` (7b's tier), and a
      **launch-free assertion**: a project of N notebooks issues zero engine
      launches in Pass-1. *(The sniff BODY landed early, 2026-09-24 — see the
      transport item's sequencing note: `has_cells_array` = JSON parse with a
      top-level `cells` array, 4 unit tests. **Done 2026-09-25** — with a
      correction to this item's wording: "discovery admission via the
      processor's sniff" is superseded by 7b's landed 2026-09-24 correction
      (discovery-time sniffing dropped entirely; one predicate, one site at
      claim time). What admission actually rides is the **static claim**:
      jupyter's `claims-files: [{extension: .ipynb, processor: ipynb}]` lands
      `.ipynb` in `builtin_file_claims()`, and `ProjectContext::discover`
      chains those into `RenderableExtensions` for the walk (gate 1). The
      sniff still gates *conversion* at claim time, already proven. New test
      `discovery_pass1::project_of_notebooks_admitted_and_converted_launch_free`
      in `ipynb_content_processor.rs`: 3-notebook project + doc.qmd with the
      explicit `project.render: ["**/*.qmd", "**/*.ipynb"]` allowlist
      (.ipynb is deliberately never auto-discovered), admitted through
      production `ProjectContext::discover`, each converted through
      `render_qmd_to_preview_ast` (ExecutionPolicy::None — Pass-1 is
      conversion+parse, and the gate returns before jupyter's availability
      check). Launch-free assertion: `find_jupyter_call_count() == 1`
      absolute — the OnceLock-capped counter's process max, i.e. the single
      lookup `EngineRegistry::new()` always pays at construction; the
      absolute form is stable under both nextest (fresh process) and plain
      `cargo test` (pre-warmed cache), where a delta form would read 0.
      Native-routing proof: preview context registers raw notebook bytes at
      `ORIGINAL_FILE_ID` (wire path can only produce
      `Generated(By::unknown())`). GREEN on first run (regression guard —
      wiring already landed); clippy clean.)*
- [x] End-to-end per CLAUDE.md: `cargo run --bin q2 -- render fixture.ipynb`,
      inspect the output, inspect a deliberately-broken fixture's terminal
      diagnostic; record invocation + snippets here. *(Done 2026-09-25,
      jupyter-less dev machine. Three findings, in the order discovered:

      **(1) `q2 render` of any `.ipynb` demands jupyter (P2-12), by design.**
      `cargo run --bin q2 -- render e2e-good.ipynb` fails with
      `Engine 'jupyter' is registered but its runtime is not available.` —
      and so does a markdown-only notebook. Mechanism: the `.ipynb` file
      claim short-circuits engine resolution to jupyter
      (`engine_execution.rs:266`, `claimed_engine_name`), `q2 render`
      hardcodes `ExecutionPolicy::All` (main.rs:1359, "always executes"),
      so `get_engine_with_fallback` fails loudly when jupyter is absent
      (`engine_execution.rs:167`, P2-12). **Parity check**: a plain `.qmd`
      with a ```{python}` cell fails with the byte-identical error — the
      ipynb path is consistent with existing engine semantics, not a 7c
      defect. Q1 divergence to note: Q1 renders stored-output notebooks
      without jupyter; q2 will too, via Phase 3's replay engine. Sub-nuance
      for Phase 3: even a zero-code-cell notebook demands jupyter under
      this model, because the claim (not the cells) drives resolution.

      **(2) Broken notebook's terminal diagnostic is correct through the
      real CLI.** Fixture: cell 1 markdown `Some intro`, cell 2 markdown
      `![logo](images/logo.svg){width="65px" .light-content}` (kv-before-
      class, same construct as the flagship fixture). Invocation:
      `cargo run --bin q2 -- render e2e-broken.ipynb`. Observed (exit
      nonzero, ANSI colors in real terminal):

      ```
      warning: profile-pass skipped …/e2e-broken.ipynb: Error: [Q-2-3] Key-value Pair Before Class Specifier in Attribute
         ╭─[ e2e-broken.ipynb[cell 2, markdown]:1:39 ]
         │
       1 │ ![logo](images/logo.svg){width="65px" .light-content}
         │                         ──────┬──────  ────────┬────────
         │                               ╰── This key-value pair cannot appear before the class specifier.
         │                                              │
         │                                              ╰── This class specifier appears after the key-value pair.
      ```

      The owning-cell label `[cell 2, markdown]:1:39` and in-cell snippet
      are exactly the Phase-1 design, now through the user-facing binary.
      (Surfaced from the profile pass — `DocumentProfileStage` parses in
      Pass-1 — not the render pipeline's `ParseDocumentStage`; same
      mapping machinery either way.)

      **(3) Positive render artifact via `q2 preview --static` +
      `preview: engine: off`.** That project config maps to
      `ExecutionPolicy::None` (`quarto-preview/src/config.rs:46`,
      `preview_static.rs:148`) — the one jupyter-free route through a real
      binary. Project: `_quarto.yml` with `preview: {engine: off}` +
      `project: {render: ["**/*.ipynb"]}`, notebook with 2 markdown cells
      + 1 code cell (`#| echo: false` + `print('hello…')`, with a stored
      stream output). Invocation:
      `cargo run --bin q2 -- preview <proj> --static --no-watch --no-browser`.
      `notebook.html` written next to the source; verified:
      `<h1 class="title">End-to-end notebook</h1>` (first markdown heading
      → title), `<h2>A computed section</h2>`, prose paragraphs; the code
      cell passes through inert:
      `<pre class="{python} code-with-copy"><code>#| echo: false
      print('hello…')</code></pre>`. Correctly absent: the stored stream
      output (`hello from the notebook` appears only inside the `print`
      source text) — Phase 3's replay engine hasn't landed. Cosmetic
      observation, classified pre-existing + out of scope: the inert
      pass-through emits the raw fence tag into the class
      (`class="{python}"`), which is the shared writer behavior for
      brace-fenced blocks (a .qmd ```{sql} block routes to jupyter too, so
      the same artifact exists there whenever a brace fence passes through
      unexecuted); execution/Phase-3 cell handling makes it moot.

      Scratch fixtures under `target/tmp-e2e-ipynb/` (gitignored).)*

      **Phase 2 boundary (2026-09-25, logged): `cargo nextest run
      --workspace` → 14839 passed / 201 skipped / 86 binaries, all green
      (462.6s; log `/tmp/nextest-7c-phase2-boundary.log`).** Delta vs the
      handoff's quoted live baseline (14824/200): **+15 passed, +1
      skipped.** Attribution from full-phase context: only two commits
      postdate the handoff point (eb5126ab2), each adding exactly one
      runnable test (composition `cell_option_yaml_error_lands_in_owning_cell`,
      discovery `project_of_notebooks_admitted_and_converted_launch_free`;
      verified by `git diff eb5126ab2..HEAD`) — **+2**. The residual
      **+13 passed / +1 skipped cannot arise from branch history** (linear,
      nothing else landed), so the handoff's 14824/200 was not measured at
      eb5126ab2's exact tree state — most plausibly a mid-session figure
      captured before the session's last test-adding commits (the scaling
      gate, committed 801670fd3 with `#[ignore]`, is the natural +1-skip
      candidate). Bounding cross-check against the nearest *logged* anchor,
      `/tmp/nextest_workspace_p8.log` (14793/200, Sep 24 12:19, this
      worktree): all growth 14793→14839 is in quarto-core unit (+37) and
      quarto-core::integration (+9), exactly where 7b/7c landed; the other
      84 binaries are flat. Nothing red, nothing missing; the only
      unreconstructable datum is the handoff's intermediate itself (its log
      was not kept). **New live baseline: 14839 passed / 201 skipped.**

### Phase 3 — code cells with stored outputs
- [x] Stored-output replay engine (option B), reusing `format_outputs`.
      (commit 630193d66: `IpynbReplayEngine` in `engine/jupyter/stored.rs`,
      `EngineExecutionStage` routes `.ipynb` without an execute demand to
      it before the policy gate — see "Phase 3 wiring decision" above.)
- [x] `--execute` route-through test (existing jupyter engine).
      (same commit: `execute_enabled_routes_to_jupyter_not_replay` +
      `no_execute_demand_replays_instead_of_running_jupyter` in
      `tests/integration/ipynb_stored_replay.rs` — a stub engine named
      "jupyter" re-declaring `static_file_claims()` makes the route
      observable machine-independently.)
- [x] Real-world notebook render through the real binary (the "Q1
      comparison" item, executed as the CLAUDE.md-mandated e2e; see the
      record below). Jupyter is not installed on this machine, which is
      itself the point: the replay render is the proof that stored
      outputs render with zero kernel. The output shapes are the ones
      pinned from Q1's `format_outputs` (unit tests in `stored.rs`), so
      the comparison is by construction.

**Phase 3 e2e record (2026-09-25).** Real-world fixture:
`quarto-cli-6147/penguins.ipynb` (8 python code cells, 1 markdown, 1 raw
front-matter cell `title: Palmer Penguins`; 3 display_data images +
2 execute_result HTML tables; every code-cell source lacks the trailing
newline). Invocation (from this worktree root):

```
cargo build -p quarto --bin q2
cd target-e2e && ../target/debug/q2 render penguins.ipynb
```

Observed in `target-e2e/penguins.html` (inspected, not inferred):

- 8 `<div class="cell">` with 8 `sourceCode cell-code code-with-copy`
  echoed, syntax-highlighted sources; `#|` options correctly stripped.
- 5 output divs in the pinned shapes: 2 `cell-output cell-output-display`
  (pandas `execute_result` tables with scoped CSS, raw HTML preserved),
  3 bare `cell-output-display` (seaborn/matplotlib PNGs — no generic
  `.cell-output` class, exactly the `format_outputs` image shape).
- 3 PNGs written: `penguins_files/figure-html/cell-5-output-1.png`,
  `cell-6-output-1.png`, `cell-6-output-2.png`.
- `#| fig-cap` cells become numbered quarto floats: `<div
  id="fig-bill-marginal" class="quarto-float …">` with
  `figcaption.quarto-float-caption-bottom` — crossref numbering works
  on replayed cells.

**Converter bug found by this e2e (fixed same day).** The first render of
penguins.ipynb exited 0 but produced a single inert `<pre class="{python}
code-with-copy">` blob: no `.cell` divs, no outputs, no figures, `#|`
options verbatim, closing fences glued onto the last code line
(`plt.show()```). Root cause: nbformat strips the source array's final
newline, so every real-world code cell ends without `\n`; the Phase 2
converter's `cell_wrap`/`format_output` emitted the closing fence's
Generated piece as `{ticks}\n` unconditionally, gluing
`plt.show()` + ``` onto one line — unparseable markdown, and
unmatchable by the replay engine's fence alignment. All prior converter
fixtures used `\n`-terminated sources, so no test caught it; only the
mandated real-binary e2e did (CLAUDE.md's "tests verify the contract the
test author had in mind" incident list gets a new entry). Fix (TDD, 2 RED
unit tests in `ipynb.rs` + integration regression
`code_cell_without_trailing_newline_still_replays`): the closing piece
prepends `\n` when the cell text doesn't end with one — the newline lives
in the Generated fence-close piece, so Original ranges stay contiguous.

Phase boundary: `cargo nextest run --workspace` run at the Phase 3
commit; delta accounted below (recorded after the run).

### Phase 4 — presentation hardening (upstream)
- [ ] `FileOrigin` structured metadata in `quarto-source-map` +
      `quarto-error-reporting` rendering (replaces the P1 pseudo-path).
- [ ] `--json-errors` structured cell locations.
- [ ] Hyperlink behaviour for virtual files.

### Phase 5 — coordination
- [ ] Point bd-19nc56ao and bd-xxul at this plan; record the
      supersession of the July-20 doc's attachment point. (k-zr88 was closed
      superseded 2026-09-24 — open question 5 — so nothing to point there.)
- [ ] User docs (usage, not internals): rendering `.ipynb` inputs.

---

## Open questions

1. ~~**Per-cell file scaling.**~~ **Resolved 2026-09-25 by measurement**
   (Phase 2 checklist item "Per-cell file scaling gate"): registration is
   linear and trivial (~0.5 ms per 1000 cells); per-diagnostic mapping is
   O(N) per call with a ~1.4 µs constant at 1000 cells. The technique
   scales for realistic diagnostic counts; no design change needed.
2. ~~**Cell numbering**~~ — **Resolved 2026-09-24**: 1-based over all cells,
   qualified with cell type; `cell.id` in JSON output only (§ Execution
   decisions item 5).
3. ~~**Pseudo-path MVP**~~ — **Resolved by the plan body**: ship P1
   (pseudo-path labels), land P2 (structured `FileOrigin`) before calling the
   feature done; no separate decision needed.
4. ~~**Concatenated-document semantics**~~ — **Confirmed by Gordon
   2026-09-24**: concatenate (Q1 parity); cross-cell fence/div structure must
   keep working (§ Execution decisions item 2). The (now-dropped) per-cell
   well-formedness gate went with it — see § Implementation seams item 3.
5. ~~**k-zr88's remaining scope.**~~ **Resolved 2026-09-24 — strand closed
   superseded.** Review-2's research confirmed Gordon's 2026-08-17 scope check
   on the
   strand: both halves superseded (ipynb → this plan; percent/spin → 7b),
   and the sidecar envelope *format* never found a consumer — 7b deferred
   persistence with none named, this plan declines the envelope (in-memory
   pipeline), and the run-table escape hatch (§ What this gives up) covers
   any future raw-offset need.
6. ~~**Flagship rendering half — upstream timing (review-2, Finding A).**~~
   **Resolved 2026-09-24 (Gordon): option (i)** — the fix lands as a PR to
   `posit-dev/quarto-error-reporting` (full access; PR first, release when
   needed), released and version-bumped **inside Phase 1**, so the flagship
   goes fully green in-phase. Concretized as the "Phase 1 (upstream)"
   checklist items; the bug's full shape (both renderer copies, the
   silent detail skip, coalesce benign) is recorded in § Implementation
   seams item 3.
7. ~~**`Converted.files` transport (review-2, Finding B).**~~ **Resolved
   2026-09-24 (Gordon): option (a)** — the stage routes processor-bearing
   claims through `content_processors::convert` directly and stamps an
   additive `LoadedSource.files`; the engine trait and wire path are
   untouched. Concretized as the Phase 2 checklist item of the same name.
   `convert`'s signature stayed fixed (settled), as required.

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
- ~~Conversion provenance prerequisite: bd-zlemoc6w~~ — closed obsolete
  2026-09-24 (§ Review corrections item 3); the native converter never touches
  the wire path it tracked
- Q1 source: `external-sources/quarto-cli/src/core/jupyter/` (`jupyterToMarkdown`,
  `mdFromCodeCell`, `mdFromRawCell`, `fixupFrontMatter`); port reference
  `ts-packages/quarto-api/src/jupyter/` (carries **no** fixups — port
  `fixupFrontMatter` from Q1 directly, § Execution decisions item 4)

## Migration note: `.ipynb` is not auto-discovered (2026-08-18)

Quarto 2 auto-discovers `**/*.qmd` and nothing else. **A project consisting
entirely of notebooks renders nothing** until the author writes:

```yaml
project:
  render:
    - "**/*.qmd"      # a positive pattern replaces the default — keep this
    - "**/*.ipynb"
```

This was decided explicitly rather than inherited. `.ipynb` was considered for
the same treatment as `.qmd` — auto-discovered — on the grounds that a notebook
in a Quarto project is almost always meant as a document, and that all-notebook
projects are a real and common Q1 workflow. **Rejected** (Gordon, 2026-08-18):
`.ipynb` is in exactly the same position as percent and spin scripts, and one
consistent rule beats a per-type table. Power users who need it should be guided
to `**/*.ipynb`.

Do not re-open this as part of 7c implementation. The decision is the discovery
policy, not an artifact of ipynb support being incomplete.

**The failure mode is the worst kind: silent.** A Q1 user pointing Quarto 2 at a
directory of 40 notebooks gets "Rendered 0 of 0 files" and no diagnostic — we
deliberately do not warn (matching an extension does not prove a processor would
take the file; see 7b's note). That makes the docs the entire mitigation, so
7c's user-facing documentation must state the `render:` requirement prominently
rather than in passing.

`docs/guides/projects/render-list.qmd` carries the general rule and the
"keep `**/*.qmd`" trap. This plan owes the notebook-specific guidance.

Supersedes D1 of `2026-08-13-ts-engine-extensions-merge-main.md`.
