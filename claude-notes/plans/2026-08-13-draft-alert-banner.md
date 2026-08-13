# Draft pages render without Q1's draft alert banner (bd-draft-banner-missing-hgx1gkqm)

**Date:** 2026-08-13
**Braid:** `bd-draft-banner-missing-hgx1gkqm` (feature, p3, labels: `navigation`, `parity`)
**Branch:** `main` @ `0dcd7e83` (investigated in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design, and considerably smaller than the strand describes** — the CSS
rule, the icon font, and the localization term all already ship; the `$if(draft)$`
template plumbing was empirically verified to work with no new wiring. The only
missing piece is emitting ~1 line of HTML, plus deciding where the localized
"Draft" string gets computed.

Four claims in the strand description are wrong or incomplete; see
**Corrections to the strand** below. They shrink the work rather than growing it,
but one of them (localization) *adds* a requirement the strand never mentions.

## Issue context

A page with `draft: true` in its front matter renders in q2 with no visual
indication that it is a draft. Quarto 1 emits, as the first child of the page
header:

```html
<div id="quarto-draft-alert" class="alert alert-warning"><i class="bi bi-pencil-square"></i>Draft</div>
```

Filed 2026-08-13 by Carlos Scheidegger; re-verified the same day against HEAD
`0dcd7e83`. Fresh, not stale — the one comment on the strand is itself a
line-number re-check against current HEAD.

Real-world impact: the Posit Connect docs port has four draft pages
(`admin/llm-gateway/index.qmd`, `admin/opentelemetry/llm-gateway.qmd`,
`admin/opentelemetry/content-telemetry.qmd`, `user/content-telemetry/index.qmd`).
All four carry the banner in the Q1 site and lose it under q2, so the ported site
publishes four unmarked draft pages.

## Dependency graph

Nearly empty — one outgoing `related` edge:

- **related → `bd-w0o9`** ("[websites] draft-mode include/visible/exclude
  option", open, p3, itself `discovered-from` `bd-9svl`). Q1 supports
  `draft-mode: visible|unlinked|none`; q2 has none of it and always excludes
  drafts from auto sidebars. **This is a sibling, not a blocker** — see the
  `draft-mode` analysis below, which argues the banner can land independently.

No `discovered-from` parent in this skein and no incoming `blocks`. The
originating context lives in a *different* skein (the connect-docs porting
skein, strand `br-draft-banner-missing-mo42fmdn`), which is why the graph here
looks bare. The strand description carries that context inline instead, so
nothing is lost — but note there is no incoming pressure from other q2 work; the
urgency is entirely from the Connect-docs port.

## What the code looks like today

All line references re-verified at `0dcd7e83`.

### Confirmed still accurate

- `DocumentProfile.draft` — field at `crates/quarto-core/src/document_profile.rs:407`,
  read from meta at `:746`.
- Its only two consumers: `transforms/sidebar_auto.rs` (:195/:204/:213, filtering
  drafts out of auto sidebars) and `project/aliases.rs:340` (skipping alias
  redirect stubs for drafts). Nothing in the HTML chrome reads it.
- `FULL_HTML_TEMPLATE` in `crates/quarto-core/src/template.rs` — body opens at
  `:194`, navbar slot at `:195`. The string `draft` does not appear in
  `template.rs` at all.
- `transforms/title_banner.rs:39` — documents that q2's navbar emits no
  `#quarto-header` wrapper, so Q1's exact DOM position is unavailable.
- **Symptom reproduces at HEAD.** `q2 render` in the repro →
  `_site/drafty.html` has no `quarto-draft-alert`; the committed
  `_site-q1/drafty.html` has it.

### Corrections to the strand

1. **The CSS already exists and already ships.** The strand says "there is no
   `#quarto-draft-alert` rule anywhere in the sass bundle (`crates/quarto-sass`)."
   It looked in the wrong place. The rule is at
   `resources/scss/bootstrap/_bootstrap-rules.scss:2604` — with both the
   `#quarto-draft-alert` block *and* the nested `i { margin-right: 0.3em }` —
   and `_bootstrap-rules.scss` is read into the bundle at
   `crates/quarto-sass/src/bundle.rs:136`. Verified end-to-end: after `q2 render`
   on the repro, `_site/site_libs/quarto/quarto-theme-701063255f83fe84.css`
   contains the rule. **No SCSS work is needed.**

2. **Bootstrap Icons already ships**, so `<i class="bi bi-pencil-square">` will
   render. `resources/bootstrap-icons/{bootstrap-icons.css,bootstrap-icons.woff}`
   exists and lands in `_site/site_libs/bootstrap/` on render. (Note q2 is
   inconsistent here — `template.rs:490` deliberately inlines an SVG *instead of*
   the `bi` font class for the author-email envelope, while
   `quarto-navigation/src/render_html.rs:293,303` uses `<i class="bi bi-arrow-*-short">`.
   Both patterns are live; this is design question 2.)

3. **The banner text is localized in Q1, and q2 already has the term.**
   The strand never mentions localization. Q1 uses
   `format.language.draft || "Draft"` (`format-html.ts:911`). q2 already has
   `draft: "Draft"` at `resources/language/_language.yml:130`, **fully translated**
   across the `_language-*.yml` set (`Borrador` es, `Brouillon` fr, `Rascunho` pt,
   …). Hardcoding "Draft" would ship a regression against a term file that already
   has the answer — the exact mistake the appendix-headings work (`60d42f0e`,
   bd-v9zs83zj) had to go back and fix for `section-title-*`.

4. **The `$if(draft)$` plumbing already works — verified, not assumed.** The
   strand asks to "verify that" front-matter keys reach the template context.
   They do. I temporarily inserted `$if(draft)$<div id="XXPROBE-DRAFTXX"></div>$endif$`
   at the top of `<body>` in `FULL_HTML_TEMPLATE`, rebuilt, and rendered the repro:
   the probe appeared in `drafty.html` (draft: true) and **not** in `index.html`.
   The probe has been reverted; the tree is clean. Mechanism:
   `add_metadata_to_context` (`template.rs:898`) → `metadata_entry_to_template_value`
   → `TemplateValue::Bool(true)` (`:1019`). **No new plumbing is required for the
   guard.** The localized *text* is a separate matter (design question 1).

### How Q1 actually gates the banner

Worth recording because it is not what the strand implies. Q1 does *not* read
`draft` at the HTML stage. `normalize/draft.lua` reads `draft` (and a `drafts`
param) plus `draft-mode`, and emits `<meta name="quarto:status" content="...">`:

| `draft` | `draft-mode` | `quarto:status` | Result |
| --- | --- | --- | --- |
| true | `gone` | `draft-remove` | blocks emptied, **no banner** |
| true | anything else (default `loose`) | `draft` | **banner** |
| false | — | absent | no banner |

`format-html.ts:902` then keys the banner off `quarto:status == "draft"`, and
inserts it as first child of `#quarto-header` (falling back to `document.body`
when there is no header — which is precisely q2's situation).

**Implication for `bd-w0o9`:** since q2 has no `draft-mode` at all, every q2 draft
is in the "anything else" row, so "banner whenever `draft: true`" is exactly
Q1-correct today. `bd-w0o9` does not block this; when `draft-mode: gone` lands it
will need to suppress the banner, which is a one-line condition wherever the guard
ends up. Worth a note in the code so `bd-w0o9`'s implementer finds it.

### Architectural note

Q1 does this in a **DOM postprocessor**. Per `CLAUDE.md` ("No DOM postprocessor —
port Quarto 1 DOM postprocessors as AST transforms"), q2 must not. And the banner
sits *outside* `#quarto-content`, where document AST blocks land — so an AST
transform cannot place it either. The template slot the strand suggests is the
right seam, and the probe confirms it reaches the right position:

```
<body class="nav-sidebar floating quarto-light">
<div id="XXPROBE-DRAFTXX"></div>      <-- probe landed here
<div id="quarto-content" ...>
```

This is the closest available equivalent to Q1's position (Q1: inside
`#quarto-header`, above the secondary nav).

### Precedent to follow

`toc_generate.rs:125-148` is the established shape for "localized string computed
from meta, consumed by chrome": read user override → localized term via
`LanguageTerms::from_meta(&ast.meta)` → English literal fallback (the literal
covering stage-less unit tests where `from_meta` returns `None`), then
`insert_path` the result into meta for the template. `appendix.rs:239-266`
(landed yesterday) follows the same chain. Either is a good model.

### Repro

Copied into `claude-notes/plans/draft-alert-banner-investigation/repro/`
(`_quarto.yml`, `index.qmd`, `drafty.qmd`) so it survives independently of the
external connect-docs checkout. `drafty.qmd` is `title: "Drafty"` + `draft: true`;
`_quarto.yml` is a website with `draft-mode: unlinked` (needed so Q1 renders the
page at all — q2 ignores the key). The Q1 reference output lives in the original
at `/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/draft-banner-missing/_site-q1/`.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD, failing first).**
  - `crates/quarto/tests/smoke-all/` fixture: `draft: true` page asserting
    `ensureHtmlElements: ["div#quarto-draft-alert.alert.alert-warning i.bi.bi-pencil-square"]`
    and `ensureFileRegexMatches` for the text.
  - Companion fixture: non-draft page asserting the banner is **absent**.
  - `smoke-all/localization/lang-es-draft-banner.qmd` asserting `Borrador`
    (mirrors `lang-es-appendix-headings.qmd` from `60d42f0e`).
  - Unit test wherever the localized-text precedence lands.
- **Phase 1 — Emit the banner.** Template slot at top of `<body>` in
  `FULL_HTML_TEMPLATE`, guarded on `draft`; localized text per design question 1.
  Comment pointing at `bd-w0o9` for the future `draft-mode: gone` suppression.
- **Phase 2 — End-to-end verification.** `q2 render` on the committed repro; diff
  the banner markup against `_site-q1/drafty.html`; confirm the CSS rule applies
  (read back computed styles in a browser, as `60d42f0e` did). Check `q2 preview`
  separately (design question 4).
- **Phase 3 — Docs**, if drafts are documented at all in `docs/`. Likely a
  one-liner; may be nothing.

No SCSS phase, no plumbing phase — both turned out to already exist.

## Open design questions for the user

1. **Where does the localized "Draft" string get computed?** Two candidates:
   - **(a) In `template.rs` where the context is built.** `add_metadata_to_context`
     (`:898`) is called from `:579` with `meta` in hand, and
     `LanguageTerms::from_meta(meta)` needs only `meta` — so the whole banner
     could be assembled right there with no new transform. Fewest moving parts.
   - **(b) A small transform inserting `draft-alert-text` (or the full rendered
     banner) into meta**, following `toc_generate.rs` / `appendix.rs`. Consistent
     with how every other localized chrome string is produced, and testable at the
     transform level, but adds a pipeline member for one string.

   I lean **(b)** for consistency with the two existing precedents, but (a) is
   genuinely simpler and the template already has everything it needs. Your call.

2. **Icon: `<i class="bi bi-pencil-square">` (Q1-identical) or an inlined SVG?**
   Both patterns exist in q2 (see correction 2). The `bi` font class is byte-identical
   to Q1 and the font ships; the inline-SVG precedent at `template.rs:490` was chosen
   deliberately for the author email. Default recommendation: **use `bi bi-pencil-square`**
   for exact Q1 parity, since this is a parity strand and the CSS rule
   (`#quarto-draft-alert i { margin-right: .3em }`) is written against an `<i>`.

3. **Should q2 also emit `<meta name="quarto:status" content="draft">`?** Q1 does,
   and it has two other consumers there (`website-llms.ts:71` excludes drafts from
   `llms.txt`; `website-draft.ts:15`). q2 has neither feature yet. Emitting the
   meta tag now is cheap and buys forward compatibility; skipping it keeps the
   change minimal. Recommendation: **skip it**, and let whoever ports `llms.txt`
   decide — but flag if you'd rather have it.

4. **Does this need to work in `q2 preview` too?** The preview path goes through
   the WASM build (`wasm-quarto-hub-client`). If it shares `FULL_HTML_TEMPLATE`,
   this is free; if the preview uses a different template, the banner would be
   missing exactly where it matters most (an author previewing a site is the
   banner's whole audience). **I did not verify this** — worth checking in Phase 2,
   and it may argue for option (a) or (b) in question 1 depending on which path
   the preview takes.

5. **Non-HTML formats?** Q1 gates on `isHtmlOutput() && !isHtmlSlideOutput()` — no
   banner in PDF, no banner in revealjs. Assume q2 matches (HTML only, not slides)
   unless you want otherwise.

## Risks / tradeoffs (draft)

- **Low risk overall.** Additive markup on a page class (`draft: true`) that today
  emits nothing distinguishing. No existing behavior changes.
- **Snapshot churn: likely zero**, since no fixture in the tree sets `draft: true`
  in an HTML-rendering context (the flag's only current consumers are sidebar
  filtering and alias stubs). Should be confirmed rather than assumed — the
  `60d42f0e` plan predicted churn and was wrong in the other direction.
- **The `bd-w0o9` seam.** Whichever site the guard lands in becomes the place
  `draft-mode: gone` must later suppress. Leave a comment naming the strand so it
  is not rediscovered from scratch.
- **Pre-flight note.** `cargo xtask verify --skip-hub-build` at `0dcd7e83` shows
  3 failing hub-client WASM smoke tests (`appendix/footnotes-heading.qmd` ×2,
  `localization/lang-es-appendix-headings.qmd`). These are the fixtures added by
  `60d42f0e` running against a **stale WASM image** — `--skip-hub-build` skips the
  rebuild. Not a regression and unrelated to this work, but it means a full
  `cargo xtask verify` (with the WASM leg) is the right gate for this strand's
  commit, not the `--skip-hub-build` shortcut.
