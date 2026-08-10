# Mermaid `%%|` cell options are not processed (bd-mermaid-cell-options-9wo3crl0)

**Date:** 2026-08-10
**Braid:** bd-mermaid-cell-options-9wo3crl0
**Branch:** `main` (investigation committed in place — no worktree was created; see
"Where this should land" below)
**Status:** Investigation — pending design alignment with user. **Do not start
implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion below.

- **Phase 0 — Test plan (TDD, failing first).** Unit tests in
  `cell_options` (mermaid → `%%`), in `codeblock_shorthand` (prefix selection,
  YAML-quoted values, inline captions, `fig-alt` routing), and at least one
  end-to-end `q2 render` fixture asserting `<figcaption>` + the accessible-name
  markup on a `%%|` mermaid block. Per CLAUDE.md, the e2e test must drive the
  real render path, not `render_qmd_to_html` with defaults.
- **Phase 1 — Teach `cell_options` about mermaid.** Add `"mermaid" => ("%%",
  None)` to `comment_syntax_for`. Cheap, isolated, testable.
- **Phase 2 — Migrate `codeblock_shorthand` onto the `cell_options` facility.**
  Replace the hard-coded `"#|"` matcher with a language-driven one (language =
  the code block's first class, minus brace form). Fixes D1 for free. Needs care
  around the `passthrough` set, which currently relies on line-level rewriting.
- **Phase 3 — Route `fig-alt` (D3) and parse caption inlines (D2).** Decide the
  markup for the accessible name (question 2) and stop dropping `fig-scap`.
- **Phase 4 — Unlabelled `fig-cap`.** Make `fig-cap` without `label:` produce a
  caption (question 1).
- **Phase 5 — Preview verification.** Confirm the React path renders the same
  structure; only touch `MermaidCodeBlock.tsx` if the parity check says so.
- **Phase 6 — Docs.** `docs/guides/authoring/diagrams.qmd` has no captions/alt
  section; add one showing the `%%|` form.
- **Phase 7 (separate strand?) — `qmd-syntax-helper` rule** rewriting
  ```` ```{mermaid} ```` → ```` ```mermaid ````, and `%%|`/`#|` normalization if
  we settle on one marker. Rule surface: `crates/qmd-syntax-helper/src/rule.rs`,
  conversions in `src/conversions/`.

## Open design questions for the user

1. **Unlabelled `fig-cap`.** Today a caption only appears when there is also a
   `label:` that classifies as a crossref — `codeblock_shorthand` returns early
   otherwise (probe C4: nothing happens). Q1 emits an unnumbered
   `<figure><figcaption>` for a bare `fig-cap`. Should q2 do the same — and if
   so, via a `Block::Figure` (which the HTML writer already renders with a bare
   `<figcaption>`), or by extending the Div scaffold to an id-less float? This
   matters a lot for the Connect docs, where captions may well appear without
   labels.

2. **What markup does `fig-alt` produce?** The output here is `<pre
   class="mermaid">` that mermaid.js replaces with an inline `<svg>` at runtime —
   there is no `<img alt>` to hang it on. Candidates: (a) `aria-label` on the
   `<pre>`; (b) a visually-hidden `<div>` plus `aria-describedby` (note the
   figure scaffold *already* emits `aria-describedby` pointing at the
   figcaption — a second description would need reconciling); (c) mermaid's own
   `accDescr:`/`accTitle:` directives injected into the diagram source, which is
   the mermaid-native answer and survives the SVG swap. I lean toward (c) with
   (a) as a fallback, but it's a real decision and it's the accessibility story
   for those Connect pages.

3. **Scope: do D1/D2/D3 get fixed here or split out?** They are pre-existing
   general bugs in the `#|` shorthand, not mermaid regressions. D3 (`fig-alt`
   dropped) is unavoidable here. D1 (quotes) falls out of the `cell_options`
   migration whether we want it or not. D2 (markdown captions) is genuinely
   separable. Options: (i) fix all three on this strand, (ii) fix D1+D3 here and
   file D2, (iii) file all three and make this strand depend on them. Whichever
   we pick, D1 and D2 will move existing snapshots — expect a snapshot diff to
   report per the CLAUDE.md snapshot policy.

4. **Which option keys are honoured, and what happens to the rest?** The strand
   asks. Beyond `label` / `fig-cap` / `fig-alt`: `fig-scap`? `fig-align`?
   `mermaid-format`/`theme` (which overlap bd-sehm2rha / bd-nj25kgbu)? And for
   unrecognized keys: today unconsumed `#|`/`%%|` lines are *left in the code
   body* — harmless for mermaid (`%%` is a comment, the diagram still draws) but
   visible in the source and visible in `q2 preview`. Q1 strips every option
   line. Do we strip all of them, and do unknown keys warn or go quiet?

5. **One marker or two?** Should ```` ```mermaid ```` accept `%%|` *only* (Q1
   parity), or keep accepting `#|` as well (which works today, per probe B3, and
   which someone may already depend on)? Accepting both is the compatible
   choice; accepting only `%%|` is the principled one and would be a behavior
   regression for B3-shaped documents.

6. **Where should this land?** I investigated on `main` in the primary checkout
   and committed only the plan + probes there. Say the word and I'll set up
   `cargo xtask create-worktree bd-mermaid-cell-options-9wo3crl0` for the
   implementation, or tell me which branch you want it on.

## Risks / tradeoffs (draft)

- **Blast radius of the `codeblock_shorthand` migration.** That desugar feeds
  every `#|` code cell in the tree — engine round-tripping
  (`crossref/roundtrip_tests.rs` depends on the Div-at-matching-depth invariant),
  crossref fixtures, and snapshots. The `cell_options` facility is the right
  destination, but the switch from line-level string surgery to YAML parsing is
  the largest single risk on this plan.
- **`passthrough` semantics.** `strip_consumed_lines` keeps unconsumed option
  lines *textually*, which the engine then re-parses. Moving to structured
  parsing means deciding how to re-emit them (or whether to, per question 4).
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
