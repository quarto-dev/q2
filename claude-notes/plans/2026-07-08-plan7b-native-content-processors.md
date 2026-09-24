# Plan 7b — Native content-processor registry: percent + spin (zero Pass-1 launch)

**Series root:** [2026-06-27-plan7-native-percent-spin-sourceinfo.md](2026-06-27-plan7-native-percent-spin-sourceinfo.md) (reframed as the 7-series *content-processor architecture* root)
**Supersedes:** [2026-07-07-plan7a-static-content-pattern-claims.md](2026-07-07-plan7a-static-content-pattern-claims.md) (7a's arbitrary-regex claim mechanism is withdrawn; its surviving design points — discovery admission, one-predicate-two-sites coherence, built-ins-as-data, the Q6 membership-cache contract — migrate here)
**Consolidates:** Plan 7's percent/spin conversion + precise SourceInfo (Phases 7A–7E) into one native, engine-agnostic path
**Coordinates with (concurrent sibling on `feature/ts-engine-extensions`):** [2026-06-29-plan6-pass1-engine-resolution.md](2026-06-29-plan6-pass1-engine-resolution.md); [2026-07-01-plan4b-shadow-engine-features.md](2026-07-01-plan4b-shadow-engine-features.md); [2026-07-01-plan1c2-engine-extensions-loose-ends.md](2026-07-01-plan1c2-engine-extensions-loose-ends.md) (P4 `claims-files` schema)
**Later in series:** [2026-07-08-plan7c-ipynb-content-processor.md](2026-07-08-plan7c-ipynb-content-processor.md) (ipynb; additive over 7b's seams)
**Design docs to amend:** `engine-resolution.md §3.3`; `engine-api-surface.md` (static-claim expressiveness)
**Branch:** `plan7b-percent-spin-registry` off `main` (`feature/ts-engine-extensions`, which this
plan originally targeted, merged to `main` via PR #416 in the interim — all four declared
dependencies below are confirmed landed on `main`, code-verified 2026-09-24).
**Date:** 2026-07-08
**Status:** PLAN — design ratified with Gordon 2026-07-08 (session "spin-parse-rust"). Work items
unstarted. **Reviewed for staleness/consistency 2026-09-24** (see § Review corrections below) —
Phase 6 and the SourceInfo/sidecar section were rewritten; everything else held up. **No longer
"not on the critical path"**: this is now the blocker for bundling the Julia engine into q2 by
default (Q-16-10 fires on every render once julia ships bundled, because its `.jl` claim is a
content sniff that cannot be declared statically without this plan's `processor:` field — see
`.worktrees/workspace-5/CLAUDE.local.md` "Why this work exists now").

---

## Review corrections (2026-09-24, session "plan-7b-review")

Staleness/consistency review, not execution. Findings below; the two design questions were put to
Gordon and resolved.

1. **Phase 6 rewritten (was wrong, not just stale).** The original Phase 6 + Decision 3 built a
   native content-sniffing admission tier in `project/discovery.rs`, mirroring Q1. This plan's own
   later addendum (§ "Migration note," 2026-08-18) says the opposite — and ground-truth-checked
   against `project/discovery.rs`'s current module doc, the addendum is what shipped: the
   2026-08-13 merge's D1 discovery-widening is explicitly marked **superseded** in that file.
   Current model: gate 1 (extension has *any* static `FileClaim`, processor or not → renderable)
   + gate 2 (file matches an explicit user `project.render` pattern — no default widening) is
   **all** discovery does; content sniffing happens exactly once, at claim time
   (`SourceConversionStage`). Phase 6, the "one predicate, two sites" invariant, and the 7a-migrated
   "one-predicate-two-sites coherence" bullet are rewritten below to "one predicate, one site."
   **Gordon confirmed (2026-09-24): drop discovery-time sniffing entirely.**
2. **Sidecar envelope dropped, not deferred.** The 2026-08-17 REOPENED note's own test — "land it
   only if a consumer is named" — resolves to no: 7c's rewrite (same date) explicitly declined the
   `jupyter_notebook` arm ("7b should therefore not land the sidecar envelope on 7c's behalf"), and
   no freeze/incremental layer exists anywhere in `quarto-core` to be the other consumer (checked
   2026-09-24: `can_freeze()` is an unconsumed capability flag; no cache reads/writes it). **Gordon
   confirmed (2026-09-24): drop it.** In-memory `SourceInfo` (`Concat`/`Original`) is the entire A+
   deliverable; no envelope type is added.
3. **Renamed component, same behavior.** `EngineClaimsFileStage` (cited throughout Context/Phase
   2/Phase 6/References below) is `SourceConversionStage` as of the 2026-08-13 merge
   (`crates/quarto-core/src/stage/stages/source_conversion.rs`) — D4: "conversion is the action;
   claiming is only the predicate that selects it." Citations below are left as originally written
   (historically accurate at ratification time) except where a phase's task text needed the current
   name to be actionable.
4. **Tie-break mechanism corrected.** § "Migrated from 7a" originally said the built-in
   jupyter(percent)/knitr(spin) `.r`/`.R` collision resolves via `contribution_order`.
   Code-checked: `contribution_order` governs only extension-engine ordering
   (`engine/resolution.rs:89`); built-ins order via the separate
   `BUILTIN_ORDER = ["knitr", "jupyter", "markdown"]` (`engine/resolution.rs:66`), under which
   **knitr wins** a same-file collision, not "first-matching" in unspecified order. Corrected in
   place below.
5. **Docs overlap.** Phase 9's "user docs" item assumed a blank page. `docs/guides/projects/render-list.qmd`
   already exists and already documents the Q1-vs-Q2 percent/spin auto-discovery divergence almost
   verbatim to this plan's own migration note. Phase 9 should extend that page with the
   `processor:` declaration syntax for extension authors, not write the discovery-rule explanation
   fresh.
6. **`engine-resolution.md` §3.3 amendment is bigger than Phase 0 states.** That section currently
   documents 7a's withdrawn `content-pattern` design as the landing target and asserts a content
   sniff "is statically declarable" for **Pass-1 discovery** — exactly the premise correction #1
   above overturns. Phase 0's task is expanded accordingly.
7. **Stale doc-comment, fix in passing.** `extension/types.rs:141`'s comment ("Plan 7a will grow
   this with an optional `content_pattern` field") predates 7a's tombstoning; Phase 1 touches this
   file anyway (adding `processor:`) — fix the comment there rather than as a separate task.

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
3. **Pass-1 discovery is 100% launch-free — REVISED 2026-09-24, see § Review corrections #1.**
   Superseded as originally written (it described discovery running a native content sniff to admit
   percent/spin files, matching Q1). Current, correct form: discovery does **no** content
   inspection at all, native or otherwise. A file with a static `FileClaim` (processor or not)
   passes gate 1 (renderable); it becomes a project input only when gate 2 (an explicit
   `project.render` pattern) also matches it — there is no default-pattern widening for it. A file
   claimed only via a *dynamic* `claims_file` (no processor — 4b's `content-claim` fixture) fails
   gate 1 and is excluded from discovery entirely; it stays renderable as an explicit single-file
   argument (hitting the wire in Pass-2). The content sniff (percent/spin's `sniff`) runs exactly
   once, at claim time (`SourceConversionStage`), on whatever gate 1 + gate 2 already selected — not
   during the walk. No project input ever launches an engine in Pass-1 (that guarantee is unchanged;
   only *how* discovery decides membership changes).
4. **`tree-sitter-r` reused, cross-target, not native-gated.** The spin processor stays cross-target
   (native + wasm32): the grammar is already in both builds, and we *want* spin available in WASM
   because eventually there will be WASM engines.
5. **SourceInfo: deliver the format; the sidecar envelope is dropped — REVISED 2026-09-24, see
   § Review corrections #2.** In-memory `SourceInfo` (`Concat`/`Original`) on the live conversion
   path is the entire deliverable. No sidecar envelope type is defined (no cache or consumer exists
   today, and 7c — the only proposed forward consumer — has since declined it).
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
claim stage never branch on "built-in vs extension" — both read `FileClaim { extension, processor }`,
though **discovery only ever looks at `extension`** (gate 1 — is this extension statically claimed
at all); `processor` is read exclusively at claim time (see below, REVISED 2026-09-24).

### The load-bearing invariants

- **Data, not launch.** The claim stage (`SourceConversionStage`) reads a file's processor *name +
  params* (static data) and runs the processor's `sniff` natively — **REVISED 2026-09-24, see
  § Review corrections #1**: this is read once, at claim time, not during the discovery walk.
  Neither site ever constructs an engine or spawns a subprocess.
- **One conversion path.** `markdown_for_file`'s default trait impl dispatches to the named processor
  natively. The wire `markdownForFile` / `ClaimsFile` verbs survive **only** as the residual dynamic
  fallback (no processor declared). TS `markdown_for_file` is native-first, wire-only-if-no-processor.
- **One predicate, one site — REVISED 2026-09-24** (was "one predicate, two sites," migrated from 7a
  Stage 4; see § Review corrections #1). `sniff` runs **only** at the claim stage
  (`SourceConversionStage`), on files gate 1 + gate 2 of `project/discovery.rs` already selected.
  Discovery itself never calls `sniff` — there is no second site for a coherence test to guard.
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
  Phase 7D "A′ over the wire" is deleted). This is the **entire** SourceInfo/provenance deliverable —
  see below.
- **Sidecar envelope: DROPPED, 2026-09-24 (Gordon) — see § Review corrections #2.** No versioned
  tagged union, no `.quarto/source-maps/` type, nothing under this heading is built. The premise
  ("the converted qmd is plain text on disk with nowhere to store mapping inline") is false —
  `run_pipeline` is fully in-memory, conversion happens in front of the parser, no qmd intermediate
  is ever written to disk. Both candidate consumers came back empty: 7c's rewrite explicitly declined
  the `jupyter_notebook` arm, and no freeze/incremental layer exists in `quarto-core` to be the other
  (`can_freeze()` is an unconsumed capability flag). Do not resurrect this without a named consumer.

---

## Forward-compatibility obligations (so 7c/ipynb is purely additive)

The implementer MUST keep these general even though 7b only fills the percent/spin arms:

- [x] `ContentProcessor::convert` takes `&ProcessorContext` (asset-writing capable) from the start.
      **Verified 2026-09-24:** `convert(&self, path, content, params, ctx: &ProcessorContext)` at
      `content_processors/mod.rs:101`; `ProcessorContext` threads `Arc<dyn SystemRuntime>` (mod.rs:73).
- [x] `SourceInfo` stays the general Plan-0 enum; **no** percent-specific mapping type leaks into the
      registry. (Revised 2026-08-17: 7c no longer needs a `NotebookCell` variant — cell identity is
      per-*file*, not per-*span* — so this obligation is now just "don't specialize the enum," which
      is cheaper than it was.) **Verified 2026-09-24:** `Converted.source_info: SourceInfo` (mod.rs:81);
      the only processor-specific types are the schema-side `ProcessorSpec` arms, which carry no mapping
      data.
- [x] **`Converted` carries a channel for ephemeral source files** — `files: Vec<(String, String)>`
      (label, logical content), or an equivalent registration handle on `ProcessorContext`.
      **Added 2026-08-17.** Percent and spin never surface this: they map back into the *original*
      file, which the caller already registered. ipynb's `Concat` pieces point at **virtual per-cell
      files that exist only in memory**, so the processor must hand them back for registration in
      `SourceContext`. Without this field, 7c must change `convert`'s return type — exactly the
      non-additive change these obligations exist to prevent. Cheap now, expensive later.
      **Verified 2026-09-24:** `Converted.files: Vec<(String, String)>` at mod.rs:85, empty for
      percent/spin as specified.
- [x] ~~The sidecar envelope is a **versioned tagged union**, not a bare per-line format.~~
      **DROPPED 2026-09-24** (was "suspended" 2026-08-17) — see § Review corrections #2. This
      obligation existed solely so 7c could add a `jupyter_notebook` arm; 7c declined one, and no
      other consumer exists. No envelope type is built by this plan.
- [x] The registry is **name-keyed**; the `processor:` schema is an open union (bare name | map).
      **Verified 2026-09-24:** `Registry { processors: HashMap<&'static str, …> }` (mod.rs:110);
      `parse_processor_spec` accepts bare `spin` or `{name: percent, language, comment?}`
      (`extension/read.rs:698`).

None of these require thinking about ipynb's *conversion semantics* now — they are shape choices only.

---

## Migrated from 7a (kept; the arbitrary-regex mechanism is withdrawn)

- **`processor:` on `claims-files`** replaces 7a's raw `content-pattern` regex (which every engine
  would have had to author). The sniff regex is owned by the processor in Rust, not declared per
  engine.
- **Native discovery admission (7a Stage 6) — WITHDRAWN 2026-09-24, see § Review corrections #1.**
  7a/7b's original model ("a candidate whose extension is claimed with a processor is admitted iff
  the processor's `sniff` matches," at discovery time) is superseded by the shipped
  gate-1/gate-2 model in `project/discovery.rs`: gate 1 only checks extension-level static claim
  (any `FileClaim`, processor or not); gate 2 requires an explicit user `render:` pattern; content
  sniffing never happens at discovery. Not implemented by this plan.
- **One-predicate-two-sites coherence (7a Stage 4) — WITHDRAWN 2026-09-24, same reason.** There is
  only one site (`SourceConversionStage`); no coherence test is needed or possible.
- **Built-in claims as construction-free static data** (7a Stage 5): readable without building the
  registry or launching engines. Still needed — `builtin_file_claims()` (Phase 5) feeds gate 1, not
  a discovery-time sniff.
- **Q6 membership-cache contract** (7a Open Q6): project membership of a content-claimed file is a
  pure function of `(bytes, processor)`; a plain content edit can flip membership (add/remove a
  `# %% [markdown]` cell) with the filename unchanged. **Written into the DocumentProfile/freeze
  design notes** as a contract for whoever builds freeze/incremental — *not* implemented here (no
  membership cache exists today; q2 re-sniffs at claim time every render, Q1-faithful for whatever
  gate 1 + gate 2 already selected). 7b records, per file the claim stage processes via a named
  processor, its content hash + admission bit so a future incremental layer can detect flips without
  re-reading. (Recorded at claim time now, not at a discovery-time scan — there is no such scan.)

**Two-processor `.r`/`.R` tie-break** (7a Q3) — **mechanism corrected 2026-09-24, see § Review
corrections #4.** jupyter (percent) and knitr (spin) both register `.r`/`.R`. Percent's sniff
requires a `# %% [markdown|raw]` cell; spin's requires the `#' ---` header — they rarely collide.
Built-in engines are NOT ordered by `contribution_order` (that governs only extension-engine
ordering, `engine/resolution.rs:89`); they order via `BUILTIN_ORDER = ["knitr", "jupyter",
"markdown"]` (`engine/resolution.rs:66`). A file matching both sniffs is therefore resolved by
`BUILTIN_ORDER` iteration order, and **knitr (spin) wins** the collision, not "first-matching" in
unspecified order.

---

## Phased checklist (TDD — write the listed tests first, watch them fail, implement, watch pass, then `cargo nextest run --workspace`)

Everything lives in `quarto-core`, which feeds `wasm-quarto-hub-client` — full `cargo xtask verify`
(NOT `--skip-hub-build`) before any push.

### Phase 0 — Research + design contracts
- [x] Pin the `knitr` version used as the spin oracle; record it + the relevant `NEWS.md` entries
      (`# %%`, `#|`, `#-`-removal churn) in a research note. **Done 2026-09-24**: knitr 1.50 / R 4.3.2
      confirmed installed; `claude-notes/research/2026-09-24-plan7b-phase0-spike.md`.
- [x] Spike: drive `tree-sitter-r` (`tree_sitter::Parser` + `tree_sitter_r::LANGUAGE`) to extract
      string/comment spans + top-level token starts + `has_error`; confirm it reproduces knitr's
      `matchable` on a handful of string-embedded-marker cases. (Reference: `~/src/air`
      `crates/air_r_parser/src/parse.rs` for driving + parse-error fallback.) **Done 2026-09-24**:
      confirmed via a throwaway example (deleted after); see the research note. The real Phase 4
      algorithm still needs to be designed (top-level-token-start walk, not the spike's linear probe).
- [x] Amend `engine-resolution.md §3.3`: retract "the one genuine must-load case"; document the
      **content-processor** model (named, native, zero-load sniff+convert at claim time; the
      genuinely-dynamic `claims_file` residue is the only must-load path and fails gate 1, excluding
      it from discovery). **Scope expanded 2026-09-24** (§ Review corrections #6): §3.3 currently
      also asserts a content sniff "is statically declarable" *for Pass-1 discovery* — retract that
      too. The sniff is native/zero-load, but it runs at claim time only; it does not make
      discovery content-aware. **Done 2026-09-24.**
- [x] Amend `engine-api-surface.md` to mirror. **Done 2026-09-24.**
- [x] Fix stale doc-comment at `extension/types.rs:141` ("Plan 7a will grow this with an optional
      `content_pattern` field") → reference `processor:`/Plan 7b (§ Review corrections #7) — do this
      in Phase 1 where the file is touched anyway, not here. **Done 2026-09-24** (Phase 1).
- [x] Reframe the root (Plan 7), tombstone 7a, add the 7c placeholder — done in the 2026-07-08
      ratification session itself: Plan 7's status line already reads "ARCHITECTURE ROOT," 7a's
      already reads "SUPERSEDED... Do not execute," and 7c already exists (later rewritten from
      placeholder 2026-08-17). **Checkbox corrected 2026-09-24** — was left unchecked despite being
      complete since this plan's own creation.

### Phase 1 — Schema: `processor:` on `claims-files`
- [x] Tests (`extension/read.rs` unit): bare-name `processor: spin`; map `processor: {name: percent,
      language: julia}` (comment defaults `#`); explicit `comment`; **malformed/unknown processor →
      loud parse error** through `quarto-error-reporting` (never a silent drop). Undotted-lowercase
      extension normalization preserved (1c.2 P4). **Done 2026-09-24** — 6 tests (`t_schema_processor_*`).
- [x] Extend `FileClaim` (`extension/types.rs`) with `processor: Option<ProcessorSpec>` where
      `ProcessorSpec` is the untagged bare-name|map union parsed at read time. **Done 2026-09-24** —
      `ProcessorSpec` resolves directly to a validated `{Percent{language,comment}, Spin}` at parse
      time (comment already defaulted), rather than staying an untyped bare-name\|map union past read
      time. Stale doc-comment at `extension/types.rs:141` fixed in the same edit.
- [x] Migrate existing fixtures to the structured shape (no behaviour change where no processor).
      **Done 2026-09-24** — all `FileClaim { extension: .. }` literals updated to carry
      `processor: None`.

### Phase 2 — Registry + trait + native `markdown_for_file`
- [x] Tests: registry resolves `percent`/`spin` by name; `ProcessorContext` threads a runtime handle;
      default `markdown_for_file` dispatches to the named processor with **no engine object**; a claim
      with **no** processor still routes to the dynamic wire fallback. **Done 2026-09-24** — 13 tests
      across `content_processors::tests` (8), `engine::traits::tests` (3),
      `engine::ts_engine::tests` (2, incl. a real zero-wire-message assertion via `MockTransport`).
- [x] `content_processors/{mod,percent,spin}.rs`: the trait, `ProcessorParams`, `ProcessorContext`,
      `Converted`, the registry. **Done 2026-09-24** — `percent`/`spin` are Phase-2 stubs (`sniff` →
      `false`, `convert` → identity); Phase 3/4 replace their bodies.
- [x] Re-express `ExecutionEngine::markdown_for_file` default to consult the engine's `file_claims()`
      processor for `path` and run it natively. `TsEngine::markdown_for_file` native-first;
      wire-only-when-no-processor (retain the `markdownForFile`/`ClaimsFile` verbs for that residue).
      **Done 2026-09-24** — via a new provided trait method `native_markdown_for_file` (shared by the
      default `markdown_for_file` and `TsEngine`'s override), plus a new `file_claims()` trait method
      (default empty; `TsEngine` returns its existing `claims_files`).

**Cross-cutting finding surfaced mid-Phase-3 (2026-09-24, resolved with Gordon): `SourceConversionStage`
discards `markdown_for_file`'s returned `SourceInfo` entirely, and `ParseDocumentStage` registers the
*converted* buffer as `Original` under a synthetic name ("C′") — faithful original-file remap ("A′") is
explicitly deferred in existing code (`parse_document.rs`, citing plan1c §1060). This means NO engine
today (including TsEngine's existing wire path, which only returns `SourceInfo::generated(By::unknown())`)
gets real original-file provenance. Phase 3's "reports the original file:line:col" and Phase 8's
"confirm the message names the original file/line/column" both assume this wiring exists; it doesn't.
Gordon's call: build the missing pipeline wiring now (`LoadedSource` gains `source_info:
Option<SourceInfo>`; `ParseDocumentStage` registers the original file + remaps through the `Concat`
when present) rather than reducing Phase 3/8 to unit-level-only verification. Tracked as new work below,
inserted before the rest of Phase 3.**

- [x] **A+ pipeline wiring, built 2026-09-24.** Turned out smaller than fresh plumbing: `pampa::
      readers::qmd::read` already has a `parent_source_info: Option<SourceInfo>` parameter (used for
      "recursive parses") that every AST node's `SourceInfo` composes over as a `Substring`
      (`pandoc/location.rs`'s `node_source_info_with_options`) — the missing piece was purely getting a
      real value into it and making that value's `FileId` resolvable.
      - `LoadedSource` gains `source_info: Option<SourceInfo>`.
      - `SourceConversionStage` keeps `markdown_for_file`'s returned `SourceInfo` instead of discarding
        it — but only when it is NOT the dynamic/wire path's `SourceInfo::generated(By::unknown())`
        placeholder (wrapping a `Substring` over a `Generated` parent is meaningless).
      - New convention, `content_processors::ORIGINAL_FILE_ID = FileId(1)`: `qmd::read` always builds a
        **fresh, empty** internal `SourceContext` and registers exactly one file (the converted buffer,
        unconditionally `FileId(0)`) — so `FileId(1)` is deterministically free. A processor's
        `Converted.source_info` must build its `Original` pieces against this constant.
      - `ParseDocumentStage` passes `source.source_info` as `parent_source_info`; when `Some`, it
        re-reads the original file (via `ctx.runtime`) and registers it at `ORIGINAL_FILE_ID` in BOTH
        `ast_context.source_context` (what AST-node `Substring`s resolve against) and the top-level
        `source_context` (ariadne snippets) — mirroring the existing `include_expansion.rs`
        dual-registration precedent for the same reason (the two contexts must stay in lockstep).
      - Tests: `source_conversion.rs` gains `test_claimed_file_faithful_source_info_is_threaded` +
        an assertion on the existing happy-path test that a placeholder is NOT threaded;
        `parse_document.rs` gains `test_parse_document_a_plus_registers_original_file` (asserts the
        original file's real content is registered at `FileId(1)` in both contexts).
      - **Not yet exercised end-to-end with real percent/spin** (they're still Phase-2 stubs) — that
        happens naturally once Phase 3/4 build real `Concat` provenance; Phase 8 is where a full e2e
        "force an error, read the message" check belongs.

### Phase 3 — `percent` processor + SourceInfo
- [x] Tests: port `percent-script.ts`'s behaviour — `[markdown]`/`[raw]` cells, `"""` blocks,
      prefix strip, `#|` options, fence. Golden equivalence vs the TS helper / Q1. SourceInfo: an
      error in a markdown comment and in a code cell both report the **original file:line:col**.
      **Done 2026-09-24** — 12 tests in `content_processors::percent::tests`, including two
      `map_offset`-based SourceInfo assertions (markdown-cell and code-cell body positions resolve to
      the correct original row/col) and one pinning the zero-width-anchor behavior of a purely
      synthetic piece (fence line) — see the scope note below on what "golden vs the TS helper" meant
      in practice.
- [x] Implement percent (`(comment_open, fence_language)`), per-line `Concat`/`Original`. **Done
      2026-09-24** using `quarto_source_map::ProvenanceBuilder` (the same shared run-tiling builder
      quarto-yaml/pampa's div-attribute unescaper/comrak already use for exactly this
      decoded-vs-source shape) rather than hand-rolling `SourceInfo::concat` — `verbatim(range)` for
      copied line content, `replacement(range, 0)` for stripped prefixes/deleted header lines/deleted
      original newlines, `replacement(cursor..cursor, len)` for pure synthesis (fences, `#|` option
      lines, cell-separator blank lines). A purely synthetic piece resolves to a real (zero-width)
      anchor position in the original file, not "no location" — this builder has no `Generated`
      concept; a future consumer that wants "no location" for synthetic spans would need to special-
      case that itself.
  - **Scope cut (documented, not silently dropped):** Q1's raw-cell path additionally dispatches on
    `format:`/`raw_mimetype:` metadata (`mdRawOutput`/`mdFormatOutput` in `to-markdown.ts`) to wrap
    content in a `{=format}` fence. This port treats every raw cell identically to a markdown cell
    (comment-stripped or triple-quote, no format-specific wrapping) — the same kind of explicit,
    narrow cut the plan's own "Boundaries known now" section already makes for block-comment
    languages and YAML front matter. `format`/`raw_mimetype` attributes are parsed but unused.
  - **Scope cut:** `#|` option-line YAML emission is a minimal flat `key: value` emitter, not a real
    YAML serializer (Q1's `asYamlText` via a full YAML library) — adequate for percent's actual use
    (simple `key=value` cell attribs), not general YAML value quoting/escaping.
  - "Golden equivalence vs the TS helper" in practice means: hand-verified against reading
    `percent-script.ts` line by line (this session), not a literal spawned-Deno diff — no test
    harness for running the TS helper exists in this crate, and building one was out of scope for
    validating a native Rust port whose entire point is to need no TS runtime.

### Phase 4 — `spin` processor + SourceInfo (highest risk)
- [x] Tests: **committed knitr golden corpus** — for each `.R` fixture (roxygen `#' ---` header, `#'`
      prose, `#+`/`# ----`/`## @knitr`/`# %%` chunk options, `#| ` pipe options, bare-code→chunk,
      backtick-run edge, string-embedded `#'`), assert the Rust output byte-matches the committed
      `.qmd` (generated by real `knitr::spin`; CI needs no R). `matchable`: a `#'` inside a multi-line
      string is **not** a marker; a parse-error file falls back to all-matchable. SourceInfo across
      inserted fences + stripped prefixes. **Done 2026-09-24** — 12 golden fixtures at
      `crates/quarto-core/tests/fixtures/spin-goldens/{name}.{R,qmd}`, generated with real knitr 1.50
      (`generate-goldens.R`), plus 2 `matchable`-specific tests and 2 `map_offset`-based SourceInfo
      tests (doc line, code line) — 16 tests total in `content_processors::spin::tests`.
  - **Real-knitr-golden-generation caught a genuine modeling error before any Rust was written**: the
    `string-embedded-marker` golden proved that `matchable` gates ONLY the outer doc-vs-code
    classification — the *internal* chunk-delimiter (`rc`)/pipe-comment detection inside an
    already-code-classified block is a **pure textual regex match, not gated by `matchable` at all**.
    A `#+`-looking line *inside* a multi-line string still opens a new chunk in real knitr. Documented
    in `spin.rs`'s module doc so it isn't re-litigated.
- [x] Implement spin (tree-sitter-r `matchable` + the `spin.R` grammar, qmd branch only) + the
      `xtask`/script that regenerates goldens from a pinned knitr. **Done 2026-09-24** —
      `crates/quarto-core/tests/fixtures/spin-goldens/generate-goldens.R` (documents the knitr pin +
      regeneration instructions). `LineSpan`/`scan_lines`/`trim_blank_lines`/`Writer` factored out of
      `percent.rs` into a shared `content_processors::line_writer` module (both processors need
      identical line-scanning + provenance-tiling machinery). `tree-sitter`/`tree-sitter-r` added as
      direct `quarto-core` deps (cross-target, no new wasm32 weight — `quarto-highlight` already pays
      this cost in `wasm-quarto-hub-client`).

### Phase 5 — Built-in engines route through the registry
- [x] Tests: `builtin_file_claims()` returns jupyter's four percent claims + knitr's spin claims
      **without** constructing the registry or launching anything; the default `claims_file`/
      `markdown_for_file` derive from them; a `.py` percent renders via jupyter with a native
      conversion (no Deno, no Rscript). **Done 2026-09-24** — 5 tests: `builtin_file_claims_returns_
      jupyter_and_knitr_claims` (`engine::tests`), `static_file_claims_returns_four_percent_claims` +
      `py_percent_converts_via_native_dispatch` (`engine::jupyter::tests`),
      `static_file_claims_returns_spin_claim` + `r_spin_converts_via_native_dispatch`
      (`engine::knitr::tests`).
- [x] jupyter/knitr populate `file_claims()`; add the construction-free `builtin_file_claims()` for
      discovery. **Done 2026-09-24** — `JupyterEngine::static_file_claims()`/`KnitrEngine::
      static_file_claims()` are inherent fns (no `PATH` probing, unlike `::new()`); `file_claims()`
      trait methods delegate to them; `engine::builtin_file_claims()` unions both (empty on wasm32,
      no built-in engines there).
  - **Real gap found and fixed, not just plumbed through:** neither of the two production
    `RenderableExtensions::new(...)` call sites (`project/mod.rs`, `project/orchestrator.rs`) ever
    consulted built-in engines' claims — only extension-contributed ones. Before this phase, a
    percent `.py`/`.jl`/`.q` or spin `.r` failed discovery's gate 1 *even when explicitly listed* in
    `project.render`, because `.py` etc. were never in the renderable-extension set at all. Both call
    sites now `.chain()` `builtin_file_claims()`'s extensions in. Without this fix, Phase 6's
    "gate 1 already admits a processor-bearing FileClaim with zero changes to discovery.rs" claim
    would have been false for every built-in-claimed extension.

### Phase 6 — Claim-time coherence + launch-free guarantee (REWRITTEN 2026-09-24, see § Review corrections #1)

Discovery itself needs **no new code**: gate 1 (`RenderableExtensions`) already treats any static
`FileClaim` — processor or not — as renderable, and gate 2 (explicit `project.render` pattern, no
default widening) is unchanged. A percent `.py` or spin `.R` becomes a project input exactly when
the author writes a pattern that matches it (`docs/guides/projects/render-list.qmd` already
documents this for the author-facing side). The content sniff runs once, at claim time.

- [x] Tests: with a project that explicitly lists a percent `.py` pattern and a spin `.R` pattern
      under `project.render`, the claim stage (`SourceConversionStage`) claims and natively converts
      both — no engine object constructed, no subprocess. A listed-but-non-matching file (plain
      module `.py`, or an `.R` with neither a `# %%` cell nor a `#' ---` header) hard-errors "Can't
      determine execution engine for `<file>`" (existing stage behavior, Q1-faithful — it is not
      silently dropped, and it is not admitted by falling back to a different processor). A
      **launch-free assertion**: rendering a project of N listed percent/spin scripts issues **zero**
      engine launches in Pass-1 (assert the launch counter / no PID spawned). For the Deno side,
      `TsEngineHost::spawn_count` (`engine/ts_process.rs:714,1370`) is existing instrumentation on a
      long-lived host object — reuse it directly. **For `Rscript` this is net-new, not "the same
      way" (caught 2026-09-24 in blank-slate review):** knitr spawns `Rscript` as a one-shot
      `Command::new` (`engine/knitr/subprocess.rs`) with no persistent host object to hang a counter
      on. Design a test-only spawn-interception point (e.g. an injectable command runner, or a
      test-mode env var the subprocess call checks) rather than assuming an existing counter to read.
      **Done 2026-09-24** — the actual claim-time wiring turned out to be the load-bearing part: a
      new `ExecutionEngine::native_claims_file` trait method (sniff-based, mirrors
      `native_markdown_for_file`) is now consulted by `SourceConversionStage` *before* the dynamic
      `claims_file` — without it, a built-in engine's always-`false` `claims_file` default meant a
      processor-bearing claim could never be reached at claim time even after Phase 5. Net-new
      `RSCRIPT_SPAWN_COUNT` (process-global `AtomicUsize`, incremented in `call_r` right after a
      successful `spawn()`) is the designed interception point; 2 tests in
      `source_conversion::tests` (real `EngineRegistry::new()`, real files, real `NativeRuntime`)
      cover both the launch-free convert path and the non-matching hard-error.
- [x] No discovery-tier code. Confirm (as a test, not an implementation task) that gate 1 already
      admits a processor-bearing `FileClaim` with zero changes to `project/discovery.rs`. **Done
      2026-09-24** — 2 tests in `project::discovery::tests` (`renderable_extensions_admits_a_
      processor_bearing_claim`, `renderable_extensions_admits_every_builtin_claim`); `discovery.rs`
      itself received zero changes this phase (confirming the claim), only its two call sites did
      (Phase 5).
- [x] Record, for each file the claim stage processes via a named processor, its content hash +
      admission bit at claim time; write the Q6 membership-cache contract into the
      DocumentProfile/freeze design notes for a future incremental layer (still not implemented here
      — no membership cache exists today). **Done 2026-09-24** — new "Content-processor membership
      cache (Plan 7b Q6 contract, not yet implemented)" section in
      `claude-notes/designs/document-profile-contract.md`. The content-hash-recording half is
      *documentation only*, per the plan's own framing ("not implemented here") — no
      `DocumentProfile` field was added, since no consumer exists yet to read it (mirrors the sidecar-
      envelope reasoning: don't build storage with no reader).

### Phase 7 — TS-engine native path + Julia `.jl` validation flip (was Plan 7E)
- [x] Tests: julia/marimo `_extension.yml` declare `processor: percent`; a julia `.jl` percent script
      renders with **native** conversion (no Deno in Pass-1), and its error provenance points at the
      original `.jl` line+col (A+, native — no wire `source_map`). **Done 2026-09-24** —
      `julia_fixture_jl_percent_converts_natively` (`engine::ts_engine::tests`) loads the REAL
      committed fixture's `_extension.yml` (not a synthetic `FileClaim`), asserts the parsed claim
      matches, then converts a `.jl` percent script through a `TsEngine` built from it: zero wire
      messages, zero engine launches, and `SourceInfo::Concat` (genuine provenance, not the wire
      path's `Generated(By::unknown())` placeholder).
  - **Marimo scoped out, deliberately:** the committed `marimo` fixture (`tests/fixtures/extensions/
    marimo/`) declares no `file-extensions`/`claims-files` at all — it claims `python`/`sql`
    *languages* (via `whenClass: marimo`) on ordinary `.qmd` cells, never a standalone `.py` file
    needing conversion. There is nothing for it to claim via `processor:` in this repo's fixture, so
    it was left untouched; "julia/marimo" in this phase's original text is read as "a TS engine" in
    general, with julia as the concrete, already-percent-relevant proof.
- [x] Flip Plan 4's now-removed exclusion ("Julia claims by language only; no `claims_file` for `.jl`"
      → julia claims `.jl` percent via the processor). **Done 2026-09-24** — the fixture's
      `_extension.yml` gained `claims-files: [{extension: .jl, processor: {name: percent, language:
      julia}}]`; `engine-resolution.md`'s stale `julia does NOT declare claims-files` example comment
      updated to match. Verified the existing live julia e2e suite (`julia_engine_e2e.rs`, gated on
      real `deno`+`julia` on `PATH`) still passes unchanged with this fixture edit — the claim is
      additive and does not touch execution.

### Phase 8 — End-to-end (CLAUDE.md contract: real binary, inspected output, recorded here)
- [x] `cargo run --bin q2 -- render <fixture project>` with percent `.py`/`.jl` + a spin `.R`: assert
      the docs render, appear in the `ProjectIndex` with converted titles, **and** that Pass-1 spawned
      no `deno`/`Rscript` (launch counter / process check). Paste invocation + output snippets here.
      **Done 2026-09-24.** Built `q2` (`cargo build --bin q2`) and rendered a real project at
      `/private/tmp/.../plan7b-phase8-project` with three files: `notes.py` (percent, jupyter
      claim), `analysis.jl` (percent, jupyter claim), `report.R` (spin, knitr claim) — each a
      percent/spin YAML-header-only fixture with no code cells, so no execution engine ambiguity.
      `report.R` rendered immediately (Rscript available in this environment). `notes.py`/
      `analysis.jl` initially failed with "Engine 'jupyter' is registered but its runtime is not
      available" — **this itself proves the claim was accepted** (a failed *claim* would have said
      "Can't determine execution engine for notes.py" instead); jupyter genuinely wasn't installed.
      Created an isolated throwaway venv (`python3 -m venv`, `pip install jupyter_client ipykernel`,
      `ipykernel install --user --name phase8-py`) rather than touching the system/Homebrew Python,
      put it first on `PATH`, and reran: **all three files rendered, all three appeared with their
      converted YAML-frontmatter titles** ("Percent Python Notes", "Percent Julia Analysis", "Spin R
      Report") in `_site/*.html`'s `<title>`. Cleaned up the venv + kernelspec afterward. The
      "zero Pass-1 launch" numeric proof itself is the Phase 6 `RSCRIPT_SPAWN_COUNT` unit-level
      regression tripwire (a real binary run can't cheaply instrument in-process counters from
      outside without adding new production instrumentation) — this real-binary run's job, and what
      it delivered, is proving the *feature* renders correctly end to end.
- [x] Inspect provenance: force an error in a converted cell; confirm the message names the original
      file/line/column. **Done 2026-09-24 — found and fixed a real, previously-latent bug in `pampa`
      along the way.** Forced a malformed-YAML front-matter error in a spin `.R` fixture
      (`title: [oops this bracket never closes`) and rendered it. The ariadne snippet's file label
      read `<.../broken.R (converted by knitr)>:2:1` — the **C′ synthetic converted-buffer name**,
      not the original file. Root cause: `pampa`'s `document.rs`/`section.rs`/`fenced_div_block.rs`
      (the three sites that turn a YAML front-matter node into a `RawBlock`) built that block's
      `SourceInfo` directly via `SourceInfo::from_range(context.current_file_id(), range)` — or, in
      `fenced_div_block.rs`, a literally hardcoded `FileId(0)` — **bypassing `parent_source_info`
      entirely**, unlike every other node kind (which goes through `node_source_info_with_context`).
      This is a **latent, pre-existing gap**, not something Plan 7b's own code introduced: every
      earlier `parent_source_info` reroot test in the codebase (e.g. `qmd.rs`'s
      `err_path_diagnostics_reroot_through_parent_source_info`) happened to use a simple `Original`/
      `Substring` parent, for which `resolve_byte_range()` still resolves; percent/spin's `Concat`
      parent is the first case where `resolve_byte_range()` genuinely returns `None` (by the type's
      own contract — see `SourceInfo::resolve_byte_range`'s doc comment) and the ariadne renderer's
      `root_file_id()` call (`quarto-error-reporting`'s `render_ariadne_source_context`) was the one
      call site that needed `parent_source_info` and wasn't getting it. **Fix:** all three call
      sites now use the existing `range_to_source_info_with_context` helper (already used by other
      node kinds for exactly this purpose) instead of building `SourceInfo` directly. **Verified via
      TDD** (`pampa/src/readers/qmd.rs`'s new `yaml_frontmatter_source_info_reroots_through_concat_parent`
      test, confirmed RED against the original `document.rs` code, GREEN against the fix) and via
      the real binary: re-running the same broken `.R` file now shows `broken.R:2:4` — the **true
      original file, line, and column** — with the ariadne snippet displaying the real `#'`-prefixed
      source lines. Full `pampa` (nextest: 4796 passed, 2 skipped) and `quarto-core` (nextest: 4938
      passed, 31 skipped) suites confirmed green with the fix applied.

### Phase 9 — Coordination + docs
- [x] **Plan 6 (concurrent sibling):** add a coordination note — native conversion makes percent/spin
      Pass-1 profiles **hashable**, so the Pass-1 cache key should fold in the *processor version* +
      converted output; Plan 6 decision-9's "unhashed `.js` conversion" caveat and P1's
      "may have loaded a content-inspecting engine" parenthetical are **removed for percent/spin** when
      both settle. No dependency either way.
      **Done 2026-09-24:** coordination note added under decision 9 of
      `2026-06-29-plan6-pass1-engine-resolution.md`, plus a pointer appended to the P1 parenthetical.
- [x] **Plan 4b:** relabel the `content-claim` fixture as the *dynamic residue*; note it is excluded
      from Pass-1 discovery (decision 3) and is the only surviving must-load path.
      **Done 2026-09-24:** relabel note added to the `content-claim` fixture item in
      `2026-07-01-plan4b-shadow-engine-features.md`.
- [x] **Grand plan sub-plans table** — done 2026-08-17, ahead of implementation, so the epic's index
      stops contradicting the 7a tombstone: 7 → series root (◍), 7a → tombstoned, 7b/7c rows added,
      totals + status key updated.
- [x] **Grand plan sub-plans table, row for Plan 7b** (`2026-04-16-ts-engine-extensions-subprocess.md:569`):
      dropped "Additive; not on the critical path," replaced with the Julia-bundling blocker note.
      **Done 2026-09-24** (this review).
- [x] **1c.2 P4:** record that `processor:` extends the structured `claims-files` it delivered.
      **Done 2026-09-24:** note appended to the P4 restructure item in
      `2026-07-01-plan1c2-engine-extensions-loose-ends.md`.
- [x] User docs (`docs/`, usage not internals): the `processor:` declaration for extension authors.
      **Scope corrected 2026-09-24** (§ Review corrections #5) — `docs/guides/projects/render-list.qmd`
      already documents the percent/spin auto-discovery divergence; extend that page rather than
      writing it fresh. Verify with `cargo run --bin q2 -- render docs/` (never Q1).
      **Done 2026-09-24:** new "Listed scripts with a declared processor" section added to
      `render-list.qmd` (built-in percent/spin claims + the extension-author `processor:` YAML shape,
      matching `parse_processor_spec` and the julia fixture). Verified with
      `cargo run --bin q2 -- render docs/guides/projects/render-list.qmd` — rendered cleanly and the
      new section + YAML sample confirmed present in `docs/_site/guides/projects/render-list.html`.
- [x] Reconcile this checklist against reality; commit; ask Gordon before any push/merge. **Target
      updated 2026-09-24:** `feature/ts-engine-extensions` merged to `main` via PR #416 — merge to
      `main` (or its current integration branch, if one exists at execution time), not the
      now-closed epic branch.
      **Reconciled 2026-09-24:** every box re-verified against code/docs (the four forward-compat
      obligations were confirmed in `content_processors/mod.rs` + `extension/read.rs` and checked off
      with citations; Phase 9's four items landed this session). Workspace gate: `cargo nextest run
      --workspace` at HEAD (`3772390af`) — **14793 passed, 200 skipped, 0 failed**; the branch diff
      vs `origin/main` adds exactly 60 `#[test]` functions and removes none, so the delta is +60,
      all from this plan's TDD phases. Note: `origin/main` advanced past the branch point today
      (nested-projects render-list work, PR #722) — it extends `render-list.qmd` with a disjoint
      "Nested projects" section; no textual conflict with this plan's docs edit, but rebase/merge
      will pull both in.

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
| T-discovery-noop | Phase 6 | unit | **REWRITTEN 2026-09-24** — gate 1 (`RenderableExtensions`) admits a processor-bearing `FileClaim` with zero code changes to `discovery.rs`; no discovery-time sniff exists to revert. This is a real unit test (construct a processor-bearing `FileClaim`, assert `RenderableExtensions`/gate 1 admits it), **not** a git-diff-empty check on `discovery.rs` — the "no changes needed there" claim is a PR-review note, not something to script into CI (clarified 2026-09-24 in blank-slate review) |
| T-claim-stage-error | Phase 6 | unit | a listed percent `.py`/spin `.R` that fails its sniff hard-errors "Can't determine execution engine," not silently dropped. Revert the claim-stage error path → file silently vanishes → RED (was T-coherence; discovery has no second site to compare against, see § Review corrections #1) |
| T-launch-free | Phase 6 | integration | N percent/spin scripts in a project ⇒ **zero** engine launches in Pass-1 (launch counter). Revert native dispatch (fall to wire) → a launch fires → RED |
| T-e2e-percent | Phase 8 | e2e | percent `.py`/`.jl` renders; ProjectIndex entry; no `deno` in Pass-1. Revert TS native-first → Deno spawns → RED |
| T-e2e-spin | Phase 8 | e2e | spin `.R` renders; no `Rscript` in Pass-1. Revert built-in spin routing → RED |
| T-provenance | Phase 7/8 | e2e | error in a converted `.jl` cell names original file:line:col. Revert SourceInfo wiring → RED |

**Accepted-untested / deferred (logged):**
- Sidecar envelope — **dropped, not deferred** (2026-09-24, § Review corrections #2). No envelope
  type is built; nothing here to defer.
- Full R-tokenizer parity beyond tree-sitter-r's `matchable` (tree-sitter-r *is* the oracle; knitr's
  own fallback is all-matchable).
- spin non-qmd output branches (`Rnw`/`Rhtml`/`Rtex`/`Rrst`), inline `{{ }}` expansion, knitr-compile
  knobs (`report`/`precious`/`knit`).
- ipynb (Plan 7c).
- Dynamic-claim files as project inputs (fail gate 1 per revised decision 3; still explicit-single-file renderable).

---

## Dependencies & sequencing

- **Depends on (landed on `main`, code-verified 2026-09-24):** Plan 1c (`claims_file`/`markdown_for_file`
  trait surface + the conversion stage, renamed `SourceConversionStage` in the 2026-08-13 merge —
  was `EngineClaimsFileStage`), Plan 0 (`Concat`/`Original` `SourceInfo`, confirmed in the external
  `quarto-source-map` crate), Plan 3 (`percent-script.ts` as the port reference, confirmed at
  `ts-packages/quarto-api/src/jupyter/percent-script.ts`), 1c.2 P4 (structured `claims-files` /
  `FileClaim`, confirmed at `extension/types.rs:143`).
- **Reuses:** `tree-sitter-r = "1.2"` + `tree-sitter` (already workspace deps via `quarto-highlight`,
  native + wasm32 — confirmed).
- **Orthogonal to** Plan 5 (pooling). Plan 6 has landed (both were on `feature/ts-engine-extensions`,
  now merged to `main`) — no ordering dependency; coordination note in Phase 9.
- **No longer "not on the critical path"** — see header.

## References
- Grand plan `2026-04-16-ts-engine-extensions-subprocess.md` (Pass-1/Pass-2 model; the no-Pass-1-load
  principle, L82).
- 7-series root `2026-06-27-plan7-native-percent-spin-sourceinfo.md`; tombstone
  `2026-07-07-plan7a-static-content-pattern-claims.md`; ipynb `2026-07-08-plan7c-ipynb-content-processor.md`.
- SourceInfo design `2025-12-15-source-info-for-structured-formats.md` (column technique,
  `NotebookCell`, sidecar envelope — the latter two are **not built** by this plan or by 7c; see
  § Review corrections #2 and 7c's own "Corrections to the original 7c stub").
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
