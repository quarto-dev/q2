# Plan 7b — Native content-processor registry: percent + spin (zero Pass-1 launch)

**Series root:** [2026-06-27-plan7-native-percent-spin-sourceinfo.md](2026-06-27-plan7-native-percent-spin-sourceinfo.md) (reframed as the 7-series *content-processor architecture* root)
**Supersedes:** [2026-07-07-plan7a-static-content-pattern-claims.md](2026-07-07-plan7a-static-content-pattern-claims.md) (7a's arbitrary-regex claim mechanism is withdrawn; its surviving design points — discovery admission, one-predicate-two-sites coherence, built-ins-as-data, the Q6 membership-cache contract — migrate here)
**Consolidates:** Plan 7's percent/spin conversion + precise SourceInfo (Phases 7A–7E) into one native, engine-agnostic path
**Coordinates with (concurrent sibling on `feature/ts-engine-extensions`):** [2026-06-29-plan6-pass1-engine-resolution.md](2026-06-29-plan6-pass1-engine-resolution.md); [2026-07-01-plan4b-shadow-engine-features.md](2026-07-01-plan4b-shadow-engine-features.md); [2026-07-01-plan1c2-engine-extensions-loose-ends.md](2026-07-01-plan1c2-engine-extensions-loose-ends.md) (P4 `claims-files` schema)
**Later in series:** [2026-07-08-plan7c-ipynb-content-processor.md](2026-07-08-plan7c-ipynb-content-processor.md) (ipynb; additive over 7b's seams)
**Design docs to amend:** `engine-resolution.md §3.3`; `engine-api-surface.md` (static-claim expressiveness)
**Branch:** `plan7b-percent-spin-registry` off `feature/ts-engine-extensions`
**Date:** 2026-07-08
**Status:** PLAN — design ratified with Gordon 2026-07-08 (session "spin-parse-rust"). Work items unstarted. Additive, post-1c; **not on the critical path**.

---

## Context — why this plan exists

**The bug in the 7a + Plan 7 combination.** Plan 7a made the *claim decision* for percent/spin
scripts static and load-free (a regex over file bytes, evaluated natively at Pass-1 discovery —
correct). But 7a explicitly left the *conversion* to Plan 7, and Plan 7 converts TS-engine percent
scripts **over the wire, Deno-side** (Phase 7D, `quarto.jupyter.percentScriptToMarkdown`). Because
Pass-1's `EngineClaimsFileStage` runs `markdown_for_file` on every claimed non-`.qmd` file, admitting
a percent `.jl`/`.py`/`.R` into a project means Pass-1 must convert it — and for a TS engine that
launches Deno **in the indexing pass**. A project with N julia/marimo percent scripts launches the
engine N times before any render. That directly violates the grand plan's own principle
(`2026-04-16-ts-engine-extensions-subprocess.md` L82): *"engine `claims_*` must not load expensive TS
engines merely to index a doc in Pass 1."* Native knitr spin has the same shape — `knitr::spin`
shells out to `Rscript`, so a built-in spin conversion in Pass-1 spawns R.

**Evidence (verified this session, `feature/ts-engine-extensions` tip):**
- `stage/stages/engine_claims_file.rs:142` — on `claims_file == true`, the stage immediately calls
  `engine.markdown_for_file(...)`; the stage is pass-agnostic and runs **first** in the Pass-1 list
  (`project/orchestrator.rs:1708`).
- `engine/ts_engine.rs`: `markdown_for_file` → `ensure_launched` → `ensure_loaded` →
  `host.ensure_started()` → `deno run --allow-all <bundle>` (`engine/ts_process.rs:519`). A
  content-inspecting TS engine launches Deno even at the `claims_file` probe.
- **There is no native Rust percent/spin conversion at all today** (grep: only URL percent-encoding).
  Built-in `JupyterEngine`/`KnitrEngine` override neither method — `claims_file` defaults `false`,
  `markdown_for_file` defaults `not_supported` (`engine/traits.rs:157,170`). Percent/spin support is
  therefore **greenfield** — there is no existing native path to refactor.

**The fix (this plan).** One **native Rust content-processor path** for percent and spin that *both*
built-in engines (jupyter/knitr) *and* TS extension engines (julia/marimo) reference **by name**.
Native sniff + native convert + A+ `SourceInfo`, needing no engine object at all → **zero subprocess
in Pass-1**, for every engine. This is strictly less surface than 7a (no per-engine regex authoring,
no regex-flavour/ReDoS questions — the Q1 census proved every real content claim is percent-or-spin),
and it eliminates Plan 7's "two conversion paths" (native for built-ins, wire for TS) in favour of
one.

**Research backing (this session):**
- Percent: **one parser, parameterized by `(comment_open, fence_language)`.** The standalone
  `~/src/quarto-julia-engine` and the in-tree julia fixture both *delegate* to the shared
  `quarto.jupyter.isPercentScript` / `percentScriptToMarkdown`; neither ships its own. Julia vs Python
  differ only in the fence label (`#` comment is shared). A Rust port of
  `ts-packages/quarto-api/src/jupyter/percent-script.ts` is a clean, faithful translation. The only
  non-parameterized convention is the triple-quote `"""` raw/markdown block (keep constant).
- Spin: **no portable converter exists.** Q1 does only the `#' ---` *detection* in TS;
  `markdownFromKnitrSpinScript` shells to R (`callR("spin", …)`). Native spin is a green-field
  reimplementation of `knitr::spin`'s grammar (fully mapped below from `~/src/knitr/R/spin.R`), and
  is *required* for A+ SourceInfo (the R path loses all provenance) **and** for a launch-free Pass-1.
- `matchable` (knitr's "is this `#'`/`{{ }}` a real marker vs. text inside a multi-line string?"):
  reuse **`tree-sitter-r`**. q2 already depends on the published crates.io `tree-sitter-r = "1.2"` via
  `quarto-highlight` (`crates/quarto-highlight/Cargo.toml:27`, `src/langs/r.rs`), and its built-in
  grammars are **statically linked native and wasm32 alike** (`quarto-highlight/src/registry.rs:125`;
  `quarto-highlight` is a dep of `wasm-quarto-hub-client`). So R parsing is already in both builds at
  **zero incremental WASM cost**; we do *not* need air/biome or a git fork. Drive `tree_sitter::Parser`
  + `tree_sitter_r::LANGUAGE` directly (string/comment spans, top-level token starts,
  `root_node().has_error()` for knitr's "won't parse ⇒ all lines matchable" fallback). `~/src/air`
  is a useful *reference* for how to drive tree-sitter-r + the parse-error fallback, but is not a
  dependency.

---

## Ratified decisions (Gordon, 2026-07-08)

1. **Declaration carries the processor and its params — no q2-global table.** Each engine states, per
   file-claim, the processor name and (for percent) its params. The only thing resembling a "table"
   is jupyter's *own* multi-extension declaration, which is jupyter's knowledge, expressed as several
   claim entries — not a mapping q2 applies behind the engine's back.
2. **`comment` defaults to `"#"`, overridable; `language` is required on a percent claim.** The
   default is a single constant, not a mapping (keeps it table-free while sparing every `#`-language
   engine the repetition; only `q` overrides to `/`). Spin takes **no** params.
3. **Pass-1 discovery is 100% launch-free.** A file claimed only via a *dynamic* `claims_file` (no
   processor — 4b's `content-claim` fixture) is **excluded from project discovery**; it stays
   renderable as an explicit single-file argument (hitting the wire in Pass-2). No project input ever
   launches an engine in Pass-1.
4. **`tree-sitter-r` reused, cross-target, not native-gated.** The spin processor stays cross-target
   (native + wasm32): the grammar is already in both builds, and we *want* spin available in WASM
   because eventually there will be WASM engines.
5. **SourceInfo: deliver the format, defer persistence.** In-memory `SourceInfo` (`Concat`/`Original`)
   on the live conversion path; the sidecar envelope is *defined* as a versioned tagged union
   (`plain_text | jupyter_notebook`) but persistence/staleness is **out of scope** (no cache exists
   today that would consume it — confirmed).
6. **Spin fidelity via committed goldens.** Generate the golden corpus with real `knitr::spin` once
   in dev, **commit the `.qmd` outputs**, and assert the Rust converter matches them in CI — so CI
   needs no R. An `xtask`/script regenerates them on a knitr version bump (spin's format is still
   evolving — pin the knitr version and track `NEWS.md`).
7. **7-series shape.** Plan 7 = architecture root; 7a = tombstone (survivors migrated here); 7b (this
   plan) = percent + spin; 7c = ipynb (later, additive). See the root doc.

---

## Architecture — the content-processor registry

A **content processor** owns *sniff + convert + A+ SourceInfo* for one non-qmd input format. It is
**not an engine**; an engine merely *names* one. This separation is what lets a single `percent`
processor serve jupyter, julia, and marimo.

### Interface (forward-designed for ipynb — see § Forward-compatibility obligations)

```rust
// crates/quarto-core/src/engine/content_processors/mod.rs
pub trait ContentProcessor {
    /// Fast, native, no-launch content sniff over already-read bytes.
    fn sniff(&self, path: &Path, content: &str, params: &ProcessorParams) -> bool;

    /// Convert to qmd, producing precise SourceInfo back into the original bytes.
    /// Takes a context so a future ipynb processor can write figure assets;
    /// percent/spin ignore the asset side.
    fn convert(
        &self,
        path: &Path,
        content: &str,
        params: &ProcessorParams,
        ctx: &ProcessorContext,   // runtime/output handle; unused by percent/spin
    ) -> Result<Converted, ProcessorError>;
}

pub struct Converted {
    pub markdown: String,
    pub source_info: SourceInfo,        // Concat/Original
    /// Ephemeral source files the pieces above point at, for the caller to
    /// register in `SourceContext`. Empty for percent/spin (they map back into
    /// the already-registered original file); 7c's ipynb processor returns one
    /// entry per cell. See § Forward-compatibility obligations.
    pub files: Vec<(String, String)>,   // (label, logical content)
}
```

- **Registry**: `HashMap<ProcessorName, Box<dyn ContentProcessor>>`, built once, engine-agnostic.
- **`ProcessorParams`**: percent → `{ comment_open: String, fence_language: String }`; spin → `{}`.
- **`ProcessorContext`**: a thin handle (runtime dir / output sink). Percent/spin need only bytes;
  the param exists so 7c's ipynb converter (which writes figures) needs no trait change.

### Declaration (the schema `processor:` field)

`claims-files` entries gain an optional `processor:` — a serde **untagged** union of a bare name or a
map with params:

```yaml
# TS engine _extension.yml — julia:
claims-files:
  - extension: .jl
    processor: { name: percent, language: julia }   # comment defaults to "#"

# a percent language whose comment differs:
  - extension: .q
    processor: { name: percent, language: q, comment: "/" }

# spin needs no params — bare name:
  - extension: .R
    processor: spin
```

Built-in engines have no `_extension.yml`; they declare the **same** `FileClaim` shape as Rust static
data (jupyter = four percent entries `.py/.jl/.r/.q`; knitr = spin on `.r/.R`). Discovery and the
claim stage never branch on "built-in vs extension" — both read `FileClaim { extension, processor }`.

### The load-bearing invariants

- **Data, not launch.** Discovery reads processor *names + params* (static data) and runs the
  processor's `sniff` natively. It **never** constructs an engine or spawns a subprocess.
- **One conversion path.** `markdown_for_file`'s default trait impl dispatches to the named processor
  natively. The wire `markdownForFile` / `ClaimsFile` verbs survive **only** as the residual dynamic
  fallback (no processor declared). TS `markdown_for_file` is native-first, wire-only-if-no-processor.
- **One predicate, two sites** (migrated from 7a Stage 4). The *same* `sniff` decides Pass-1 discovery
  admission (`project/discovery.rs`) and the claim stage (`EngineClaimsFileStage`). A coherence test
  guards "discovery admits ⟺ claim stage claims."
- **Processor output is engine-independent.** Same `(extension, params)` ⇒ identical qmd, regardless
  of which engine named the processor (only *execution* differs downstream).

---

## The two processors

### `percent` (port of `percent-script.ts`)

Faithful Rust translation of `ts-packages/quarto-api/src/jupyter/percent-script.ts` (itself a rewrite
of Q1 `src/core/jupyter/percent.ts`). Parameterized by `(comment_open, fence_language)`; everything
else is a shared constant.

- **Sniff:** `^\s*{comment}\s*%%+\s+\[(markdown|raw)\]` (multiline). Requires a markdown/raw cell — a
  code-only percent script is **not** admitted (Q1-faithful; call it out as a conscious choice).
- **Convert:** classify cells via the header `^\s*{comment}\s*%%+\s*(?:\[(markdown|raw)\])?\s*(.*)$`;
  markdown/raw cells are either a triple-quote `"""…"""` block (verbatim interior) or comment-prefix
  stripped (`^{comment}\s?`); code cells emit `#| ` option lines then a ` ```{{fence}} `…` ``` ` fence.
- **SourceInfo:** a `Concat` of **per-line `Original` pieces** with a constant per-line column shift
  (the stripped prefix); inserted fence lines are synthetic (no `Original`). Content is verbatim
  post-prefix, so column mapping is exact through conversion (design §"Column Precision with Concat").

**Boundaries known now:** block-comment languages (`[open, close]`) don't fit the single-marker
prefix strip — not needed (`.py/.jl/.r/.q` are all single-line). The `"""` delimiter is a fixed
Python-flavoured convention, kept constant. Percent does not special-case YAML front matter.

### `spin` (native reimplementation of `knitr::spin`)

Green-field port of `~/src/knitr/R/spin.R` (read in full this session), targeting the `qmd`/`Rmd`
output branch. **`matchable` computed via `tree-sitter-r`.**

**Grammar (from `spin.R`), for the qmd/Rmd branch:**
- **Doc (prose) marker** `doc = "^#+'[ ]?"` — one-or-more `#`, a `'`, optional single space; strip
  and emit verbatim.
- **Chunk delimiter** `rc = "^(#|--)+(\+| %%| ----+| @knitr)(.*?)\s*-*\s*$"` — group 3 (options text,
  trailing whitespace+dashes trimmed) becomes the chunk-header body **verbatim**. Aliases `#+`,
  `# %%`, `# ----` (≥4 dashes), `## @knitr`, `--`-prefix. `#-` is intentionally unsupported.
- **Pipe options** `#| ` (hash-pipe-**space**) — starts a chunk (bare ` ```{r} ` fence prepended);
  the `#|` lines are preserved verbatim inside the chunk (Quarto cell options).
- **YAML header** — the roxygen `#' ---` … `#' ---` block is just doc-marker prose (strip `#'` ⇒ a
  normal `--- … ---` front matter). Detection of a *spinnable* file keys on this header
  (`/^\s*#'\s*---[\s\S]+?\s*#'\s*---/`).
- **Bare code → chunk** — a code block whose first line is not already an opener gets a default
  ` ```{r} ` prepended; blank-trimmed; empty blocks dropped.
- **Backtick count** — the fence length is `max(longest-backtick-run-in-file + 1, 3)`, computed over
  the whole file; inline uses `longest-run + 1`.
- **Blank-line wrapping** — every code chunk is wrapped `["", …, close-fence, ""]`; doc blocks are
  not. Load-bearing for byte fidelity.
- **Block comments** — paired `/* … */` lines removed before classification (mismatched counts = hard
  error).

**`matchable` via tree-sitter-r:** parse the script; a `#'`/`{{ }}` line is a *real* marker only if it
starts a top-level token at column 1 — i.e. it is **not** inside a `string`/`string_content` span
(walk for those node kinds' byte ranges) and begins a top-level `program` child. On
`root_node().has_error()`, fall back to **all-matchable** (knitr's exact behaviour, `spin.R:73`).

**SourceInfo:** `Concat` of prefix-stripped `Original` pieces (doc/`#'` lines, chunk bodies) +
synthetic inserted pieces (fences). More involved than percent (inserted fence lines, dropped blanks)
but the same `Concat`/`Original` model.

**Scope for this plan:** the **qmd/Rmd markdown branch only.** The `Rnw`/`Rhtml`/`Rtex`/`Rrst` output
formats and inline `{{ expr }}` expansion are out of scope (q2 emits qmd). `report`/`precious`/`knit`
knobs are knitr-compile concerns, not conversion — out of scope.

---

## SourceInfo (A+) and the sidecar envelope

- **In scope:** in-memory `SourceInfo` (Plan 0 `Concat`/`Original` infra) on the live conversion path,
  so an error in a percent/spin markdown comment *or* code cell reports the **original file, line, and
  column**. This is the A+ provenance Plan 7 promised, now uniform (no per-engine wire remap; Plan 7's
  Phase 7D "A′ over the wire" is deleted).
- **Defined but deferred:** the sidecar envelope format — a **versioned tagged union**
  `{ version, kind: plain_text | jupyter_notebook, … }` under `.quarto/source-maps/`. 7b lands the
  `plain_text` shape and the envelope type; **persistence, staleness, and cleanup are out of scope**
  (no consumer today; the freeze/incremental layer that would read it does not exist — see the Q6
  contract). 7c adds the `jupyter_notebook` arm.

  > **REOPENED 2026-08-17 — does 7b need the envelope at all?** The sidecar's premise is that "the
  > converted qmd is plain text on disk with nowhere to store mapping inline." That premise is false:
  > `run_pipeline` is fully in-memory, conversion happens in front of the parser, and no qmd
  > intermediate is written. 7c (rewritten) consequently **does not want a `jupyter_notebook` arm**,
  > which was the envelope's only forward consumer — and its only present-day consumer is likewise
  > absent, as this bullet already concedes. **Decide before Phase 3: land the envelope type only if a
  > consumer is named, otherwise drop it and keep the mapping in memory.** Dropping it does not touch
  > the in-scope bullet above (in-memory `SourceInfo` is the actual A+ deliverable).

---

## Forward-compatibility obligations (so 7c/ipynb is purely additive)

The implementer MUST keep these general even though 7b only fills the percent/spin arms:

- [ ] `ContentProcessor::convert` takes `&ProcessorContext` (asset-writing capable) from the start.
- [ ] `SourceInfo` stays the general Plan-0 enum; **no** percent-specific mapping type leaks into the
      registry. (Revised 2026-08-17: 7c no longer needs a `NotebookCell` variant — cell identity is
      per-*file*, not per-*span* — so this obligation is now just "don't specialize the enum," which
      is cheaper than it was.)
- [ ] **`Converted` carries a channel for ephemeral source files** — `files: Vec<(String, String)>`
      (label, logical content), or an equivalent registration handle on `ProcessorContext`.
      **Added 2026-08-17.** Percent and spin never surface this: they map back into the *original*
      file, which the caller already registered. ipynb's `Concat` pieces point at **virtual per-cell
      files that exist only in memory**, so the processor must hand them back for registration in
      `SourceContext`. Without this field, 7c must change `convert`'s return type — exactly the
      non-additive change these obligations exist to prevent. Cheap now, expensive later.
- [ ] ~~The sidecar envelope is a **versioned tagged union**, not a bare per-line format.~~
      **Suspended 2026-08-17** — see the REOPENED note above. This obligation existed solely so 7c
      could add a `jupyter_notebook` arm; 7c no longer wants one. Do not treat it as binding until the
      envelope's fate is decided.
- [ ] The registry is **name-keyed**; the `processor:` schema is an open union (bare name | map).

None of these require thinking about ipynb's *conversion semantics* now — they are shape choices only.

---

## Migrated from 7a (kept; the arbitrary-regex mechanism is withdrawn)

- **`processor:` on `claims-files`** replaces 7a's raw `content-pattern` regex (which every engine
  would have had to author). The sniff regex is owned by the processor in Rust, not declared per
  engine.
- **Native Pass-1 discovery admission** (7a Stage 6): a candidate whose extension is claimed with a
  processor is admitted iff the processor's `sniff` matches. Bounded by the extension pre-filter.
- **One-predicate-two-sites coherence** (7a Stage 4): the same `sniff` at discovery and claim time.
- **Built-in claims as construction-free static data** (7a Stage 5): readable at discovery without
  building the registry or launching engines.
- **Q6 membership-cache contract** (7a Open Q6): project membership of a content-claimed file is a
  pure function of `(bytes, processor)`; a plain content edit can flip membership (add/remove a
  `# %% [markdown]` cell) with the filename unchanged. **Written into the DocumentProfile/freeze
  design notes** as a contract for whoever builds freeze/incremental — *not* implemented here (no
  membership cache exists today; q2 re-walks + re-sniffs every render, Q1-faithful). 7b records, per
  content-claimed candidate, its content hash + admission bit so a future incremental layer can detect
  flips without re-reading.

**Two-processor `.r`/`.R` tie-break** (7a Q3): jupyter (percent) and knitr (spin) both register
`.r`/`.R`. Percent's sniff requires a `# %% [markdown|raw]` cell; spin's requires the `#' ---` header
— they rarely collide, and a file matching both is resolved by `contribution_order`, first-matching
processor wins.

---

## Phased checklist (TDD — write the listed tests first, watch them fail, implement, watch pass, then `cargo nextest run --workspace`)

Everything lives in `quarto-core`, which feeds `wasm-quarto-hub-client` — full `cargo xtask verify`
(NOT `--skip-hub-build`) before any push.

### Phase 0 — Research + design contracts
- [ ] Pin the `knitr` version used as the spin oracle; record it + the relevant `NEWS.md` entries
      (`# %%`, `#|`, `#-`-removal churn) in a research note.
- [ ] Spike: drive `tree-sitter-r` (`tree_sitter::Parser` + `tree_sitter_r::LANGUAGE`) to extract
      string/comment spans + top-level token starts + `has_error`; confirm it reproduces knitr's
      `matchable` on a handful of string-embedded-marker cases. (Reference: `~/src/air`
      `crates/air_r_parser/src/parse.rs` for driving + parse-error fallback.)
- [ ] Amend `engine-resolution.md §3.3`: retract "the one genuine must-load case"; document the
      **content-processor** model (named, native, zero-load sniff+convert; the genuinely-dynamic
      `claims_file` residue is the only must-load path and is excluded from Pass-1 discovery).
- [ ] Amend `engine-api-surface.md` to mirror.
- [ ] Reframe the root (Plan 7), tombstone 7a, add the 7c placeholder (this session).

### Phase 1 — Schema: `processor:` on `claims-files`
- [ ] Tests (`extension/read.rs` unit): bare-name `processor: spin`; map `processor: {name: percent,
      language: julia}` (comment defaults `#`); explicit `comment`; **malformed/unknown processor →
      loud parse error** through `quarto-error-reporting` (never a silent drop). Undotted-lowercase
      extension normalization preserved (1c.2 P4).
- [ ] Extend `FileClaim` (`extension/types.rs`) with `processor: Option<ProcessorSpec>` where
      `ProcessorSpec` is the untagged bare-name|map union parsed at read time.
- [ ] Migrate existing fixtures to the structured shape (no behaviour change where no processor).

### Phase 2 — Registry + trait + native `markdown_for_file`
- [ ] Tests: registry resolves `percent`/`spin` by name; `ProcessorContext` threads a runtime handle;
      default `markdown_for_file` dispatches to the named processor with **no engine object**; a claim
      with **no** processor still routes to the dynamic wire fallback.
- [ ] `content_processors/{mod,percent,spin}.rs`: the trait, `ProcessorParams`, `ProcessorContext`,
      `Converted`, the registry.
- [ ] Re-express `ExecutionEngine::markdown_for_file` default to consult the engine's `file_claims()`
      processor for `path` and run it natively. `TsEngine::markdown_for_file` native-first;
      wire-only-when-no-processor (retain the `markdownForFile`/`ClaimsFile` verbs for that residue).

### Phase 3 — `percent` processor + SourceInfo
- [ ] Tests: port `percent-script.ts`'s behaviour — `[markdown]`/`[raw]` cells, `"""` blocks,
      prefix strip, `#|` options, fence. Golden equivalence vs the TS helper / Q1. SourceInfo: an
      error in a markdown comment and in a code cell both report the **original file:line:col**.
- [ ] Implement percent (`(comment_open, fence_language)`), per-line `Concat`/`Original`.

### Phase 4 — `spin` processor + SourceInfo (highest risk)
- [ ] Tests: **committed knitr golden corpus** — for each `.R` fixture (roxygen `#' ---` header, `#'`
      prose, `#+`/`# ----`/`## @knitr`/`# %%` chunk options, `#| ` pipe options, bare-code→chunk,
      backtick-run edge, string-embedded `#'`), assert the Rust output byte-matches the committed
      `.qmd` (generated by real `knitr::spin`; CI needs no R). `matchable`: a `#'` inside a multi-line
      string is **not** a marker; a parse-error file falls back to all-matchable. SourceInfo across
      inserted fences + stripped prefixes.
- [ ] Implement spin (tree-sitter-r `matchable` + the `spin.R` grammar, qmd branch only) + the
      `xtask`/script that regenerates goldens from a pinned knitr.

### Phase 5 — Built-in engines route through the registry
- [ ] Tests: `builtin_file_claims()` returns jupyter's four percent claims + knitr's spin claims
      **without** constructing the registry or launching anything; the default `claims_file`/
      `markdown_for_file` derive from them; a `.py` percent renders via jupyter with a native
      conversion (no Deno, no Rscript).
- [ ] jupyter/knitr populate `file_claims()`; add the construction-free `builtin_file_claims()` for
      discovery.

### Phase 6 — Pass-1 discovery admission + coherence + launch-free guarantee
- [ ] Tests: the walk admits a percent `.py`, a spin `.R`; excludes a plain module, a code-only
      percent script, and a **dynamic-claim** file (decision 3 — not a project input); still excludes
      underscore/dot/output-dir files. Coherence test: discovery-admits ⟺ claim-stage-claims. A
      **launch-free assertion**: rendering a project of N percent/spin scripts issues **zero** engine
      launches in Pass-1 (assert the launch counter / no PID spawned).
- [ ] Extend `project/discovery.rs` with the processor-sniff admission tier (native, whole-file read,
      byte-identical to the claim stage's read). Exclude dynamic-claim files from discovery.
- [ ] Record per content-claimed candidate its content hash + admission bit; write the Q6
      membership-cache contract into the DocumentProfile/freeze design notes.

### Phase 7 — TS-engine native path + Julia `.jl` validation flip (was Plan 7E)
- [ ] Tests: julia/marimo `_extension.yml` declare `processor: percent`; a julia `.jl` percent script
      renders with **native** conversion (no Deno in Pass-1), and its error provenance points at the
      original `.jl` line+col (A+, native — no wire `source_map`).
- [ ] Flip Plan 4's now-removed exclusion ("Julia claims by language only; no `claims_file` for `.jl`"
      → julia claims `.jl` percent via the processor).

### Phase 8 — End-to-end (CLAUDE.md contract: real binary, inspected output, recorded here)
- [ ] `cargo run --bin q2 -- render <fixture project>` with percent `.py`/`.jl` + a spin `.R`: assert
      the docs render, appear in the `ProjectIndex` with converted titles, **and** that Pass-1 spawned
      no `deno`/`Rscript` (launch counter / process check). Paste invocation + output snippets here.
- [ ] Inspect provenance: force an error in a converted cell; confirm the message names the original
      file/line/column.

### Phase 9 — Coordination + docs
- [ ] **Plan 6 (concurrent sibling):** add a coordination note — native conversion makes percent/spin
      Pass-1 profiles **hashable**, so the Pass-1 cache key should fold in the *processor version* +
      converted output; Plan 6 decision-9's "unhashed `.js` conversion" caveat and P1's
      "may have loaded a content-inspecting engine" parenthetical are **removed for percent/spin** when
      both settle. No dependency either way.
- [ ] **Plan 4b:** relabel the `content-claim` fixture as the *dynamic residue*; note it is excluded
      from Pass-1 discovery (decision 3) and is the only surviving must-load path.
- [x] **Grand plan sub-plans table** — done 2026-08-17, ahead of implementation, so the epic's index
      stops contradicting the 7a tombstone: 7 → series root (◍), 7a → tombstoned, 7b/7c rows added,
      totals + status key updated.
- [ ] **1c.2 P4:** record that `processor:` extends the structured `claims-files` it delivered.
- [ ] User docs (`docs/`, usage not internals): percent/spin script inputs; the `processor:`
      declaration for extension authors. Verify with `cargo run --bin q2 -- render docs/` (never Q1).
- [ ] Reconcile this checklist against reality; commit; ask Gordon before any push/merge to
      `feature/ts-engine-extensions` (`--no-ff`).

---

## Test Seam Spec (TDD — write before implementing)

| # | item | tier | seam / revert → RED |
|---|------|------|----------------------|
| T-schema | Phase 1 | unit | `processor:` bare-name + map parse; comment default `#`; malformed/unknown → parse error. Revert parser → RED |
| T-registry | Phase 2 | unit | registry resolves by name; `markdown_for_file` native dispatch needs no engine; no-processor → wire fallback. Revert dispatch → RED |
| T-percent | Phase 3 | unit | percent convert = golden vs `percent-script.ts` (all cell kinds); SourceInfo maps comment + code errors to original col. Revert port → RED |
| T-percent-src | Phase 3 | unit | per-line `Original` column shift exact. Revert Concat build → RED |
| T-spin-golden | Phase 4 | unit | each `.R` fixture → byte-match committed knitr `.qmd`. Revert grammar branch → RED |
| T-spin-matchable | Phase 4 | unit | `#'` inside a multi-line string is NOT a marker; parse-error file ⇒ all-matchable. Revert tree-sitter-r span check → RED |
| T-builtin-data | Phase 5 | unit | `builtin_file_claims()` returns jupyter/knitr claims without registry/launch. Revert accessor → RED |
| T-discovery | Phase 6 | unit | admit percent `.py` + spin `.R`; exclude plain module, code-only percent, **dynamic-claim** file. Revert admission tier → RED |
| T-coherence | Phase 6 | unit | discovery-admit set == claim-stage-claim set. Revert to divergent read → RED |
| T-launch-free | Phase 6 | integration | N percent/spin scripts in a project ⇒ **zero** engine launches in Pass-1 (launch counter). Revert native dispatch (fall to wire) → a launch fires → RED |
| T-e2e-percent | Phase 8 | e2e | percent `.py`/`.jl` renders; ProjectIndex entry; no `deno` in Pass-1. Revert TS native-first → Deno spawns → RED |
| T-e2e-spin | Phase 8 | e2e | spin `.R` renders; no `Rscript` in Pass-1. Revert built-in spin routing → RED |
| T-provenance | Phase 7/8 | e2e | error in a converted `.jl` cell names original file:line:col. Revert SourceInfo wiring → RED |

**Accepted-untested / deferred (logged):**
- Sidecar persistence/staleness/cleanup (no consumer today; envelope format landed, behaviour deferred).
- Full R-tokenizer parity beyond tree-sitter-r's `matchable` (tree-sitter-r *is* the oracle; knitr's
  own fallback is all-matchable).
- spin non-qmd output branches (`Rnw`/`Rhtml`/`Rtex`/`Rrst`), inline `{{ }}` expansion, knitr-compile
  knobs (`report`/`precious`/`knit`).
- ipynb (Plan 7c).
- Dynamic-claim files as project inputs (excluded by decision 3; still explicit-single-file renderable).

---

## Dependencies & sequencing

- **Depends on (landed):** Plan 1c (`claims_file`/`markdown_for_file` trait surface +
  `EngineClaimsFileStage`), Plan 0 (`Concat`/`Original` SourceInfo), Plan 3 (`percent-script.ts` as
  the port reference), 1c.2 P4 (structured `claims-files`).
- **Reuses:** `tree-sitter-r = "1.2"` + `tree-sitter` (already workspace deps via `quarto-highlight`,
  native + wasm32).
- **Orthogonal to** Plan 5 (pooling). **Concurrent sibling** with Plan 6 (both on
  `feature/ts-engine-extensions`) — additive, no ordering dependency; coordination note in Phase 9.
- **Not on the critical path.**

## References
- Grand plan `2026-04-16-ts-engine-extensions-subprocess.md` (Pass-1/Pass-2 model; the no-Pass-1-load
  principle, L82).
- 7-series root `2026-06-27-plan7-native-percent-spin-sourceinfo.md`; tombstone
  `2026-07-07-plan7a-static-content-pattern-claims.md`; ipynb `2026-07-08-plan7c-ipynb-content-processor.md`.
- SourceInfo design `2025-12-15-source-info-for-structured-formats.md` (column technique, `NotebookCell`,
  sidecar envelope).
- Q1 percent: `external-sources/quarto-cli/src/core/jupyter/percent.ts`; port target
  `ts-packages/quarto-api/src/jupyter/percent-script.ts`.
- knitr spin: `~/src/knitr/R/spin.R` (grammar oracle). tree-sitter-r driving reference:
  `~/src/air/crates/air_r_parser/src/parse.rs`, `treesitter.rs`.
- Code path (Pass-1 launch): `stage/stages/engine_claims_file.rs:142`; `engine/ts_engine.rs`
  `markdown_for_file`→`ensure_started`; `engine/ts_process.rs:519`; `project/orchestrator.rs:1708`.

## Migration note: percent/spin scripts are not auto-discovered (2026-08-18)

Quarto 2 auto-discovers `**/*.qmd` and nothing else. A project of percent-format
`.py` or spin-format `.R` scripts renders **nothing** until the author lists them:

```yaml
project:
  render:
    - "**/*.qmd"      # a positive pattern replaces the default — keep this
    - "**/*.py"
```

Quarto 1 differed, and the difference is the reason for the change. Q1 walked the
whole project and asked each engine to claim what it found, which for these types
meant **opening every `.py` and every `.R`** and regex-matching for `# %%` cells
(`core/jupyter/percent.ts:32-45`) or a `#' ---` header (`execute/rmd.ts:570-579`),
at discovery time, on every render. See `claude-notes/research/` — the Q1 discovery
model was confirmed by source audit on 2026-08-18.

Two things follow for this plan:

1. **Moving conversion into Rust does not make these auto-discovered.** The rule is
   about the render list, not about which processor handles a file. A native
   percent processor still only ever sees files a pattern selected. Do not add a
   content-sniffing discovery pass to "restore Q1 parity" — that is the behavior
   being removed on purpose.
2. **This needs user-facing docs, not a diagnostic.** Gordon's call (2026-08-18):
   matching an extension proves files exist, not that any processor would take
   them, so a "you have unlisted `.py` files" warning would fire on every
   `conftest.py` in the world. `docs/guides/projects/render-list.qmd` carries the
   rule; this plan owes the percent/spin-specific migration guidance.

Power users with existing Q1 script-based projects are the affected population.
They are a small group, but the failure mode is silent (zero files rendered, no
message), so the docs have to be findable.

Supersedes D1 of `2026-08-13-ts-engine-extensions-merge-main.md`.
