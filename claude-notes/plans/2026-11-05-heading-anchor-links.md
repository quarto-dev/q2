---
title: 'Website headings have no anchor links (bd-5kf2dnw4, GH #1058)'
date: 2026-11-05
description: 'Port Quarto 1''s hover anchor link on headings: emit the `anchored` class document-wide, honor `anchor-sections`, and ship the AnchorJS runtime plus its styling.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-5kf2dnw4
  priority: P3
---

**Branch:** `main` \@ `3fb2f41b7` (investigated in the main checkout; no worktree created)
**GitHub issue:** [#1058](https://github.com/quarto-dev/q2/issues/1058)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand bd-5kf2dnw4 (filed 2026-08-13) already describes exactly what GH #1058 reports, and its Q1 research is accurate against the current `external-sources/quarto-cli` tree. The user direction is "match Q1's behavior", which removes the biggest design axis; what remains is the runtime choice (vendor `anchor.min.js` vs. native) and a few edge-case questions below.

## Issue context

GH #1058 (filed against q2 0.40.0-nightly.20261104): headings in a rendered website have ids but no hover link icon, so readers cannot copy a section link. Q1 shows an AnchorJS `¶`-style link on hover. The reporter found it while porting the Posit Assistant docs (`posit-dev/product-doc-theme@q2`); the rendered `<h2>` carries no `anchored` class and the page has no anchor script.

No braid strand carried the GH issue number; bd-5kf2dnw4 is the matching strand and now references #1058.

## Dependency graph

- **discovered-from**: bd-v9zs83zj (closed) — added `anchored` to the five appendix headings (matches Q1's `headingClasses`, `format-html-appendix.ts:98,363`) and noted the class is inert until this lands. See `crates/quarto-core/src/transforms/appendix.rs:287-304`.
- **related**: bd-q36vnfdp (P4 catalog strand, parent bd-4doe9lvt) lists "anchorjs rules (L155–177)" of `_quarto-rules.scss` as CSS blocked on this emitter. This work should land that CSS and update the catalog row.
- **blocks**: none.

## Q1 behavior (the target)

Three parts, all in `external-sources/quarto-cli/src`:

1. **Emit** (`format/html/format-html.ts:828-848`). When `anchors` is on, add class `anchored` to every `h2`–`h6`, `.quarto-figure[id]` and `div[id^=tbl-]` inside `main` (Bootstrap) or `body` (otherwise). Skips `#toc-title` and anything with `.no-anchor`. `h1` is **not** included.
2. **Option** (`format-html.ts:314-317, 465-473`; schema `resources/schema/document-options.yml:66`). `anchor-sections: boolean`, default `true`, HTML formats only. Falls back to `false` for `minimal` (feature default `anchors: !minimal`, `:688`). `options.anchors` may also be a *string* (custom icon glyph); the template renders `anchors === true ? '' : anchors`.
3. **Consume** (`resources/formats/html/templates/quarto-html-after-body.ejs:73-82`). Loads vendored `anchor/anchor.min.js` (AnchorJS 5.0.0, 5,888 bytes, MIT), then in `DOMContentLoaded`: `new AnchorJS()`, `options = {placement: 'right', icon}`, `add('.anchored')`.
4. **Style** (`_quarto-rules.scss:147-177`). `.quarto-figure > .anchorjs-link` and `div[id^="tbl-"] > .anchorjs-link` absolutely positioned; `opacity: 1` on `:hover` of h2–h6 / figure / table div (workaround for AnchorJS's generic hover rule). `div[id^="tbl-"] {position: relative}`. There is no `.anchored` CSS rule; the class is purely a JS selector.
5. Secondary reader: `quarto-html-after-body.ejs:370` reads `a.anchorjs-link` inside footnote handling (hover footnotes) — only matters if/when that feature is ported.

## What the code looks like today

Confirmed by grep at `3fb2f41b7`:

- No `anchor-sections` key anywhere in q2 schema/crates (the only hits are unrelated strings in `quarto-toc.js`); no `"anchors"` option; no `anchor.min.js`; no `.anchorjs-link` CSS.
- Only `AppendixStructureTransform` emits `anchored` (appendix.rs:304).
- A directly comparable precedent exists for "vendored JS shipped conditionally": `crates/quarto-core/src/stage/stages/bootstrap_js.rs` (predicate → `js:*` artifact → `<script src>` via `ApplyTemplateStage`) and headroom (`pipeline.rs:2625`, `resources/js/headroom/`, `claude-notes/plans/2026-08-21-headroom-fixed-top-header.md`). Resource inventory lives in `resources/js/README.md` and `resources/vendored.toml`.
- Not reproduced by running a render: the reporter's HTML (`<section id="events"…><h2>Events</h2>`, no script) is consistent with the code reading, and `cargo xtask verify` was occupying the build lock. First task in Phase 0 is a failing test, which doubles as the repro.

## Constraints carried from the strand

- The document-wide pass must be an **AST transform**, not a DOM postprocessor (CLAUDE.md: no post-Pandoc DOM stage).
- Q1 relies on `classList.add` being idempotent. The appendix headings are `h2` and already carry `anchored`; the general transform must **skip headings that already have it** or we emit `class="anchored anchored quarto-appendix-heading"`.
- Ordering: run in Finalization *after* `AppendixStructureTransform`, or synthesized appendix headings are missed (`claude-notes/designs/transform-pipeline-phases.md`).
- Vendor `anchor.min.js` into `resources/` — never reference `external-sources/`.
- hub-client theming rule: any new CSS colors must be solid (the icon inherits link color; no new colors expected).

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD).** Failing tests: (a) website page with `##`…`######` and a `#`-level title gets `anchored` on h2–h6 only; (b) `anchor-sections: false` → no class, no script; (c) `.no-anchor` and `#toc-title` skipped; (d) appendix headings carry exactly one `anchored`; (e) tbl-/figure divs; (f) script present only when enabled; (g) `minimal: true` → off. Smoke-all fixture with a rendered-HTML assertion.
- **Phase 1 — Option plumbing.** Parse `anchor-sections` (bool | string) with HTML-format-only scope and the minimal default; add to schema.
- **Phase 2 — Emit `anchored`.** New Finalization transform after `AppendixStructureTransform`, scoped to the main-content region; idempotent.
- **Phase 3 — Runtime.** Vendor `anchor.min.js` (+ `vendored.toml`, `resources/js/README.md`), register a `js:` artifact stage following `bootstrap_js.rs`, and emit the small init snippet with the icon option.
- **Phase 4 — CSS.** Port `_quarto-rules.scss:147-177` rules for `.anchorjs-link` into the Bootstrap and non-Bootstrap bundles; update bd-q36vnfdp's catalog.
- **Phase 5 — Preview parity.** Decide whether `q2 preview` (hub-client iframe) gets the runtime; see question 3.
- **Phase 6 — Docs.** Document `anchor-sections` in the user docs and changelog.

## Open design questions for the user

1. **Runtime: vendor AnchorJS or implement natively?** Vendoring the 5.9 KB MIT file is byte-for-byte Q1 behavior (same DOM: `a.anchorjs-link` with `aria-label`, same `data-anchorjs-icon`, same CSS), and it matches the headroom/bootstrap precedent. A native hover link would drop a script but would have to re-derive AnchorJS's DOM and accessibility details. I recommend vendoring, since the goal is parity.
2. **Config surface: full `anchor-sections` (including the string-icon form) or just the boolean?** Q1 accepts a custom glyph via the same key. Recommend porting both; the string form is a few lines.
3. **`q2 preview`: include in this change?** The preview iframe re-renders its body from the AST without reloading, so AnchorJS would need re-running after each update (Q1 has no such concern). Options: ship render-only now and file a preview-parity strand, or handle preview here. Recommend render-only first.
4. **Scope of targets.** Port Q1's full selector list (h2–h6, `.quarto-figure[id]`, `div[id^=tbl-]`) in one go, or headings only for now? Figures/tables need the absolute-positioned CSS and q2's float/xref emitters to produce the same classes/ids — worth verifying per-target before claiming parity.
5. **Non-Bootstrap themes.** Q1 scopes the content container to `main` for Bootstrap and `body` otherwise. Is the non-Bootstrap (`theme: none`) path in scope, or do we only support the default Bootstrap case initially?

## Risks / tradeoffs (draft)

- Duplicate-class risk with the appendix transform is the main correctness trap (covered by Phase 0 test d).
- Adding a script to every HTML page affects snapshot/smoke tests broadly; expect churn in existing expected outputs that assert the script list.
- Heading ids that are duplicated (bd-cjf31ps7, bd-4qjl87ax) would produce anchors pointing at the first match; pre-existing, not introduced here.
- Hover-only affordance: no change for touch devices (same as Q1).
