---
title: '`.embed-example-iframe`: inline code snippet + Demo crossref for all examples'
date: 2026-06-10
description: 'Lets an `.embed-example-iframe` div carry a leading code snippet that renders above its iframe, gives every reveal.js example a numbered Demo crossref, and adds matching styling and docs migration.'
status: draft  # DESIGN — awaiting user review before implementation. No code yet.
braid:
  strand: bd-15uump3h  # discovered-from bd-z1smhvuo, the embed feature; related to bd-t3cert81, crossreferenceable Demo blocks
  priority: P2
---

**Page in focus:** `docs/presentations/revealjs/index.qmd`
**Feature code:** `crates/quarto-core/src/transforms/example_embed.rs`
**Writing skill:** apply `reader-expectations-prose` when (re)drafting doc prose.

## Overview

Two changes to how reveal.js docs present their runnable examples, plus the
small styling needed to make them look right.

1. **Crossreference every example.** Give every `.embed-example-iframe` div on
   the page a `#demo-…` id so each becomes a numbered, `@demo-…`-referenceable
   "Demo N" block. Today only `#demo-fragments` is numbered; the other seven
   are unnumbered embeds.

2. **Fold the code snippet into the div.** Change the `.embed-example-iframe`
   feature so the div may carry a **code snippet as its first child**, which
   renders as a code block **directly above** the iframe; the **second child**
   is the caption. The result reads as Q1's revealjs page does — *code block →
   iframe → caption* — but with Q2's added "Demo N" numbering and a real GitHub
   source link.

3. **Style it** to approximate Q1's `.slide-deck` look (bordered frame, sized
   block, captioned).

### Why this shape

Today the docs hand-author the illustrating snippet as a separate \`\`\`markdown
fence *before* the embed div, and the embed plan
(`2026-06-09-website-example-iframe-embed.md`, lines 25–27 & 293) explicitly
scoped the snippet *out* of the feature ("hand-authored… out of scope"). This
plan brings the snippet *into* the div so a single authored block owns the
whole example unit — snippet, frame, caption — and the three render together as
one cross-referenceable float. This mirrors Quarto's general crossref div
convention (the caption is a child element of the float div) and removes the
loose coupling between a prose code fence and the div beneath it.

### Current behavior (verified by reading the code + rendering the page)

`example_embed.rs` is a sugar→render pair:

- **Sugar** (`ExampleEmbedTransform`, normalization phase): matches
  `Div.embed-example-iframe`, validates `file=` (static-asset-only; `.qmd`
  rejected via Q-5-5, missing via Q-5-4), and emits a
  `CustomNode("ExampleEmbed")`. The **entire div body** goes into the `body`
  slot. If the div id is `demo-…` *and* `file=` is valid, it writes the crossref
  triple `{ref_type:"demo", kind:"Demo", identifier}` so `CrossrefIndexTransform`
  numbers it. `demo`/\"Demo\" is a built-in ref-type
  (`crossref/registry.rs:104`), distinct from the theorem-like `exm`/\"Example\".
- **Render** (`ExampleEmbedRenderTransform`, after `CrossrefRenderTransform`):
  builds a container `Div.embed-example` containing **(1)** the `<iframe>`
  RawBlock (page-relative `src` via `resolve_static_resource_href`, default
  `aspect-ratio: 16/9`, `height=` override) then **(2)** a `.embed-example-source`
  div holding the body as caption. When numbered, `with_number_label` prepends a
  `Demo\u{a0}N: ` span (`.embed-example-label`) to the caption's first
  paragraph/plain.

So the **order is fixed: iframe, then caption.** There is no notion of content
that should render *before* the iframe, and the whole body is treated as caption.

**No CSS exists** for `.embed-example`, `.embed-example-iframe`,
`.embed-example-source`, or `.embed-example-label` anywhere in the repo
(`resources/scss/**`, `docs/styles.css`). `docs/styles.css` is effectively empty.
Q1's analogous `.slide-deck` styling lives in quarto-web's own `styles.css`
(`border: 3px solid #dee2e6; width: 100%; height: 475px;`) — i.e. even in Q1
this is *site* CSS, not shipped by Quarto core.

## Design

### Authoring syntax (target)

`````markdown
::: {#demo-fragments .embed-example-iframe file="/examples/presentations/03-fragments/slides.html"}
```` markdown
## Reveal on click

::: {.fragment}
Appears on the first click.
:::
````

Fragments revealing content one step at a time.
[View source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
:::
`````

Renders to: **code block → iframe → "Demo N: Fragments revealing… View source"**.

Contract:
- **First child is a code block** ⇒ it is the *snippet*, lifted to render
  above the iframe. Detection is positional + typed: the first body block is a
  `Block::CodeBlock`. (At the sugar stage — normalization phase — a fenced
  ```` ```markdown ```` / ```` ```yaml ```` block is still a plain
  `Block::CodeBlock`; the copy-button scaffold is added later by the HTML
  writer, so detection is robust.)
- **The remaining children are the caption** (the "second element" the user
  described — in practice a single paragraph: a sentence + the source link).
- **No leading code block** ⇒ unchanged: the whole body is the caption (keeps
  any future non-snippet embeds working, and is the pre-migration behavior).
- **Code block but no caption** ⇒ code → iframe, no caption line (degrade
  cleanly; a numbered demo with no caption still gets its `Demo N` — decide
  during impl whether to synthesize a bare-label line, see Open Questions).

### Feature changes (`example_embed.rs`)

1. **Sugar** — split the body. Keep storing the caption in the `body` slot
   exactly as today, but additionally peel a **leading `CodeBlock`** into a new
   `snippet` slot (`Slot::Blocks` or `Slot::Block`). Everything else is
   unchanged (crossref triple, `file=` validation, diagnostics).
2. **Render** — when a `snippet` slot is present, push its block(s) into
   `content` **before** `iframe_block(...)`. The caption pipeline
   (`with_number_label` + `.embed-example-source` div) is unchanged and still
   consumes the `body` slot, so "Demo N:" still prepends to the caption.
3. **Container class** — keep `.embed-example`. Optionally add a marker class
   when a snippet is present (e.g. `.embed-example-has-snippet`) if CSS needs to
   distinguish; decide during styling.

This is the minimal change: one new slot, populated in sugar, consumed first in
render. Ordering of the three pieces lives entirely in `render_embed`.

### Styling (Q1 parity)

Add CSS approximating Q1's `.slide-deck`, scoped to the new structure:
- `.embed-example` — the unit container (optional light spacing/margins).
- `.embed-example iframe.embed-example-iframe` — `border: 3px solid #dee2e6;`
  (Q1's value), `width: 100%`, the existing inline aspect-ratio/height kept.
- `.embed-example-source` — muted, smaller caption line (mirror figcaption
  treatment); `.embed-example-label` — emphasized "Demo N" label.
- The snippet code block needs no special rule (Bootstrap/highlight styles it).

**Styling home — DECIDED (user, 2026-06-10): `docs/styles.css`** (docs-scoped),
matching Q1's own precedent that this is site CSS, not core-shipped. Q2 has no
general bundled component-SCSS layer for callouts/floats today
(`resources/scss/html/` only has highlight + title-block), so shipping a core
default stays a larger, separate decision for later.

### Docs migration (`docs/presentations/revealjs/index.qmd`)

For each of the 8 examples:
1. Add a `#demo-<slug>` id to the `.embed-example-iframe` div.
2. Move the section's canonical hand-authored code fence **into** the div as the
   first child (the snippet that best represents the deck).
3. Replace the bare `[Example: NN-…]` link body with a one-sentence caption +
   \+ a `[View source](…github…)` link (so the numbered output reads
   "Demo N: \<sentence>. View source", not "Demo N: Example: NN-…").
4. Optionally add `@demo-<slug>` cross-references in the prose where they read
   naturally (fragments already does this at line 136).

Proposed ids (kebab, matching the example dir): `demo-creating-slides`,
`demo-sections`, `demo-fragments` (exists), `demo-incremental-lists`,
`demo-columns`, `demo-speaker-notes`, `demo-asides`, `demo-footnotes`.

**Direction — one small teaching block per demo (user, 2026-06-10).** The
aspiration is **many, very small demos**: each `#demo-…` unit carries *exactly
one* teaching code block, ideally ~10 lines of markdown, and the matching
self-contained `slides.qmd` is that same snippet plus only the scaffolding
(`title`, `format: revealjs`, etc.). So the in-div snippet and the example
project should converge — the snippet *is* the deck's body.

For sections that today bundle several teaching fences (e.g. *Creating slides*:
Habits deck + `slide-level` yaml; *Fragments*: `.fragment` + `fragment-index`;
*Incremental lists*: `incremental: true` yaml + `.nonincremental`), prefer
**splitting them into separate small demos** over folding one and dropping the
rest. Where a fence teaches a pure config knob with no visual deck payoff (e.g.
`slide-level`, `reference-location: document`), it can stay as a prose-level
yaml fence without its own iframe.

**Scope question for Phase C (see Open Questions #5):** splitting into many
small demos may mean *new, finer-grained example projects* under
`examples/presentations/` (each needing a `manifest.yml` entry + re-staging),
which is a larger content effort than re-pointing the existing 8. Confirm
whether Phase C does the full split now or first migrates the existing 8 to the
inline-snippet form and tracks the finer split as follow-on.

## Plan (TDD-first)

### Phase A — Feature: code snippet as first child

Tests first (`example_embed.rs` unit tests, mirroring the existing ones):
- [x] **sugar peels a leading CodeBlock into a `snippet` slot**; `body` retains
  only the caption blocks.
- [x] **sugar with no leading CodeBlock**: `snippet` slot absent, `body`
  unchanged (regression guard).
- [x] **render emits snippet → iframe → caption in that order**.
- [x] **render numbered**: "Demo N:" prepends to the caption, not the snippet.
- [x] **render snippet + no caption, numbered**: keeps a visible "Demo N" label.
- [x] existing 9 tests still green (20/20 pass incl. crossref integration).

Implemented: sugar peels a leading `Block::CodeBlock` into a `snippet` slot;
render extends `content` with the snippet before `iframe_block`. ✅

### Phase B — Styling — BLOCKED by pre-existing Q2 bugs

CSS rules drafted (border `#dee2e6`, full-width frame, muted caption, bold
`Demo N` label). **Cannot be attached to the target page**: every Q2 mechanism
for project-wide CSS resolves the file **relative to each document's own
directory**, so a `_quarto.yml`-level entry works for root pages (home/about)
but is **silently dropped for the 150 subdirectory pages**, including
`presentations/revealjs/`:

- `css: styles.css` — read + emitted as `<link>`, but **never copied** to
  `_site/` for a website project. Filed **bd-r1y48cx0**.
- custom theme SCSS (`theme: [cosmo, embed-example.scss]`) — resolved against
  `ThemeContext.document_dir` (`quarto-sass/src/themes.rs:344,468`); compiled
  into the root-page bundle only. Filed **bd-oejuizi9**.
- `include-in-header` — also document-relative
  (`include_resolve.rs:38-42`). Same failure.

Root cause shared by bd-oejuizi9: project-wide config file paths should resolve
against the **project/config root** (as Quarto 1 does), not each document's
directory.

**RESOLVED (user, 2026-06-10): option 1 — ship as a built-in core SCSS layer.**
The docs-level routes are all blocked by the rule-1 gap (bd-oejuizi9/bd-r1y48cx0),
but a bundled core layer is compiled into *every* theme bundle independent of
any document/declaring directory, so it works on every page today — and a
built-in transform's default look belongs shipped with the feature (like
callouts/highlight). The systemic path-resolution fix is tracked as **option 2**
under bd-oejuizi9, with consolidated design at
`claude-notes/designs/path-resolution-model.md`.

- [x] `resources/scss/html/templates/embed-example.scss` (`scss:rules`):
  `.embed-example` margins, `iframe.embed-example-iframe` (3px `$gray-300`
  border, full width), `.embed-example-source` (muted), `.embed-example-label`
  (bold). Uses Bootstrap vars (`$gray-300`, `$text-muted`).
- [x] `load_embed_example_layer()` in `quarto-sass/src/bundle.rs`, included at
  all 5 assembly sites in `compile.rs` (native + WASM; theme / doc-vars /
  default-css paths) next to the highlight layer.
- [x] Re-captured the `phase5-single-doc-baseline` `styles.css` hash
  (bd-15uump3h comment); doc.html hash unchanged (fixture has no embed div).
  184 quarto-sass + 2322 quarto-core tests green.
- [x] Reverted the dead `docs/styles.css` rules + `theme: embed-example.scss`
  wiring (kept `docs/styles.css` at its original empty state).

### Phase C — Docs migration

- [ ] `01 creating-slides` → `#demo-creating-slides`; fold the Habits deck;
  keep the `slide-level` yaml in prose. Caption sentence.
- [ ] `02 sections` → `#demo-sections`; fold the sections+rule block. Caption.
- [ ] `03 fragments` → already `#demo-fragments`; fold the `.fragment` block
  (keep the `fragment-index` block in prose); refresh caption.
- [ ] `04 incremental-lists` → `#demo-incremental-lists`; fold the
  `.nonincremental` block (keep `incremental: true` yaml in prose). Caption.
- [ ] `05 columns` → `#demo-columns`; fold the `.columns` block. Caption.
- [ ] `06 speaker-notes` → `#demo-speaker-notes`; fold the `.notes` block.
  Caption. (Keep the "speaker view not yet" callout.)
- [ ] `07 asides` → `#demo-asides`; fold the `.aside` block. Caption.
- [ ] `08 footnotes` → `#demo-footnotes`; fold the per-slide footnotes block
  (keep the `reference-location: document` yaml in prose). Caption.
- [ ] Add `@demo-…` references in prose where natural.

### Phase D — End-to-end verification (per CLAUDE.md)

- [x] Re-staged example assets: `cargo xtask stage-doc-examples`.
- [x] `cargo run --bin q2 -- render docs/` (152/152; the 3 warnings are
  pre-existing, e.g. the `_brand.yml` markdown-parse warning — unrelated).
- [x] **Inspected the rendered HTML**: each unit emits, in order, a
  `<pre class="…code-with-copy">` snippet, an
  `<iframe class="embed-example-iframe" src="../../examples/…">`, and a
  `.embed-example-source` caption beginning "Demo N:". All 8 numbered
  Demo 1–8 with `#demo-…` ids; `@demo-fragments` resolves to "Demo 3". The
  revealjs page's theme bundle (`quarto-theme-51c4…`) contains
  `.embed-example iframe.embed-example-iframe{…border:3px solid #dee2e6}`.
- [x] **Browser check** (headless Chrome, isolated profile): screenshot shows
  code block → bordered live deck (HABITS / SECTIONS & BREAKS) → "Demo N:"
  caption + View source, matching the Q1 reading experience.
- [x] `quarto-sass` (184) + `quarto-core` (2322) green; full workspace running.
- [ ] `cargo xtask verify` (full, not `--skip-hub-build`) — required before push
  since `quarto-sass`/`quarto-core` feed `wasm-quarto-hub-client`. **Pending.**
- [ ] Preview parity tracked separately (bd-kjrpya2d /
  `2026-06-09-preview-embed-vfs-resolution.md`); the embed crossref already
  works in preview per that plan. Not re-verified here.

### Follow-ons filed
- bd-oejuizi9 (option 2): rule-1 path resolution for `_quarto.yml` theme/css/
  include. bd-r1y48cx0: `css:` never copied for website projects.
- Finer split into many small demos (deferred per user): **to be planned next.**

## Open questions

**Resolved (user, 2026-06-10):**
1. **Styling home** → `docs/styles.css` (docs-scoped).
3. **Number every example** → yes; all 8 get `#demo-…` ids and visible
   "Demo 1…8" captions.
   (Aspiration: many small demos, one \~10-line teaching block each — see the
   Direction note in Docs migration.)

**Remaining (decide during execution / Phase C kickoff):**
2. **Snippet + no caption, numbered**: keep a visible "Demo N" label line even
   with no caption text (recommended, for consistency) vs index-only.
4. **Snippet detection**: positional (first child is a `CodeBlock`) as proposed,
   vs an explicit opt-in marker. Recommend positional — matches "first element
   is the code cell" and Quarto's caption-as-child convention.
5. **Phase C breadth** → RESOLVED (user, 2026-06-10): **migrate the existing 8
   to the inline-snippet form now**; the finer split into many small demos
   (new `examples/presentations/` projects) becomes a **separate follow-on
   plan**, written after this migration lands.

## Out of scope

- Preview/VFS resolution of the iframe src (bd-kjrpya2d).
- The staging script's design (`cargo xtask stage-doc-examples` exists).
- New example projects or non-presentation categories.
- Dynamic (`.qmd`) iframe targets — still disallowed (Q-5-5).
