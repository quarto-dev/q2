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

- [x] **Phase 0 — Skeleton.** Add empty `CodeBlockGenerateTransform`
      and `CodeBlockRenderTransform` to `build_transform_pipeline`.
      Wire up a `CodeBlockDecoration` payload (empty struct to start).
      Test: pipeline still builds, no behavioral change.
      *(Closed bd-ea5tl, commit `e673015c`.)*

- [x] **Phase 1 — Filename header.** End-to-end slice for the
      `filename="x"` attribute. Smallest test: `{r filename="hi.R"}`
      → `<div class="code-with-filename"><div class="code-with-filename-file"><pre><strong>hi.R</strong></pre></div>…</div>`.
      Include the matching SCSS rule in `resources/scss/` (port from
      Q1's `_quarto-rules-code-filename.scss`). End-to-end verify
      with a browser screenshot, per CLAUDE.md.
      *(Closed bd-j73yw, commits `6ca143d4` + `8b32c0aa` + `464b3874`.)*

- [x] **Phase 2 — Code copy button.** Document-level `code-copy: true`
      triggers per-block copy button. Inject clipboard.js as an HTML
      dependency. Wrap each code block in
      `<div class="code-copy-outer-scaffold">…<button class="code-copy-button">…</button></div>`.
      Decide: hover-only vs always-visible (Q1 default is hover via
      `$code-copy-selector` SCSS variable).
      *(Closed bd-j1trh, commits `f3974cf2` + `abc94e7d` + `0e85f954`.
      Hover default chosen at kickoff; mirror Q1. Both `code-with-copy`
      class and Bootstrap-Tooltip-driven "Copied!" feedback shipped.
      Browser e2e: hover shows icon, click swaps to checkmark + shows
      tooltip, state reverts after 1s. See Phase 3 hand-off below.)*

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

- **(2026-05-19) Phase 2 strategy** (cleared with the user at kickoff):
  - **Wrapper composition: single-pass cumulative wrap** inside
    `wrap_in_place`. Innermost is the original `CodeBlock` (with the
    `code-with-copy` class already added by Generate); filename Div
    wraps it next (Phase 1); copy scaffold Div is outermost. Each
    layer is opt-in based on the decoration. Extends naturally to
    Phase 3's `<details>` outermost.
  - **No per-block override of `code-copy`.** Mirror Q1: document-
    level metadata only. The Generate transform reads
    `ast.meta["code-copy"]` once at the top of the walk and applies
    the resolved `CopyMode` to every code block. Per-block override
    can be added later if a user asks for it.
  - **clipboard.js shipping:** new `ClipboardJsStage` modeled on
    `BootstrapJsStage`. Vendors `clipboard.min.js` under
    `resources/js/clipboard/`, gated on `!is_minimal_html(meta)`
    AND `meta.code-copy != false`. Stores two `js:` artifacts
    (`js:clipboard` for the vendored lib, `js:code-copy-init` for
    the small init handler). The init handler also depends on
    Bootstrap JS for the "Copied!" Tooltip popover, so the same
    minimal-HTML gate applies on both sides.
  - **Button accessibility:** Q2 adds `aria-label="Copy code"` in
    addition to Q1's `title=` attribute. Minor a11y improvement over
    Q1; mirrors the recommendation in Phase 2's hand-off section.
  - **Hover-vs-always default:** Q1's default of `hover`. Behavior
    is controlled by the SCSS variable `$code-copy-selector` (set to
    `'div.code-copy-outer-scaffold:hover > '` in hover mode, `''` in
    always mode), which means the markup never changes — only the
    selector that wraps the button-visibility rule does. Ported
    accordingly.

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

---

## Hand-off to next session — Phase 2 (copy button), bd-j1trh

This section is the kickoff packet for the session that picks up
Phase 2. Phases 0 and 1 landed across commits `e673015c` (skeleton),
`6ca143d4` (sideband infra), `8b32c0aa` (Generate → Render data
flow), `464b3874` (SCSS port + e2e). Phase 1 closed `bd-j73yw`;
Phase 2's beads issue is `bd-j1trh` (now unblocked).

### Load-bearing architectural facts (proven on Phase 1)

1. **Decoration storage**: sideband `HashMap<CodeBlockDecorationKey, CodeBlockDecoration>`
   on `RenderContext::code_block_decorations`. Key is
   `(file_id, start_offset, end_offset)` derived from
   `SourceInfo::Original`. Non-Original variants skip decoration
   (rare for `Block::CodeBlock` in practice). Both transforms run
   inside `AstTransformsStage`, so no `StageContext` bridge is needed
   — but if you add fields to `RenderContext` for new flow, check the
   bridging block at `crates/quarto-core/src/stage/stages/ast_transforms.rs:160-220`.

2. **Pipeline placement**:
   - Generate → Normalization Phase, right after `metadata-normalize`
     (so doc-level defaults are visible — see `pipeline.rs:986-994`).
   - Render → Finalization Phase, between `crossref-render` and
     `resource-collector` (see `pipeline.rs:1100-1108`).
   - Tests pin both positions:
     `html_pipeline_includes_code_block_decoration_transforms`
     and `q2_preview_pipeline_includes_code_block_decoration_transforms`
     (`pipeline.rs:2326-2380`).

3. **AST rewrite pattern**: Render uses
   `std::mem::replace(block, placeholder)` (placeholder is a tiny
   `RawBlock("html", "")`) to swap a `Block::CodeBlock` into a
   `Block::Div { class: "code-with-filename" }` wrapping the original
   block plus a filename-header `RawBlock`. See
   `crates/quarto-core/src/transforms/code_block_render.rs::wrap_in_place`.

4. **Native/React parity for free** — load-bearing insight: as long
   as Render emits standard Pandoc AST nodes (`Div`, `RawBlock`,
   etc.), the React renderer needs no changes. Phase 1 added the
   filename header without touching `ts-packages/preview-renderer/`.
   Phase 2 should keep this property. The one caveat: React's
   `RawBlock` wraps `dangerouslySetInnerHTML` in an extra `<div>`,
   which is invisible to descendant-style CSS selectors but matters
   for sibling / child selectors. If a Phase-2 SCSS rule uses `>`
   (child combinator) anywhere through a `RawBlock`, the preview
   will diverge.

5. **Composition order** (per Q1's `customnodes/decoratedcodeblock.lua`,
   confirmed by visual parity in Phase 1): filename header is the
   innermost wrapper, copy scaffold goes outside it, fold `<details>`
   is outermost. Phase 2's Render code needs to thread the new
   wrapper outside (or around) any filename wrapper already produced
   for the same block. Two strategies:
   - **Single pass with all wrappers in order**: build the full
     wrapper stack inside `wrap_in_place` based on the full
     decoration (filename + copy + fold all at once). Cleanest;
     refactor `wrap_in_place` to handle the cumulative case.
   - **Sequential wrappers**: each phase wraps independently. Order
     of execution determines nesting. Brittle; avoid.

### Phase 2 specifics — copy button

Q1 reference (from the audit in this plan): copy is implemented in
**TypeScript post-DOM** in Q1, not Lua. Source:
`external-sources/quarto-cli/src/format/html/format-html.ts:746-772`.
Q2 moves it to the Render transform, mirroring how Phase 1 moved the
Lua filename filter into the transform.

**Triggers** (decoration source-of-truth):
- Document/format metadata `code-copy: true | false | hover`.
- Default in Q1: `hover` (button visible on hover via SCSS
  `$code-copy-selector` set to `"div.code-copy-outer-scaffold:hover > "`).
- No per-block override in Q1 — copy is doc-level only.

**HTML output**:
```html
<div class="code-copy-outer-scaffold">
  <div class="sourceCode">
    ...<pre class="code-with-copy">...</pre>
  </div>
  <button class="code-copy-button" aria-label="Copy code">
    <i class="bi bi-clipboard"></i>
  </button>
</div>
```

Note: the `code-with-copy` class lands on the `<pre>` — that's a
class on the *existing* CodeBlock's attr, which means Generate
needs to mutate `attr.1` (the classes list) when the block is
covered by copy. The wrapper (`code-copy-outer-scaffold`) is added
by Render around the whole thing.

**Three commits suggested** (mirroring Phase 1's rhythm):

1. **Payload + doc-default resolver**. Add `copy: CopyMode` to
   `CodeBlockDecoration` with `CopyMode::{Off, Hover, Always}`.
   Add helper that reads `ast.meta`'s `code-copy` (a `String` or
   `Bool`) and resolves a default `CopyMode`. Wire into Generate.
   No Render changes yet. Tests: a Generate test asserting the
   default propagates, and one asserting per-block override (if Q2
   decides to support it — see open question below).

2. **Generate → Render data flow**. Generate emits the `code-with-copy`
   class onto the `<pre>`'s attr-classes when the decoration says copy
   is on. Render emits the `code-copy-outer-scaffold` wrapper + the
   button as a `RawBlock("html", …)`. Add the clipboard.js dependency
   via the HTML format-deps mechanism — see open question.

3. **SCSS + JS + e2e**. Port `_quarto-rules-copy-code.scss` and
   `_quarto-variables-copy-code.scss` from Q1. Add a small JS file
   under `resources/js/` that wires up the click handler (Q1
   inlines this in HTML; Q2 should keep it as a separate file
   loaded by all HTML renders). Browser e2e: hover triggers
   visible button, click copies to clipboard, "Copied!" tooltip
   appears.

### Open questions for Phase 2

- **Per-block override of `code-copy`?** Q1 supports doc-level only.
  Q2 could allow `{python code-copy=false}` on a per-block basis
  for trivial cost (Generate reads kvs first, falls back to doc
  default). Decision: punt unless there's a known user ask.

- **Hover vs always-visible default**. Q1 default: hover. Discoverability
  vs. visual noise. Recommendation: match Q1 (hover) and ship a
  `$code-copy-selector` SCSS variable so users / themes can opt
  into always-visible.

- **Clipboard.js dependency injection.** How does Q2 ship a JS file
  to the rendered HTML output? Look at how `bootstrap.bundle.min.js`
  is wired (see `crates/quarto-core/src/dependency.rs:26-80` and
  `crates/quarto-core/src/stage/stages/bootstrap_js.rs`). For
  clipboard.js, two paths:
  - Vendor it under `resources/js/clipboard.min.js` and ship it
    alongside Bootstrap's vendored bundle.
  - Load it lazily from a CDN. (Bad for offline / privacy. Skip.)

- **Copy button accessibility.** Q1 sets `aria-label="Copy code"` and
  swaps the icon on success. Mirror exactly; don't reinvent.

### First steps for the next session

1. Read this section.
2. `br show bd-j1trh` for the sub-task acceptance criteria.
3. Quick recon: read the three Phase 1 commit files end-to-end —
   `crates/quarto-core/src/transforms/code_block_generate.rs`,
   `crates/quarto-core/src/transforms/code_block_render.rs`,
   `resources/scss/bootstrap/_bootstrap-rules.scss` (lines around
   `.code-with-filename`).
4. Decide the wrapper-composition strategy (single-pass vs
   sequential) before writing any code — the choice shapes
   `wrap_in_place`. Recommendation: single-pass.
5. TDD: write the failing Generate test first, mirroring
   `generate_populates_filename_decoration`.

### Verification commands

- `cargo nextest run --workspace` — fast.
- `cargo xtask verify --skip-treesitter-tests --skip-treesitter-crlf-tests`
  — full check, skipping the pre-existing tree-sitter regression
  (see "Known blockers" below).
- For e2e: `target/debug/q2 render <fixture>` (rebuild q2 binary
  first via `cargo build --bin q2`) and
  `target/debug/q2 preview --no-browser --port 0 <fixture-in-project-dir>`.
  **For preview, the fixture must live inside a directory with a
  `_quarto.yml`** — single-file preview is broken per bd-tnm3k.

### Known blockers / context

- **bd-tnm3k**: `q2 preview <single-file.qmd>` is broken when no
  `_quarto.yml` ancestor exists. Workaround: place a minimal
  `_quarto.yml` next to the file. Affects e2e verification only;
  in-process tests and `q2 render` are unaffected.

- **Pre-existing tree-sitter regression** in
  `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/inline-multiline-attrs.txt`
  — 20 parses fail on `tree-sitter test`. Pre-existing on `main`,
  unrelated to bd-1tl09 work. Tracked under a separate beads ID
  (search beads for "inline-multiline-attrs"). Trips
  `cargo xtask verify` step 4/12 unless you pass
  `--skip-treesitter-tests --skip-treesitter-crlf-tests`.

- **Phase 5's "expected_hashes.txt" baseline**: every time SCSS
  changes, the `doc_files/styles.css` hash drifts. The file has an
  established commenting convention — read the existing comments
  before adding yours. Test:
  `crates/quarto-core/tests/artifact_scoping_pipeline.rs::single_doc_render_unchanged_under_scope_refactor`.

### After Phase 2

`bd-g1prx` (Phase 3, code folding) is the natural next item. It
adds the `<details>` wrapper outermost, exercising the composition
rule with both filename and copy already in place. Plan §"Phase 3 —
Code folding" has the design.

## Hand-off to next session — Phase 3 (code folding), bd-g1prx

This section is the kickoff packet for the session that picks up
Phase 3. Phases 0–2 landed across these commits:

- Phase 0: `e673015c` (skeleton, bd-ea5tl).
- Phase 1: `6ca143d4` + `8b32c0aa` + `464b3874` (sideband + filename
  + SCSS, bd-j73yw).
- Phase 2: `f3974cf2` + `abc94e7d` + `0e85f954` (CopyMode payload
  + scaffold + SCSS/JS/e2e, bd-j1trh).

### Load-bearing architectural facts (proven on Phases 1 and 2)

1. **Decoration storage**: same sideband map as before. Phase 3
   adds `fold: FoldMode` and `summary: Option<String>` fields to
   [`CodeBlockDecoration`](../../crates/quarto-core/src/transforms/code_block_generate.rs).
   `decoration_has_any_field` needs to be extended to include
   `fold.is_on()` (mirroring how Phase 2 extended it for `copy`).

2. **Single-pass cumulative wrap**: `wrap_in_place` in
   `transforms/code_block_render.rs` builds the wrapper stack
   innermost-to-outermost. Phase 2 added the `wrap_with_filename`
   and `wrap_with_copy_scaffold` helpers; Phase 3 adds
   `wrap_with_fold_details` as the **outermost** layer. The
   composition rule from `customnodes/decoratedcodeblock.lua`
   (Q1) and bd-g1prx's description is:

   ```text
   <details class="code-fold">              ← Phase 3 (outermost)
     <summary>…</summary>
     <div class="code-copy-outer-scaffold"> ← Phase 2
       <div class="code-with-filename">     ← Phase 1
         <div class="code-with-filename-file">…</div>
         <div class="sourceCode">           ← HTML writer's wrap
           <pre class="code-with-copy">…</pre>
         </div>
       </div>
       <button class="code-copy-button">…</button>
     </div>
   </details>
   ```

   Just add a `wrap_with_fold_details` call **after** the existing
   `wrap_with_copy_scaffold` block in `wrap_in_place`. The
   `inner = wrap_with_X(inner, …)` chaining shape extends naturally.

3. **Pipeline placement**: no new stages required. Both transforms
   already exist; Phase 3 only changes what they emit / read.
   `ClipboardJsStage` doesn't gate on fold — folding is purely
   visual / DOM-structural and ships no JS or library.

4. **Doc-default vs per-block**: Q1 supports BOTH for `code-fold`
   (doc-level `code-fold: true|show|false` + per-block override
   via `#| code-fold: …` chunk option). This is a small departure
   from Phase 2's "doc-level only" Q1 mirror. Generate reads
   per-block first, falls back to doc default. The chunk-option
   handling in Q2 currently goes through the engine layer; check
   `crates/pampa/src/...` for the existing `#| filename:` parsing
   to model the `#| code-fold:` parsing on.

5. **Native/React parity for free** — still holds in Phase 2.
   Render emits standard Pandoc AST nodes (`Div`, `RawBlock`), so
   the React renderer in `ts-packages/preview-renderer/` needs no
   changes. Phase 3 should keep this property. `<details>` is a
   semantic-HTML element — emitting it via a `RawBlock("html",…)`
   wrapper containing the inner Div as its `<summary>` sibling
   should work in both targets.

   **Caveat (still!):** React's `RawBlock` wraps
   `dangerouslySetInnerHTML` in an extra `<div>`. For Phase 3 the
   `<details>` element has structural semantics (the first
   `<summary>` child is special). If Render emits the `<details>`
   open tag as a RawBlock, the inner `<summary>` and content
   blocks, then a closing RawBlock — that would split the
   `<details>` across multiple RawBlocks, which would render
   wrong in React because of the extra wrapping `<div>`. Two
   safer approaches:

   - **A.** Emit the whole `<details>…</details>` as a single
     RawBlock containing serialized inner HTML. Trade-off: the
     inner content (potentially complex if there's a filename
     header + copy scaffold) has to be pre-serialized, which
     defeats the AST-level composition story.
   - **B.** Use a Pandoc `Div` with class `code-fold` plus a
     leading `RawBlock` carrying just `<details open><summary>…</summary>`
     and a trailing one with `</details>`. Tested in Phase 1 for
     the filename Div; should compose naturally with the existing
     wrap helpers.

   Recommendation: **A**, mirroring how `make_copy_button`
   serializes the button to a single RawBlock. Inner content is
   already deeply nested (filename + copy + sourceCode) and
   pre-serializing it requires running a sub-render — too much
   overhead. Re-evaluate if a hot fixture surfaces issues.

   **Final decision should be cleared with the user at kickoff.**
   This is the only non-trivial design choice in Phase 3.

### Phase 3 specifics — code folding

Q1 reference: `quarto-post/foldcode.lua` (the Lua filter that
implements this in Q1) plus `_quarto-rules.scss:280-282` for the
`.code-fold` SCSS. The Lua filter:

- Reads `code-fold` and `code-summary` from per-chunk options.
- Builds the `<details class="code-fold" [open]><summary>…</summary>…</details>`
  wrapper.
- Default summary: language-table `code-summary` value, English
  "Code".

**Triggers** (decoration source-of-truth):

- Per-block attribute / chunk option: `#| code-fold: true | show | false`,
  `#| code-summary: "Custom Label"`.
- Document-level fallback: `code-fold: true|show|false` (Q1
  honors this; Q2 should too).

**`FoldMode` enum**:

```rust
pub enum FoldMode {
    /// Don't wrap the block — Q1's `code-fold: false` (default).
    Off,
    /// Wrap, render collapsed by default — `code-fold: true`.
    Hide,
    /// Wrap, render expanded by default — `code-fold: show`.
    Show,
}
```

`FoldMode::is_on()` returns true for `Hide` and `Show`; both emit
the wrapper, differing only in the presence of the `open` attribute.

**Three commits suggested** (mirroring Phase 2's rhythm):

1. **Payload + doc-default + per-block resolver.** Add
   `fold: FoldMode` and `summary: Option<String>` to
   `CodeBlockDecoration`. Add `resolve_default_fold_mode(meta)`
   (mirrors `resolve_default_copy_mode`). Generate reads per-block
   `#| code-fold` first, falls back to doc default. `summary`
   takes per-block `#| code-summary` first, then doc-level, then
   None (the wrapper template handles the English-default fallback).
   Tests in `code_block_generate.rs` mirroring Phase 2's TDD pattern.

2. **Generate → Render data flow.** Render's `wrap_in_place`
   gains a `wrap_with_fold_details` helper as the outermost
   layer. The summary text needs HTML-escaping (same defense as
   the filename header). Tests in `code_block_render.rs`
   asserting the AST shape: outermost is a Div with class
   `code-fold` containing `[RawBlock(open-details-and-summary),
   inner-block, RawBlock(close-details)]` — OR a single RawBlock
   if option A above is chosen. Choose approach with the user
   first.

3. **SCSS + composition e2e.** Port `_quarto-rules.scss:280-282`
   (the `details > summary > p:only-child { display: inline }`
   rule and any other `.code-fold` rules) into
   `_bootstrap-rules.scss`. Browser e2e: click the disclosure
   triangle, confirm the inner content (including filename
   header) reveals/hides. Verify composition with both filename
   AND copy present.

### Open questions for Phase 3

- **`<details>` RawBlock structuring (single vs split)** — the
  React-parity caveat above. Clear with user first.
- **`code-summary` localization.** Q1 reads from the language
  table (`code-summary` key → "Code"). Q2 doesn't have a language
  table. Hardcode English for now, leave a TODO. Phase 2 made the
  same decision for "Copy to Clipboard" / "Copied!".
- **Doc-default support.** Phase 2 declared "no per-block
  override, doc-default only" to mirror Q1. Phase 3 needs both
  (Q1 supports both for code-fold). Slight Generate-side
  complexity bump; document precedence (per-block beats
  doc-default).
- **Interaction with `<details>` open state when user clicks the
  copy button inside.** Q1's copy button works inside an open
  `<details>` because clipboard.js binds globally. Q2 should
  inherit this for free since our handler does the same global
  bind. Verify in e2e.

### First steps for the next session

1. Read this section.
2. `br show bd-g1prx` for the sub-task acceptance criteria.
3. Quick recon:
   - `crates/quarto-core/src/transforms/code_block_generate.rs`
     (Phase 2's CopyMode payload + resolver as the model).
   - `crates/quarto-core/src/transforms/code_block_render.rs`
     (the single-pass cumulative wrap shape).
   - `external-sources/quarto-cli/src/resources/filters/quarto-post/foldcode.lua`
     (Q1 reference).
4. Decide the `<details>` RawBlock structuring with the user
   (single vs split — see "Open questions").
5. TDD: write failing Generate tests first, mirroring
   `generate_resolves_code_copy_*_to_*` from Phase 2.

### Verification commands

- `cargo nextest run --workspace` — fast.
- `cargo xtask verify --skip-treesitter-tests --skip-treesitter-crlf-tests`
  — full check.
- For e2e: rebuild q2 (`cargo build --bin q2`), render a fixture
  with `code-fold: true` + `filename="x.py"` + default Hover copy,
  open in Chrome via MCP devtools, click the disclosure triangle,
  hover code, click copy. **Don't forget to rebuild q2 after SCSS
  edits** — SCSS is embedded via `include_dir!` in
  `crates/quarto-sass/src/resources.rs` and only refreshes on
  rebuild.

### Known blockers / context (still relevant)

- **bd-tnm3k**: `q2 preview <single-file.qmd>` is broken when no
  `_quarto.yml` ancestor exists. Workaround: place a minimal
  `_quarto.yml` next to the file. Affects e2e verification only.

- **Pre-existing tree-sitter regression** in
  `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/inline-multiline-attrs.txt`
  — pre-existing on `main`, unrelated to bd-1tl09 work. Skip with
  `--skip-treesitter-tests --skip-treesitter-crlf-tests`.

- **Phase 5's `expected_hashes.txt` baseline**: every time SCSS
  changes or a new artifact is shipped, the relevant hash drifts.
  The file has an established commenting convention — read the
  existing comments before adding yours. Phase 2 added three
  comment blocks (one per commit); Phase 3 will likely add two
  (Commit 2 for the AST shape if it affects doc.html, Commit 3
  for the SCSS rules). Test:
  `crates/quarto-core/tests/artifact_scoping_pipeline.rs::single_doc_render_unchanged_under_scope_refactor`.

- **SCSS edits require rebuild**. `include_dir!` in
  `crates/quarto-sass/src/resources.rs` embeds the SCSS tree at
  compile time. After editing any `.scss` file, run
  `cargo build --bin q2` before manually rendering a fixture, or
  hashes won't update.

### After Phase 3

Phase 4 (line numbers, `bd-q...` — search beads for the issue ID) is
likely the next item but is marked deferred in the plan. The
website-projects epic also has pending work that may take priority.
Clear with the user.
