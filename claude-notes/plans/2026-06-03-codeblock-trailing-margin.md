---
title: 'Fix stray empty line at the bottom of highlighted code blocks'
date: 2026-06-03
description: 'Removes a stray empty line at the bottom of highlighted code blocks by adding the margin rules that Quarto 1 gets from Pandoc''s baseline highlighting CSS.'
status: done  # done — implemented, verified e2e (render + preview), all phases complete
braid:
  strand: bd-jby1i
  priority: P1
---

## Overview

Highlighted code blocks (any language tree-sitter highlights, e.g. `ts`)
render with roughly one line-height of extra gray background below the
last line of code. This affects both `q2 render` HTML output and
`q2 preview` (both consume the same compiled theme CSS). Unhighlighted
blocks are unaffected because they don't get the `div.sourceCode`
wrapper.

### Repro

```
tmp-codeblock-repro/repro.qmd   (any ```ts block; see bd-jby1i)
cargo run --bin q2 -- render tmp-codeblock-repro/repro.qmd
```

Open the output in a browser: the gray box extends \~18px below `);`.

### Root cause (verified, not hypothesized)

Verified in Chrome via DevTools MCP on 2026-06-03:

- The emitted HTML is correct — `<pre>`/`<code>` content has **no**
  trailing newline. The stray space is pure CSS.
- Markup shape: `div.code-copy-outer-scaffold > div.sourceCode >
  pre.sourceCode > code.sourceCode`.
- The gray background is on `div.sourceCode`. It has
  `overflow-y: hidden` (`resources/scss/bootstrap/_bootstrap-rules.scss`
  \~line 1134), which creates a **block formatting context**.
- Bootstrap's reboot rule `pre { margin-bottom: 1rem }` (computed 17px)
  is never reset on `pre.sourceCode` in Q2. Inside the BFC, that
  bottom margin cannot collapse out — it is **trapped inside the div**,
  inflating the gray box by 17px ≈ one line-height.
- Measured: `div.bottom − pre.bottom` = **18px in Q2** (17px margin +
  1px border) vs **1px in Q1** (border only).

### Why Q1 doesn't have this bug

Q1 documents include Pandoc's baseline highlighting CSS
(`$highlighting-css$`), which contains:

```css
div.sourceCode { margin: 1em 0; }
pre.sourceCode { margin: 0; }
```

Q2 replaced Pandoc's highlighter with tree-sitter and ships its own
baseline in `resources/scss/html/templates/highlight.scss` — but that
file never picked up these two margin rules. (Same omission family as
the `pre > code { display: block }` rule already patched there.)

Q1 computed values, for parity reference (measured on a Q1 1.8 render
of the same document): `div.sourceCode` margin `17px 0`, `pre` margin
`0`, `gapInsideDiv` 1px.

## The fix

Add to `resources/scss/html/templates/highlight.scss` (rules layer),
next to the existing `pre > code { display: block }` baseline rule:

```scss
// Quarto 1 inherits these from Pandoc's baseline highlighting CSS
// ($highlighting-css$). Without `pre.sourceCode { margin: 0 }`,
// Bootstrap reboot's `pre { margin-bottom: 1rem }` is trapped inside
// the BFC created by `div.sourceCode { overflow-y: hidden }`
// (_bootstrap-rules.scss) and renders as a stray empty line at the
// bottom of every highlighted code block. The div takes over the
// outer spacing role.
div.sourceCode {
  margin: 1em 0;
}

pre.sourceCode {
  margin: 0;
}
```

Notes:

- Specificity is safe: `pre.sourceCode` (0,1,1) beats reboot's `pre`
  (0,0,1) regardless of layer order.
- Outer spacing is preserved: today the *visual* spacing after a block
  is the trapped 17px (rendered as gray background). After the fix,
  `div.sourceCode { margin: 1em 0 }` provides the same 17px *outside*
  the box, matching Q1. The margin collapses out through
  `.code-copy-outer-scaffold` (no border/padding/BFC on it), which is
  fine and matches Q1 behavior.
- Existing rules `.tab-pane div.sourceCode { margin-top: 0 }` and
  `.callout div.sourceCode { margin-left: initial }` already expect a
  div-level margin and continue to win by specificity.
- One fix location covers both pipelines: `highlight.scss` is embedded
  into `quarto-sass` (`bundle.rs::load_highlight_layer`) and compiled
  into the theme CSS used by native render **and** the WASM preview
  client.

## Work items

### Phase 1 — test first (TDD)

- [x] Add a regression test in
  `crates/quarto-sass/tests/integration/compile_all_themes_test.rs`
  (`test_compiled_css_resets_source_code_pre_margin`). Note: the
  test could NOT reuse the file's `compile_theme` helper —
  `assemble_with_theme` does not include the highlight layer; the
  render path passes it as an always-present user layer via
  `compile_default_css` / `compile_with_doc_vars`. The test mirrors
  that with `assemble_with_user_layers(&[load_highlight_layer()?])`.
- [x] Run it, verify it **fails** before the SCSS change. (Verified
  twice: once before writing the fix, and again by `git stash`-ing
  the SCSS change after the test was revised.)

  **Correction of an earlier misdiagnosis recorded in this session:**
  during the TDD loop the test kept failing after the SCSS edit, which
  was initially blamed on `include_dir!` not triggering rebuilds.
  False — `crates/quarto-sass/build.rs` emits
  `cargo:rerun-if-changed=../../resources/scss`, and an empirical
  check (`touch` a `.scss`, `cargo build -p quarto-sass`) confirms the
  crate recompiles. The *actual* cause was the real trap documented
  above: the test was still compiling via `assemble_with_theme`,
  which never includes the highlight layer, so no edit to
  `highlight.scss` could ever reach its output.

### Phase 2 — fix

- [x] Add the two rules to
  `resources/scss/html/templates/highlight.scss` with the explanatory
  comment above.
- [x] Run the new test, verify it passes.
- [x] `cargo nextest run --workspace` — 9543 passed. One expected
  fixture update: the Phase-5 byte-identity baseline
  (`crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`)
  re-captured for `doc_files/styles.css` (CSS legitimately changed);
  `doc.html` hash unchanged. Re-capture entry documented in the file
  per its convention. No `.snap` changes.

### Phase 3 — end-to-end verification (required before declaring done)

- [x] `cargo run --bin q2 -- render tmp-codeblock-repro/sidebyside.qmd`,
  measured in Chrome: `div.sourceCode` bottom − `pre` bottom = **1px**
  (border only; was 18px), `pre` margin `0px`, `div` margin `17px 0`.
  Gap above text 11.06px vs below 11.38px — symmetric. Matches Q1's
  measured values exactly. Screenshot of original repro confirms the
  stray line is gone.
- [x] Spacing between code block and adjacent paragraphs verified with
  `tmp-codeblock-repro/spacing.qmd`: 17px above and 17px below the
  gray box (outside it), did not collapse to zero.
- [x] Rebuilt the preview chain: `cargo xtask verify` (full, exit 0 —
  covers the WASM + q2-preview-spa builds) then `cargo build --bin q2`
  to re-embed dist/. Spot-checked `q2 preview
  tmp-codeblock-repro/repro.qmd` in Chrome: same measurements inside
  the preview iframe (`gapInsideDiv` 1px, `pre` margin 0, `div` margin
  17px 0); screenshot shows no stray line.
- [x] Record the invocation + observed measurements in the session
  transcript / this plan. (This checklist is the record.)

### Phase 4 — wrap up

- [x] `cargo xtask verify` (full — `quarto-sass` feeds the WASM
  client, so the hub-build leg is in scope). All 12 steps passed.
- [x] Close bd-jby1i; `br sync --flush-only`; commit (including
  `.beads/`).
- [x] Remove `tmp-codeblock-repro/` scratch directory.

## Out of scope (noted, not fixed here)

- Unhighlighted code blocks get no `div.sourceCode` wrapper and no
  gray background box at all in Q2, while Q1 gives all code blocks the
  background treatment. That is a separate parity question — file a
  separate beads issue if we want Q1 parity there.
- Pandoc's baseline also ships line-numbering (`pre.numberSource`) and
  print-media rules Q2 doesn't have; same story — separate issue if
  needed.
