# Code-block features in Quarto 2

**Beads:** [bd-1tl09](../../.beads/issues.jsonl) — Code-block decorations epic.

**Status:** Draft, awaiting iteration with user.

## Overview

Quarto 1 (TS) decorates code blocks with a substantial set of features —
filename headers, copy buttons, folding, line numbers, code
annotations, and a side-by-side rendered-preview iframe. Quarto 2
currently has only syntax highlighting (`CodeHighlightStage`) and the
freshly-landed `<div class="sourceCode">` wrapper (commit `c81b6001`).
This plan brings the rest of Q1's code-block functionality to Q2 in a
way that makes the *format-agnostic* and *format-specific* halves of
the work cleanly separable, in the spirit of the existing Generate /
Render transform pairs (`TocGenerate` / `TocRender`,
`NavbarGenerate` / `NavbarRender`, `ListingGenerate` /
`ListingRender`, …).

We are explicitly **not** reimplementing Q1's Lua filters
file-for-file. Q1 grew its code-block pipeline organically across
several filters (`code-filename.lua`, `foldcode.lua`,
`line-numbers.lua`, `code-annotation.lua`, `customnodes/decoratedcodeblock.lua`,
plus TS post-DOM in `format-html.ts`). The Q2 port is an opportunity
to consolidate that surface area into one Generate transform that
extracts a typed `CodeBlockDecoration` from the AST and one Render
transform per format that consumes it.

## Q1 feature inventory (audit summary)

Source paths are all under
`external-sources/quarto-cli/src/resources/filters/` unless otherwise
noted.

| Feature | Trigger | Q1 source | HTML shape |
|---|---|---|---|
| Filename header | `#\| filename: "x.py"` or `{r filename="x.py"}` | `quarto-pre/code-filename.lua` + `customnodes/decoratedcodeblock.lua` | `<div class="code-with-filename"><div class="code-with-filename-file"><pre><strong>x.py</strong></pre></div><div class="sourceCode">…</div></div>` |
| Code copy button | doc/format metadata `code-copy: true \| hover \| false` | TS DOM pass in `src/format/html/format-html.ts:746-772` | `<div class="code-copy-outer-scaffold"><div class="sourceCode"><pre class="code-with-copy">…</pre></div><button class="code-copy-button">…</button></div>` + clipboard.js dep |
| Code folding | `#\| code-fold: true \| show` (+ `code-summary`) | `quarto-post/foldcode.lua` | `<details class="code-fold" [open]><summary>…</summary><div class="sourceCode">…</div></details>` |
| Line numbers | `#\| code-line-numbers: true \| "3,5-7"` | `quarto-pre/line-numbers.lua` | Adds Pandoc's `.number-lines` class; highlighter renders gutter |
| Code annotations | `# <1>` markers + following ordered list | `quarto-pre/code-annotation.lua` | `<pre class="code-annotation-code">…<span class="code-annotation-anchor">1</span>…</pre>` plus `<dl class="code-annotation-container">…</dl>` with `data-code-cell`/`data-code-cell-annotation`/`data-code-cell-lines` attrs |
| Code preview iframe | `{python code-preview="examples/foo.qmd"}` | TS DOM pass in `src/format/html/format-html.ts:775-791` | Appends `<iframe src="examples/foo.html">` next to the code block, copies code's classes onto it |
| Unifying wrapper | (used internally) | `customnodes/decoratedcodeblock.lua` | Composes filename + folding cleanly so the filename never gets stuck *inside* the `<details>` |

## Architecture for Q2

### Two transforms, one typed payload

Mirror the existing Generate/Render pairs:

1. **`CodeBlockGenerateTransform`** — format-agnostic.
   - Walks every `CodeBlock` (and possibly inline `Code` for annotations).
   - Reads attributes (`filename`, `code-fold`, `code-summary`,
     `code-line-numbers`, `code-copy`, `code-preview`, `code-annotations`)
     and the effective document metadata (e.g. `code-copy`,
     `code-annotations: hover|select|true|false`).
   - Parses chunk-option–style attrs (`code-line-numbers: "3,5-7"`)
     into a typed `CodeBlockDecoration` struct.
   - For annotations specifically: also scans the source text for
     `# <N>` markers and pairs them with the following ordered list.
   - Output: each decorated `CodeBlock` carries a structured
     `decoration: CodeBlockDecoration` (either via a new field on a
     dedicated CustomNode wrapping the code block, or a typed entry on
     `RenderContext` keyed by block id — TBD, see open questions).

2. **`CodeBlockRenderTransform`** (HTML for now).
   - Consumes the typed decoration produced above.
   - Emits the format-specific markup: filename header, copy-button
     scaffold, `<details>` wrapper, code-preview iframe, etc.
   - Has no parsing logic; all attribute interpretation already
     happened in Generate.

This makes future format support (LaTeX, Typst, ipynb) a matter of
implementing another Render transform — Generate doesn't change.

### Pipeline placement

Within `build_transform_pipeline` in
`crates/quarto-core/src/pipeline.rs`. The generate step belongs in the
Normalization Phase (after `MetadataNormalizeTransform`, so document-
level defaults are resolved). The render step belongs in the
Finalization Phase (alongside `CrossrefRenderTransform`,
`AttributionRenderTransform`).

`CodeHighlightStage` is independent and stays where it is — it
annotates code blocks with `data-hl-spans` and runs as a separate
pipeline stage (it does I/O for grammars, which transforms don't).
The Generate transform should not depend on highlight spans being
present; it reads source attributes only.

### Reuse vs. rewrite of the html.rs `<pre>` writer

The HTML writer currently emits the `<div class="sourceCode">`
wrapper in-place inside `Block::CodeBlock` (the change we just made).
The Render transform's job is to emit the *outer* decorations
(filename header, `<details>`, copy scaffold, preview iframe) by
producing `RawBlock("html", …)` or `Div`-with-classes structures
around the existing `CodeBlock`, so the html.rs writer continues to
handle the inner `<div class="sourceCode">…</div>` part unchanged.

This composability is important: folding *containing* a filename
*containing* the highlighted code is straightforward when each layer
emits its own wrapper.

## TDD-flavored phases

Each phase starts with a failing snapshot/unit test, then implements
the minimum to make it pass. Phases are sized so each could be a
single PR.

**Current iteration scope: Phases 0 – 3.** Phases 4, 5, and 6 are
deferred to separate sessions — each is substantially more involved
than the first three (line-numbers depends on coordination with
`quarto-highlight`, the iframe feature has a real design change from
Q1, and annotations are a multi-node transform that warrants its own
sub-plan). The deferred research below stays as-is so the next
session can pick it up without re-auditing Q1.

- [ ] **Phase 0 — Skeleton.** Add empty `CodeBlockGenerateTransform`
      and `CodeBlockRenderTransform` to `build_transform_pipeline`.
      Wire up a `CodeBlockDecoration` payload (empty struct to start).
      Test: pipeline still builds, no behavioral change.

- [ ] **Phase 1 — Filename header.** End-to-end slice for the
      `filename="x"` attribute. Smallest test: `{r filename="hi.R"}`
      → `<div class="code-with-filename"><div class="code-with-filename-file"><pre><strong>hi.R</strong></pre></div>…</div>`.
      Include the matching SCSS rule in `resources/scss/` (port from
      Q1's `_quarto-rules-code-filename.scss`). End-to-end verify
      with a browser screenshot, per CLAUDE.md.

- [ ] **Phase 2 — Code copy button.** Document-level `code-copy: true`
      triggers per-block copy button. Inject clipboard.js as an HTML
      dependency. Wrap each code block in
      `<div class="code-copy-outer-scaffold">…<button class="code-copy-button">…</button></div>`.
      Decide: hover-only vs always-visible (Q1 default is hover via
      `$code-copy-selector` SCSS variable).

- [ ] **Phase 3 — Code folding.** `code-fold: true|show`. Render
      `<details class="code-fold" [open]><summary>…</summary>…</details>`
      *outside* the filename wrapper (Q1's DecoratedCodeBlock
      composition pattern — see audit notes).

---

*Phases 4 – 6 are deferred to separate sessions. Notes below kept for
continuity; do not implement in the current iteration.*

- [ ] **Phase 4 — Line numbers.** *(Deferred.)* Annotate `CodeBlock`
      with Pandoc's `.number-lines` class. For Reveal.js/Docusaurus,
      stash the line range in a kv attribute (Q1 uses
      `kCodeLineNumbers`). Pandoc's syntax highlighter renders the
      gutter; ensure our highlighter matches. (May require coordinating
      with `quarto-highlight` crate.)

- [ ] **Phase 5 — Code preview iframe.** *(Deferred.)* In Q1 the
      attribute is `code-preview="examples/foo.qmd"` and a TS
      post-DOM pass auto-rewrites `.qmd` → `.html` before inserting
      the iframe. **Q2's design will diverge from Q1 here.** The
      attribute value will be a **relative URL pointing directly at
      an HTML file** (e.g. `code-preview="examples/foo.html"`) — no
      automatic `.qmd → .html` rewrite — and it may carry additional
      information beyond the bare URL (exact shape TBD; likely
      structured, perhaps `code-preview-src=…` paired with sibling kv
      attrs, or a parseable single-value form).

      The use case driving this change: in the Quarto 2 documentation
      website we want this feature to be much more pervasive than Q1
      uses it, and many references will point at pages **inside other
      Quarto 2 websites** — used to demonstrate website-level features
      that only render meaningfully against an entirely separate
      rendered site. When the target lives in a different project,
      the source `.qmd` path doesn't reliably map to a specific output
      `.html` path (it depends on the target site's `_quarto.yml`,
      not the source site's), so doing the rewrite automatically is
      unsafe in the general case. Requiring the author to write the
      output URL directly keeps the transform total and predictable.

      The phase work itself is the same shape as the others (Generate
      parses the attribute → typed `CodePreview`; Render emits the
      iframe), but no `.qmd → .html` filename surgery and the
      attribute schema is a fresh design decision rather than a port.

- [ ] **Phase 6 — Code annotations.** *(Deferred — largest single
      feature; warrants its own sub-plan.)* Port `code-annotation.lua`'s
      logic for: detecting `# <N>` markers (language-aware comment
      detection), pairing them with following ordered list, emitting
      annotation anchors + definition list.

Suggested ordering rationale: each phase is independently shippable
and demonstrable. Filename first because it's the simplest end-to-end
slice that exercises the full Generate → Render plumbing. Annotations
last because they're the only feature that couples *multiple* AST
nodes (the code block and the following list).

## Test fixtures

Build a single `tests/integration/code-block-features/` fixture
directory with one `.qmd` per feature plus a `combined.qmd` that
exercises the in-scope features together — for the current iteration
that's filename + copy + fold (Phases 1, 2, 3). The combined fixture
catches composition bugs (e.g., does the copy button work when the
block is folded?). Extend the combined fixture as later phases land.

Compare rendered HTML against snapshots. Visual parity with Q1 is the
acceptance bar — render the same fixture through Q1 and Q2 side by
side, screenshot both, and require human review for each phase.

## Resolved decisions

(Live decisions migrated out of "Open questions" as they get pinned
down — keep this section as the authoritative record so future
sessions don't re-litigate.)

- **(2026-05-19) Decoration storage shape: sideband map on
  `RenderContext`** (option (b) below). Cleared by the user with the
  rationale that Q1 used the CustomNode approach (option (a)) and it
  made handling **nested CustomNodes** problematic; Q2's pipeline
  isn't ready to tackle those interactions yet. Phase 1 will add a
  `HashMap<Key, CodeBlockDecoration>` field to `RenderContext`. The
  key type is itself an open sub-question — likely a small derived
  struct over `SourceInfo::Original`'s `(file_id, start, end)`, with
  a graceful skip for non-Original variants (rare for CodeBlocks).

## Open questions to resolve before Phase 0

These shape the typed-payload design, so resolving them up front
avoids rework.

1. **Where does `CodeBlockDecoration` live?** *Resolved 2026-05-19 —
   see "Resolved decisions" above.* The remaining open sub-question
   is the *key type* for the sideband map; recommendation is a
   `SourceInfoKey { file_id, start, end }` derived from
   `SourceInfo::Original` (which is the variant CodeBlocks land in
   essentially always — `Substring` / `Concat` are inline-text
   artefacts, `FilterProvenance` applies to filter-created blocks
   that don't exist in our pipeline yet).
   Options considered:
   - **(a)** A new `CodeBlockDecorated` CustomNode that wraps `CodeBlock`
     and carries the decoration as a typed field. Mirrors Q1's
     `DecoratedCodeBlock`. *Rejected per the resolved decision above
     — Q1's experience with nested CustomNodes was painful and we're
     not ready to take that on in Q2's pipeline.*
   - **(b)** ✅ A `HashMap<Key, CodeBlockDecoration>` on
     `RenderContext`, keyed by source location / block id. Mirrors
     how `resolved_listings` works today. Pros: keeps AST shape stable.
     Cons: id stability across transforms; lookup overhead.
   - **(c)** Stash a `data-codeblock-decoration` kv attribute on the
     CodeBlock carrying a serialized payload. Pros: trivial; works
     with existing walkers. Cons: stringly-typed, awkward for nested
     data like annotation line-range maps.

2. **Does inline `Code` get decorations too?**
   Q1's annotations infrastructure operates only on block-level
   `CodeBlock`; copy/filename/etc. similarly. Inline `Code`
   highlighting we already do. So: no, scope is `CodeBlock` only
   (and the OrderedList immediately following one for annotations).

3. **Document-default behavior:** `code-copy: true` at document level
   should turn on copy for *all* code blocks unless individually
   opted out. Where does this resolution happen — in Generate (so
   the decoration carries `copy: bool`), or in Render (Generate carries
   `copy_override: Option<bool>`, Render combines with the default)?
   Recommendation: resolve in Generate so Render is purely a
   semantic-to-syntax mapping. This means Generate needs to read
   `doc.ast.meta`.

4. **What does the in-AST decoration *look like* between Generate
   and Render?** Concretely:
   - if (a) above: the CustomNode replaces `CodeBlock` in the AST.
   - if (b): the AST is unchanged; data lives sideband. The user-filter
     slot question (bd-0fd0) applies — if a user filter mutates the
     code block between Generate and Render, the sideband data may
     reference stale state. Mitigation: re-run Generate after user
     filters, or document that decorations are post-user-filter.

5. **Iframe live-preview source path resolution.** Q1's
   `code-preview="examples/foo.qmd"` becomes an iframe pointing at
   `examples/foo.html`. We need to: (a) verify the target file is
   part of the same render (project) or rendered separately,
   (b) decide whether Q2 should warn when the target hasn't been
   rendered. Probably out of scope for the first slice — start with
   "user is responsible for rendering the target," document the
   `.qmd` → `.html` rewrite, defer cross-render-validation.

6. **Annotations and Lua-filter compatibility.** Q1 filters that
   consume annotation markup (e.g. `llms-code-annotations.lua`)
   won't exist in Q2. Do downstream Q2 features need access to
   annotation data, and if so, in what form? Likely defer until
   Phase 6.

## CSS porting

Q1's code-block SCSS is spread across:

- `src/resources/formats/html/_quarto-rules.scss` (general, annotations)
- `src/resources/formats/html/_quarto-rules-code-filename.scss` (filename)
- `src/resources/formats/html/_quarto-rules-copy-code.scss` (copy)
- `src/resources/formats/html/_quarto-variables-copy-code.scss` (copy vars)
- `src/resources/formats/html/bootstrap/_bootstrap-rules.scss:760-940`
  (code-block background, padding, etc.)

Port these incrementally per phase. Many already-shipped Q2
stylesheets contain the relevant rules (the rounded background
already worked once we emitted `div.sourceCode`); each phase needs to
audit whether its specific selectors are present.

## Out of scope

- **PDF / LaTeX rendering.** Q1's LaTeX path for code blocks is
  significant (Verbatim environments, `\circled` for annotations,
  `\begin{codelisting}` for filenames). Defer until a separate
  LaTeX-output plan.
- **Reveal.js–specific code-line-numbers ranges.** Q1 emits
  `data-line-numbers` for Reveal.js's incremental line highlighting.
  Add when we have a Reveal.js format in Q2.
- **Code annotations rendering quirks.** Hover-vs-select interaction
  modes, tooltip styling (`.code-annotation-tip-content`), Reveal.js
  variants — all deferred to Phase 6 sub-plan.

## Pointers

- Existing pipeline architecture: `crates/quarto-core/src/pipeline.rs:968-1116`
  (`build_transform_pipeline`).
- Existing Generate/Render pair to model after:
  `ListingGenerateTransform` / `ListingRenderTransform`, plus the
  surrounding comments in `pipeline.rs:1033-1075` about sideband data
  flow and user-filter ordering.
- HTML writer code-block emission:
  `crates/pampa/src/writers/html.rs:1273-1285` (`Block::CodeBlock`)
  and `crates/pampa/src/writers/html.rs:570-628`
  (`write_highlighted_codeblock`).
- Q1 audit details (with file:line references for each feature) are
  preserved in the conversation that produced this plan; ask
  whoever continues this work, or re-run the audit Agent prompt from
  the original transcript.
