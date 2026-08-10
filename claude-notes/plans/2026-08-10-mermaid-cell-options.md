# Mermaid `%%|` cell options are not processed (bd-mermaid-cell-options-9wo3crl0)

**Date:** 2026-08-10
**Braid:** bd-mermaid-cell-options-9wo3crl0
**Branch:** `feature/bd-mermaid-cell-options-9wo3crl0` (no worktree; the
investigation commit `f44b3a81` moved onto this branch and `main` was rewound to
`a217ab5a`, so the whole strand lands as one PR)
**Status:** Implemented 2026-08-10, pending review. `cargo xtask verify` (full)
passes. See "Resolved decisions" and the verification record.

## Triage verdict

**Ready to design**, and the shape is much smaller than the strand assumed: the
caption/figure/crossref machinery already works end-to-end for a mermaid fence —
it just keys off the `#|` marker instead of mermaid's `%%|`. The narrow fix is to
make the pre-engine cell-option desugar *language-aware*. Two adjacent gaps
surfaced during the probe that are **not** mermaid-specific and probably want
their own strands.

## Issue context

Filed 2026-08-10 by Carlos, `feature`, priority 1, label `parity`, no dependency
edges. Direction recorded in the description: q2 standardizes on the **GFM
spelling** (```` ```mermaid ````), not Q1's ```` ```{mermaid} ```` executable-cell
form. Phase one (diagrams render at all) is already done via
`crates/quarto-core/src/transforms/mermaid.rs` (bd-5m4ga0s1, plan
`claude-notes/plans/2026-07-20-mermaid-regular-rendering.md`). The live work is
phase two: pull the `%%|` cell options out of the diagram source and route
`fig-cap` / `fig-alt` into the output.

Real-world driver: the Posit Connect docs port — 33 diagrams across 14 pages, all
in the Q1 brace form. Rewriting them to the GFM spelling makes them draw
immediately; captions and `fig-alt` accessible descriptions stay lost until this
strand lands.

## Dependency graph

**Empty.** `braid dep tree` and `braid dep list` both return no edges — no
incoming pressure from a blocked strand, and no `discovered-from` parent in this
skein. The origin strand lives in a *different* skein (the connect-docs porting
project, `br-mermaid-cell-options-r52f8p3v`) and is not reachable from here; the
strand description carries the context that edge would have.

Neighbors found by text search (informational only, no edges):

- **bd-5m4ga0s1** (closed) — the transform this strand extends.
- **bd-c3dtpe36** (open) — "Mermaid render component for q2-preview
  (experiment → built-in path)". Owns the React side; relevant to the
  preview/render parity question below.
- **bd-sehm2rha**, **bd-nj25kgbu** (open) — mermaid *theming* (`$mermaid-*` SCSS
  vars, `--mermaid-*` CSS bridge, `mermaid:` document metadata). Adjacent
  surface: whichever of these lands first settles where mermaid-scoped
  *document* metadata is read, which interacts with question 4 below.

## What the code looks like today

Everything the strand description points at still exists with the described
shape. HEAD is `a217ab5a`; `cargo xtask verify --skip-hub-build` is green after a
WASM rebuild (the one failure — a `named-entities` hub-client WASM smoke test —
was **stale WASM**, not a real regression: `npm run build:wasm` followed by the
same suite passes clean. Skipping the hub *build* still runs the hub *tests*
against whatever `.wasm` is on disk, which is the trap here).

### The relevant machinery, and how it actually composes

Three pieces, in pipeline order:

1. **`crates/quarto-core/src/cell_options/mod.rs`** — q2's single, proper
   cell-option facility: `comment_syntax_for(language)` (a port of Q1's
   `kLangCommentChars`), `partition_cell_options()` splitting a cell body into a
   YAML options block + code with real source attribution, `options_to_config()`,
   `merge_cell_over_scope()`. **It has no `mermaid` entry**, so mermaid falls
   through to the `#` default. Q1 keeps mermaid's `%%` in
   `src/resources/filters/modules/constants.lua` (`mermaid = {"%%"}`) and on the
   handler (`comment: "%%"` in `src/core/handlers/mermaid.ts`), *not* in
   `kLangCommentChars` — which is exactly why the port missed it.

2. **`crates/quarto-core/src/crossref/codeblock_shorthand.rs`**, run from
   `PreEngineSugaringStage` (`stage/stages/pre_engine_sugaring.rs:142`) — the
   desugar that turns

   ```
   ```mermaid
   #| label: fig-x
   #| fig-cap: A caption.
   …
   ```
   ```

   into `Div(#fig-x)[ CodeBlock, Paragraph(caption) ]`. **It does not use the
   `cell_options` facility**: it carries its own `parse_cell_options()` that
   hard-codes the literal `"#|"` prefix and does a naive `split_once(':')`
   instead of parsing YAML. (The `cell_options` module doc explicitly names this
   matcher as a consumer that "should migrate here" — that migration never
   happened.)

3. **`crates/quarto-core/src/transforms/mermaid.rs`** — `Finalization`-phase
   transform, recurses into `Div`/`Figure`/lists, rewrites the `CodeBlock` to
   `RawBlock("html", "<pre class=\"mermaid\">…</pre>")`, appends the pinned-CDN
   module script once. Excluded from the preview pipeline
   (`Q2_PREVIEW_TRANSFORM_EXCLUDED`) so the raw `CodeBlock` survives to
   `ts-packages/preview-renderer/src/q2-preview/blocks/MermaidCodeBlock.tsx`.

Because (2) runs pre-engine and (3) recurses into `Div`, the two compose
**already**. That is the key finding.

### Probe results at HEAD

Repro + probes committed under
`claude-notes/plans/mermaid-cell-options-investigation/`. Rendered with
`cargo run --bin q2 -- render <probe>.qmd`; HTML inspected directly.

| probe | source | observed at HEAD |
|---|---|---|
| B1 | ```` ```mermaid ```` + `%%\| fig-cap` / `%%\| fig-alt`, no label | options verbatim in `<pre>`, no figure, no caption, no alt — **the reported bug** |
| B2 | ```` ```mermaid ```` + `%%\| label: fig-diagram` + `%%\| fig-cap` | options verbatim; `@fig-diagram` emits `?fig-diagram?` + an unresolved-crossref warning |
| **B3** | ```` ```mermaid ```` + **`#\|`** `label` + `fig-cap` | **fully works** — `<div id="fig-hash" class="quarto-float quarto-figure">`, `<figure>`, `<div aria-describedby="fig-hash-caption">`, `<figcaption>Figure 1: …</figcaption>`, and the diagram still converts to `<pre class="mermaid">` |
| B4 | a `%%\|` line *below* the first code line | correctly left alone (leading-run-only semantics already hold) |

So the entire figure/caption/crossref/`aria-describedby` path is live for mermaid
today. **The only thing missing for the labelled case is that `%%|` isn't
recognized as mermaid's option marker.** Verbatim from B3's output:

```html
<div id="fig-hash" class="quarto-float quarto-figure quarto-figure-center">
<figure class="quarto-float quarto-float-fig">
<div aria-describedby="fig-hash-caption">
<pre class="mermaid">
flowchart LR
  I --&gt; J
</pre>
</div>
<figcaption id="fig-hash-caption" class="quarto-float-caption-bottom quarto-float-caption quarto-float-fig">
<p>Figure 1: &quot;Hash-prefixed.&quot;</p>
</figcaption>
</figure>
</div>
```

### Three defects the probe exposed in the *shared* shorthand path

These are **not mermaid-specific** — probe3 reproduces all three on a
```` ```python ```` cell with `#|` options, so every `#|` code cell in the tree is
affected. Note the `&quot;` in B3's figcaption above:

- **D1 — quoted YAML strings keep their quotes.** `#| fig-cap: "Quoted caption."`
  renders as `Figure 2: "Quoted caption."` (literal quote characters). Cause:
  `parse_cell_options`' `split_once(':')` + `trim()` instead of YAML parsing.
- **D2 — markdown in captions is not parsed.** `#| fig-cap: A *emphasized*
  caption with [a link](…)` renders the asterisks and brackets literally. Cause:
  `caption_paragraph()` builds a single `Inline::Str` rather than parsing the
  caption as inlines.
- **D3 — `fig-alt` (and `fig-scap`) are consumed and silently dropped.**
  `partition_options()` classifies `<reftype>-alt` and `<reftype>-scap` as
  *consumed* — removing them from the code body — but only `<reftype>-cap` is
  ever read back into the Div scaffold. The text vanishes with no warning. Since
  `fig-alt` is precisely what the strand cares about (the accessibility of the
  Connect reference-architecture pages rests on it), **D3 is on this strand's
  critical path** even though it is a general bug.

Both D1 and D2 disappear naturally if the shorthand is migrated onto
`cell_options::partition_cell_options` (real YAML) plus an inline parse for the
caption — which is the same migration that makes `%%|` work. That argues for
doing the migration rather than bolting a second prefix onto the ad-hoc parser.

### Preview / render parity comes for free

`PreEngineSugaringStage` is a *stage*, and `Q2_PREVIEW_TRANSFORM_EXCLUDED` only
filters *transforms*. The stage runs in all three pipelines (`pipeline.rs:321`,
`:573`, `:707`). So if option-stripping and figure-scaffolding happen at the
pre-engine stage, `q2 preview` gets the stripped source and the caption structure
too, without touching `MermaidCodeBlock.tsx`. Doing the work inside
`transforms/mermaid.rs` instead would **break parity** (the transform is excluded
from preview) *and* would land after `crossref-render`, so a `label:` could never
be numbered. This is the strand's most important architectural constraint.

## Resolved decisions (Carlos, 2026-08-10)

1. **Unlabelled `fig-cap` emits `Block::Figure`.** A bare `fig-cap` with no
   `label:` becomes a real `Figure` node, which the HTML writer already renders
   as `<figure>…<figcaption>` with no number and no float scaffolding. Rationale
   worth preserving: *Q1 could not do this because its cell handling was entirely
   textual — it had to emit markdown and let a filter rebuild the structure. We
   are working on the AST, so we can construct the node directly.* Labelled
   captions keep going through the existing crossref float Div, which is what
   gives them `Figure N:` numbering and `aria-describedby`.

2. **`fig-alt` is injected as mermaid's native `accDescr:` directive** (option
   (c)). It survives mermaid.js's replacement of the `<pre>` with an inline
   `<svg>`, which `aria-label` on the `<pre>` would not.
   **Recorded for the future:** option (a) — putting the accessible name on the
   emitted element — *becomes the right answer once we render diagrams
   server-side for PDF/print output.* At that point there is a real image element
   to carry `alt`, the runtime SVG swap no longer happens, and `accDescr:` inside
   the diagram source stops being the mechanism that reaches assistive tech. Any
   future server-side-rendering strand should revisit this decision rather than
   assume `accDescr:` generalizes.

3. **D1, D2 and D3 are all fixed on this strand**, one commit per defect so the
   PR reads as a sequence of reviewable changes. The three `discovered-from`
   strands (bd-5jcmmj1f, bd-sdpp9rw4, bd-il6pxq4f) stay open as the record and
   close when this lands.

4. **Unknown option keys warn, with a source-mapped diagnostic.** This is what
   makes migrating onto `cell_options::partition_cell_options` load-bearing
   rather than incidental: it is the only path that carries real spans, so the
   diagnostic can point at the offending key rather than at the block.
   **Scope limit — important:** the warning applies to *non-executable* diagram
   cells only. For an executable cell (` ```{python} `) an unrecognized key is
   normally an engine option (`echo`, `eval`, `warning`, engine-specific keys)
   and must keep passing through silently; warning there would fire on
   essentially every real document. So "unknown key" is defined against the
   recognized set *for a diagram language*, and the executable path keeps its
   current passthrough behavior.

5. **Q1 parity on the marker: ` ```mermaid ` accepts `%%|` only.** `#|` in a
   mermaid fence stops being a cell-option marker — a deliberate behavior change
   from what probe B3 does today. Rationale: q2 is in `0.*`; being strict now
   avoids teaching people that `#|` is universal. Consequence worth handling:
   `#` is *not* a mermaid comment character, so a leftover `#| …` line becomes
   diagram source and mermaid renders a syntax error. A leading run of `#|` lines
   in a mermaid block therefore gets its own diagnostic pointing at `%%|`,
   so the failure is legible rather than a broken diagram.

6. **Lands on `feature/bd-mermaid-cell-options-9wo3crl0`**, no worktree. The
   investigation commit moved onto the branch; `main` was rewound to `a217ab5a`.

## Phases

- [x] **Phase 0 — Investigation** (commit `f44b3a81`): plan + probes.
- [x] **Phase 1 — `cell_options` learns mermaid** (commit `3407e7db`). Add `"mermaid" => ("%%",
      None)` to `comment_syntax_for`, with tests (including that a matlab/tikz
      `%` cell and a mermaid `%%` cell do not cross-talk).
- [x] **Phase 2 — Migrate `codeblock_shorthand` onto `cell_options` (fixes
      D1)** (commit `6347b3b3`). Language-aware prefix selection (language = the block's first
      class, brace forms excluded) + real YAML parsing, replacing the
      hard-coded `"#|"` matcher and `split_once(':')`. Makes `%%|` work and
      makes quoted captions come out unquoted.
- [x] **Phase 3 — Caption inlines (fixes D2)** (commit `d8c01072`). Parse
      `fig-cap` as markdown inlines instead of a single `Str`.
- [x] **Phase 4 — Unlabelled `fig-cap` → `Block::Figure`** per decision 1
      (commit `1adc3ece`); `fig-scap` → `Caption::short`.
      *Swapped with the original Phase 5*: the unlabelled path had to exist
      before `fig-alt` could compose with it, otherwise Phase 5 would have
      built a structure Phase 4 immediately tore down.
- [x] **Phase 5 — `fig-alt` + `fig-scap` (fixes D3)** (commit `02cfccf4`).
      Stop consuming what cannot be routed; route `fig-alt` into `accDescr:`
      per decision 2.
- [x] **Phase 6 — Diagnostics** (commit `0233f3cc`, renumbered on rebase). Q-2-47 (option ignored on
      a diagram cell, distinguishing unknown from recognized-but-unroutable)
      and Q-2-48 (`#|` where `%%|` is expected), both source-mapped to the
      offending key.
- [x] **Phase 7 — End-to-end + preview verification.** See below.
- [x] **Phase 8 — Docs.** `docs/guides/authoring/diagrams.qmd` gains a
      "Captions, cross-references, and alt text" section, and the
      "Differences from Quarto 1" list drops captions/labels/cross-references.
      New worked example `examples/diagrams/03-mermaid-captions` registered in
      `examples/manifest.yml`.
- [x] **Phase 9 — filed as its own strand, bd-vg8p5yoj**: `qmd-syntax-helper`
      rule rewriting ```` ```{mermaid} ```` → ```` ```mermaid ````. Cell
      options need no rewrite — Q1 documents already write `%%|`, which is
      exactly what this strand made q2 accept.

## Rebase note: error codes renumbered

The branch was rebased onto `origin/main` before opening the PR, and
`main` had meanwhile claimed **Q-2-42 through Q-2-46** (conditional-content
attributes, callout titles, reference-style links — PR #497). The two codes
added here were renumbered **Q-2-42 → Q-2-47** and **Q-2-43 → Q-2-48**; the
catalog conflict was resolved by keeping `main`'s entries and appending the
renumbered pair. The Phase 6 commit *message* still names the pre-rebase
numbers — the code, catalog, tests and this plan all use the final ones.

Worth knowing for the next person: `error_catalog.json` is an append-heavy
shared file with no reservation mechanism, so two branches adding codes in
the same window will always collide there. The conflict is mechanical, but
the renumber has to be chased through every `with_code(...)` call, its
tests, and any prose that quotes the code.

## Verification record (Phase 7)

**`cargo xtask verify` (full, including the hub/WASM leg): all steps passed.**

### `q2 render`

`cargo run --bin q2 -- render claude-notes/plans/mermaid-cell-options-investigation/probe.qmd`,
output inspected:

- **B1** (`%%| fig-cap` + `%%| fig-alt`, no label) →
  `<div class="quarto-figure quarto-figure-center"><figure><pre class="mermaid">`
  whose second line is `accDescr: Two nodes connected by an arrow.`, followed by
  `<figcaption>A tiny flowchart.</figcaption>`.
- **B2** (`%%| label: fig-diagram` + `%%| fig-cap`) → the numbered float,
  `<figcaption id="fig-diagram-caption">Figure 1: A labelled flowchart.</figcaption>`,
  and `@fig-diagram` resolving to a live `Figure 1` link (it previously emitted
  `?fig-diagram?` plus an unresolved-crossref warning).
- **B3** (`#|` in a mermaid fence) → left inert, with Q-2-48 reported at
  `probe.qmd:28:1` — the exact line of the `#|` run.
- **B4** (a `%%|` below the first code line) → left as an ordinary comment.

`…/probe4.qmd` exercises the diagnostics: Q-2-47 at `9:5` (`echo`) and `10:5`
(`theme`) with the caret on the key, Q-2-47 at `20:5` naming `fig-scap` as
recognized-but-ineffective, and **no** diagnostics from the ```` ```{python} ````
cell in the same document.

### The accessibility claim, verified against real mermaid

Both browser routes were unavailable in this session (the claude-in-chrome
extension was not connected; the chrome-devtools MCP profile was locked by an
already-running Chrome). Instead the claim was verified **headlessly against
real mermaid 11.12.0** — the version `MERMAID_VERSION` pins — under jsdom, with
the exact text `inject_acc_descr` emits:

```
desc text        : "Two nodes connected by an arrow."
aria-describedby : chart-desc-probe
desc id          : chart-desc-probe
ids match        : true
placement-before : REJECTED — UnknownDiagramError: No diagram type detected …
```

So the injected line becomes `<desc>` inside the SVG with the SVG's
`aria-describedby` pointing at it, and the negative control confirms the
placement constraint is real: `accDescr:` *before* the diagram-type line is
rejected outright, which is why the directive is inserted after the first
declaration line rather than at the top.

**Not verified:** how the result presents in an actual screen reader, and the
`q2 preview` UI in a live browser session.

### Preview parity

Pinned by `pipeline::tests::render_qmd_to_preview_ast_processes_mermaid_cell_options`:
the preview AST carries the `Figure` wrapper, the caption inlines, the injected
`accDescr:`, and no `%%|` — while the diagram still arrives as a `CodeBlock` for
`MermaidCodeBlock.tsx` (`mermaid-render` stays on
`Q2_PREVIEW_TRANSFORM_EXCLUDED`). That is the intended difference between the
surfaces, and the test pins it so neither list can drift silently.

Reading that AST surfaced an unrelated pre-existing issue, filed as
**bd-e3m3rkik**: because `mermaid-render` is excluded from preview,
`CodeBlockRenderTransform` *does* see mermaid blocks there and wraps them in
copy-button chrome that `q2 render` suppresses by transform ordering.

### Docs

`cargo run --bin q2 -- render docs/` → 194 of 194 files, no errors. The
diagrams page carries the new section and the `Demo 3` cross-reference to the
embedded example.

## Risks / tradeoffs (draft)

- **Blast radius of the `codeblock_shorthand` migration.** That desugar feeds
  every `#|` code cell in the tree — engine round-tripping
  (`crossref/roundtrip_tests.rs` depends on the Div-at-matching-depth invariant),
  crossref fixtures, and snapshots. The `cell_options` facility is the right
  destination, but the switch from line-level string surgery to YAML parsing is
  the largest single risk on this plan.
- **`passthrough` semantics.** `strip_consumed_lines` keeps unconsumed option
  lines *textually*, which the engine then re-parses. Moving to structured
  parsing means deciding how to re-emit them. Per decision 4 the executable path
  keeps textual passthrough; only diagram cells consume every key.
- **Language detection.** Picking the comment syntax needs the block's language,
  which for a plain fence is the first class. Brace-form classes arrive as
  `{mermaid}` (see the existing `brace_form_mermaid_cell_untouched` test) —
  whatever normalization we add must not accidentally start matching brace cells,
  which are deliberately engine territory.
- **The `%%|` prefix collides with matlab/tikz.** `comment_syntax_for` maps those
  to `%`, so a matlab cell's `%|` and a mermaid cell's `%%|` are different
  markers. `option_content_ranges` strips the prefix then requires `|` — a
  mermaid line `%%| x` under a `%` syntax would see `%| x` and fail the `|`
  check, so there's no silent cross-talk. Worth a test either way.
- **`aria-describedby` double-description** (question 2) — the figure scaffold
  already emits one; adding a second description without a plan produces
  confusing screen-reader output, which would be an own-goal on an
  accessibility-motivated change.
