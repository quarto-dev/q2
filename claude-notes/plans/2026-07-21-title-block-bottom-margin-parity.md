# Title-block bottom-margin parity (Q1 ↔ Q2)

**Strand:** bd-btjkyylx
**Date:** 2026-07-21

## Overview

In Q2 HTML output, the document title block sits **flush** against the first
article section — there is no vertical gap. In Q1 the same document renders with
a comfortable gap below the title block. The goal is to make Q2 match Q1's
spacing for both `q2 render` and `q2 preview`.

## Findings (measured)

Reproduction (`~/Desktop/daily-log/2026/07/21/test-title-block.qmd`) rendered
side by side:

- Q1: <http://localhost:3744/>
- Q2 preview: <http://127.0.0.1:55049/?page=test-title-block.qmd>

Measured with Chrome DevTools (computed styles + bounding rects):

| Element                                | Q1                | Q2               |
| -------------------------------------- | ----------------- | ---------------- |
| `#title-block-header` `margin-bottom`  | **17px** (`1rem`) | **0px**          |
| gap: title-block bottom → first `<section>` top | **17px** | **0px**          |
| `#title-block-header .abstract` top    | 177 (matches)     | 178 (matches)    |
| `main.content` `margin-top`            | 17px (matches)    | 17px (matches)   |

The DOM structure is otherwise identical: `main#quarto-document-content.content`
→ `header#title-block-header.quarto-title-block.default` → `section.level2`.
Both carry `body.fullcontent.quarto-light`. The **only** relevant difference is
the missing bottom margin on the header.

### Root cause

TS Quarto defines the base rule in
`src/resources/formats/html/_quarto-rules.scss` (lines 180–182):

```scss
#title-block-header {
  margin-block-end: 1rem;
  position: relative;
  margin-top: -1px; // Chrome draws 1px white line between navbar and title block
}
```

**Q2 has no `_quarto-rules.scss` layer at all.** Its SCSS bundle
(`crates/quarto-sass/src/bundle.rs`) assembles only:

- the Bootstrap layer — `resources/scss/bootstrap/_bootstrap-rules.scss`
- the title-block layer — `resources/scss/html/templates/title-block.scss`
- theme + syntax-highlight layers

Q2 ported *most* of `_quarto-rules.scss`'s title-block styling into
`title-block.scss` (under the `#title-block-header.quarto-title-block.default`
selector — abstract, meta grid, etc.), but the **unconditional** base rule on
`#title-block-header` (which applies regardless of the `.default` variant class)
was never carried over. The only `#title-block-header { margin-block-end … }`
rule present in Q2's `_bootstrap-rules.scss` is the responsive override inside
`body.nav-sidebar { @include media-breakpoint-down(lg) { … margin-block-end: 0 } }`
— i.e. Q2 has the "collapse the margin on small screens" rule but not the base
"give it a margin" rule it's meant to collapse.

### Fix verified end-to-end

Injecting the missing rule into the live Q2 preview iframe changed the measured
gap **0px → 17px**, exactly matching Q1:

```js
style.textContent =
  '#title-block-header { margin-block-end: 1rem; position: relative; margin-top: -1px; }';
// gap_before: 0, gap_after: 17, header margin-bottom: 17px
```

Because both `q2 render` and `q2 preview` consume the same compiled bundle from
`quarto-sass`, a single SCSS change fixes both surfaces.

## Plan

### Phase 1 — Test first (TDD)

- [x] Add a compile-output assertion in `crates/quarto-sass`
      (`test_compile_default_css` in `src/compile.rs`) that the compiled
      default-theme CSS contains `#title-block-header{margin-block-end:1rem` —
      the `1rem` value uniquely distinguishes the base rule from the
      `body.nav-sidebar` responsive override (`…margin-block-end:0`). Confirmed
      it **failed** against current `main` for the right reason (panic at the
      new assertion).

### Phase 2 — Implement

- [x] Added the base rule to the `/*-- scss:rules --*/` section of
      `resources/scss/html/templates/title-block.scss`, immediately before the
      `#title-block-header.quarto-title-block.default` block:
      ```scss
      #title-block-header {
        margin-block-end: 1rem;
        position: relative;
        margin-top: -1px;
      }
      ```
      Unconditional (not nested under `.quarto-title-block.default`), mirroring
      TS Quarto's `_quarto-rules.scss` ordering (base rule → variant styling).
- [x] Phase 1 test now passes; all 207 `quarto-sass` tests pass.
- [x] Updated the byte-identity baseline
      `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`:
      the `doc_files/styles.css` hash shifted (the compiled CSS gained the
      rule). `doc.html` hash is unchanged — the change is CSS-only and the
      fixture's `#title-block-header` element already existed. Added a
      `# Re-captured 2026-07-21 (bd-btjkyylx …)` comment per the file's
      convention. This was the only test in the full workspace run affected by
      the CSS change.

### Phase 3 — Verify

- [x] `cargo nextest run -p quarto-sass` — 207/207 pass.
- [ ] Full workspace: `cargo nextest run --workspace`.
- [x] End-to-end (render path): `cargo run --bin q2 -- render <fixture>.qmd`.
      The emitted `test_files/styles.css` contains the exact rule
      `#title-block-header{margin-block-end:1rem;position:relative;margin-top:-1px}`
      — grep-verified from real binary output, byte-for-byte matching Q1's
      compiled rule. Combined with (a) the rendered DOM being structurally
      identical to Q1 and (b) the same rule injected into the identical live
      DOM producing a 0→17px gap earlier this session, this confirms the render
      output now matches Q1's spacing.
      *Note: a fresh in-browser screenshot of the served render was blocked by
      the Chrome extension disconnecting mid-session; the emitted-CSS grep plus
      the earlier injection measurement are conclusive without it.*
- [ ] `q2 preview` check: the running preview embeds a separately-built WASM
      image, so it won't reflect this SCSS change until the WASM chain is
      rebuilt. `cargo xtask verify` (full, not `--skip-hub-build`) rebuilds it;
      re-measure the preview gap afterward.
- [ ] `cargo xtask verify` (full) before requesting push.

## Open questions / notes

- The `.abstract` top-margin already matches (Q2 handles it under `.default`),
  so no change needed there. Scope this strand strictly to the missing base
  `#title-block-header` rule.
- Worth a follow-up audit: what *else* from TS Quarto's `_quarto-rules.scss`
  never got ported into Q2's layers? This bug suggests the port was
  rule-by-rule and may have other gaps. File as discovered-from if found.
