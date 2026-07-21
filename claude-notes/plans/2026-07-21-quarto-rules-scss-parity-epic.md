# Epic: Port TS Quarto `_quarto-rules.scss` to Q2 for HTML DOM parity

**Epic:** bd-4doe9lvt
**Audit task:** bd-eias3e39
**Discovered from:** bd-btjkyylx (title-block bottom-margin, PR #406)
**Date:** 2026-07-21

## Overview

TS Quarto's `src/resources/formats/html/_quarto-rules.scss` (774 lines,
~80 top-level selectors) is the HTML format's base rule layer. **Q2 has no
single counterpart layer.** Its rules were ported into Q2 piecemeal — into
`resources/scss/bootstrap/_bootstrap-rules.scss` and
`resources/scss/html/templates/title-block.scss` (and a few dedicated layers
like `copy-code.scss`, `highlight.scss`, `embed-example.scss`) — and the port
is **incomplete**. The title-block bottom-margin bug (bd-btjkyylx) was one
missing slice; that it slipped through is the signal that motivates this epic:
a rule-by-rule port leaves gaps, and we should systematically close them.

**Goal:** bring over the rest of `_quarto-rules.scss` to the extent Q2's
emitted DOM matches, and adapt the rules that make sense, without shipping dead
CSS for DOM Q2 doesn't produce.

## Guiding principle

A rule is only worth porting if Q2 emits the DOM it targets. The missing rules
split into two very different buckets:

- **Port-now** — Q2 already emits the DOM; the rule is a real, visible parity
  gap (like the title-block margin). Port it.
- **Blocked-on-emitter** — Q2 does not yet emit the DOM (e.g. Jupyter widget
  output, knitr SQL tables, layout panels). Porting the CSS now is dead code;
  the work is really a *feature* strand for the emitter, and the CSS follows.

So the epic's first deliverable is **not** a big CSS commit — it's a
**categorized inventory** (the audit). The audit promotes port-now rows to
themed child strands and files blocked rows with `blocks` deps on their emitter
features.

## Phase 1 — Audit (bd-eias3e39)

For each of the ~80 top-level selectors in `_quarto-rules.scss`:

- [x] Is the rule **already present** in Q2's SCSS? (grep
      `_bootstrap-rules.scss`, `title-block.scss`, `copy-code.scss`,
      `highlight.scss`, `embed-example.scss`, page-footer layer.)
- [x] Does Q2's HTML writer / transforms **emit the DOM** the selector targets?
      (grep `crates/pampa/src/writers/`, `crates/quarto-core/src/transforms/`;
      render a fixture exercising the feature and inspect the HTML.)
- [x] **Categorize:** `PORT-NOW` / `BLOCKED-ON-EMITTER` (name the emitter) /
      `ALREADY-PRESENT` / `INTENTIONALLY-DROPPED` (with reason).
- [x] Write the inventory table to
      `claude-notes/research/2026-07-21-quarto-rules-scss-inventory.md`.
- [x] File child strands under bd-4doe9lvt from the inventory (see Phase 2).
      Filed 2026-07-21: bd-u5yvsdgw (code), bd-dxgcpl02 (tables),
      bd-28iqotrt (misc), bd-ih6jrf39 (:root vars + print),
      bd-iq08mmnh (title-block), bd-18410csp (engine output),
      bd-sehm2rha (mermaid), bd-9fz5fweg (floats/layout CSS, blocked on
      bd-hcp8m3ve taxonomy feature), bd-q36vnfdp (backlog catalog);
      plus bug bd-obkvhlam (task-list rendering, discovered-from audit).

### Initial coverage scan (from the bd-btjkyylx session — NOT authoritative)

A quick token grep of `resources/scss/` for selector families. **Every row must
be re-verified by the audit** — "present" only means the token appears
somewhere, not that the rule matches TS Quarto, and "missing" doesn't say
whether the DOM is emitted.

| Selector family                | scss token present? |
| ------------------------------ | ------------------- |
| `quarto-layout-cell`           | present             |
| `quarto-float-caption`         | present             |
| `title-block-header` (base)    | **now complete** (bd-btjkyylx) |
| `code-copy-outer-scaffold`     | present             |
| `task-list`                    | present             |
| `tippy`                        | present             |
| `panel-input`                  | present             |
| `quarto-embedded-source-code`  | present             |
| `quarto-layout-panel`          | missing             |
| `quarto-figure`                | missing             |
| `code-overflow-wrap` / `-scroll` | missing           |
| `footnote-back`                | missing             |
| `quarto-cover-image`           | missing             |
| `quarto-unresolved-ref`        | missing             |
| `widget-subarea`               | missing             |
| `knitsql-table`                | missing             |
| `abstract-title`               | missing             |
| `quarto-float-tbl`             | missing             |

The MISSING set mixes port-now (Q2 emits the DOM) and blocked (Q2 doesn't yet)
— separating them is the whole point of the audit.

## Phase 2 execution — decisions + order (Carlos, 2026-07-21)

Green-lit to implement **all PORT-NOW strands** in one pass on integration
branch `feature/quarto-rules-scss-parity` (each sub-task branches off it,
`--no-ff` merged back per worktrees.md). The PN batch is the five themed
strands below; **engine-output (bd-18410csp) and mermaid (bd-sehm2rha) are
NOT in this batch** — the audit classes them as own-strand specials
(engine-output needs *executed* jupyter/knitr fixtures; mermaid is theming),
and bd-9fz5fweg (floats) stays blocked on the taxonomy feature bd-hcp8m3ve.

Two decisions taken before starting:

1. **Row 17 `.quarto-unresolved-ref` (Carlos): additive.** TS emits a
   `Span.quarto-unresolved-ref`; Q2 emits `Link.quarto-xref` with visible
   `?id?`. We keep Q2's louder `?id?` Link and *add* `quarto-unresolved-ref`
   to the class vec in `crossref_render.rs::render_resolved_ref` on the
   `!resolved` branch (so `<a class="quarto-xref quarto-unresolved-ref">`),
   then port the CSS. Downstream extensions key off the class (element-
   agnostic selector), so the additive form satisfies them while preserving
   Q2's better default. This row carries a pampa emitter test in addition to
   the compile-output assertion.
2. **Row 29 light/dark-content (Carlos): own strand.** Pulled out of
   bd-28iqotrt into **bd-l1rx9yzh** — the dark half is entangled with the
   not-yet-built dark-mode feature and its own difficulties.

**Execution order** (pure-CSS first to validate the mechanism, emitter-tweak
strand last; each shifts `styles.css` so the `phase5` hash is re-captured
per strand, sequentially):

- [x] 1. **bd-iq08mmnh** title-block remainder (row 6c) — DONE 2026-07-21,
      merged de2b9774. `#title-block-header a` + grouped `.author/.date/.doi`
      margins → title-block.scss; styles.css hash 6eafe1bf→0fc7bc97.
- [x] 2. **bd-dxgcpl02** tables base (row 7) — DONE 2026-07-21. All three
      selectors verified live (`table.table` margins; `tr.header>th>p` in
      multi-block header cells; `<caption>` for plain-caption tables, with
      `text-align:center` overriding bootstrap's `left`). `.table-caption`
      left BE. Ported into `_bootstrap-rules.scss`; styles.css hash
      0fc7bc97→0a0741ba.
- [x] 3. **bd-u5yvsdgw** code (rows 13a,b,23) — DONE 2026-07-21, merged
      9dd840c7. DOM verification revised three sibling rows: **13d** deferred
      (→ bd-8oyd9dg4, `$code-white-space` var is theming infra, no visible
      gap); **13e** reclassified **PN→BE** (Q2 emits no `<span><a>` line
      anchors); **13g** held for a design call (→ bd-bthmzyrc, Q2 already ships
      a divergent `code a:any-link` downlit rule). styles.css 0a0741ba→0c69b852.
- [x] 4. **bd-ih6jrf39** print (row 28) — DONE 2026-07-21, merged a4ac9b24.
      Ported `:root{font-size:11pt}`, `#quarto-sidebar,#TOC{display:none}`,
      `.page-columns .content` page-start, caption `#666`. `.fixed-top` skipped
      (no DOM). **Row 24** (`:root --quarto-*` vars) relocated to bd-18410csp
      (no consumer today → land with gt). styles.css 0c69b852→90c78796.
- [x] 5. **bd-28iqotrt** misc (rows 1a,10,11,15a,17,22,25) — DONE 2026-07-21,
      merged 9d532598. Ported `.hidden`, `iframe`, the `details` set,
      `.footnote-back`, `.quarto-unresolved-ref`, `a` underline-offset,
      `div.columns`/`div.column`; `.visually-hidden` skipped (bootstrap-
      provided). **Row 17 additive emitter**: crossref_render.rs now emits
      `class="quarto-xref quarto-unresolved-ref"` on unresolved refs. **Row 29**
      split to bd-l1rx9yzh. styles.css 90c78796→b06ec21d (doc.html unchanged).

### Outcome

All five PORT-NOW strands complete, plus the row-13g follow-up. DOM
verification revised the audit on several rows (documented in each commit +
inventory): **13d** deferred (bd-8oyd9dg4), **13e** reclassified BE,
`.fixed-top`/`.visually-hidden` found already-handled-or-BE, **row 24**
relocated to engine-output (bd-18410csp — record kept as a comment on that
strand). **Row 13g resolved** (Carlos, 2026-07-21): adopted Quarto 1's
`code a:any-link{text-decoration:none}` + `code a:hover{underline}`, replacing
Q2's gray-underline variant; merged (bd-bthmzyrc closed). Remaining epic
children are all blocked/own-strand specials (engine-output, mermaid, floats,
backlog) — the row-24 var work rides with engine-output.

Each follows the bd-btjkyylx template (PR #406): failing `css.contains(...)`
assertion in `crates/quarto-sass/src/compile.rs` `test_compile_default_css`
→ port into the thematically-right existing SCSS layer with a
`// ported from _quarto-rules.scss:<lines> (<strand>)` provenance comment →
re-capture `phase5-single-doc-baseline/expected_hashes.txt` with a dated
`# Re-captured` note → e2e `q2 render` grep of emitted `styles.css`.

## Phase 2 — Themed port-now strands (created by the audit)

Candidate groupings to become child strands of bd-4doe9lvt (final shape decided
by the audit):

- [ ] Figures / floats — `quarto-figure`, `quarto-figure > figure`,
      `quarto-float-caption-*`, `quarto-float-tbl`, `figure > figcaption`.
- [ ] Code overflow — `pre.code-overflow-wrap`, `pre.code-overflow-scroll`,
      `pre > code.sourceCode` line-anchor rules.
- [ ] Footnotes — `.footnote-back`, `.tippy-content .footnote-back`,
      tippy footnote-popup rules.
- [ ] Layout panels — `.quarto-layout-panel`, `.quarto-layout-row`,
      `.quarto-layout-valign-*` (likely blocked on the layout-panel emitter).
- [ ] Misc — `.quarto-cover-image`, `.quarto-unresolved-ref`,
      `details`/`summary`, `.visually-hidden`/`.hidden`/`.top-right` utilities.

Each port-now strand follows the bd-btjkyylx template: TDD (compile-output
assertion) → port the rule into the right existing layer → re-capture the
`phase5-single-doc-baseline` hash if `styles.css` shifts → end-to-end render
check.

## Phase 3 — Blocked-on-emitter strands (created by the audit)

For each selector whose DOM Q2 doesn't emit yet, file a strand describing the
emitter feature and add a `blocks` dep so the CSS work isn't picked up as ready
prematurely. Known likely-blocked: `widget-subarea` (Jupyter widgets),
`knitsql-table` (knitr SQL), possibly the layout-panel family.

## Notes

- The byte-identity baseline
  `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`
  has a strong convention of a `# Re-captured <date> (<strand>): …` comment per
  intentional CSS change — follow it for every port that shifts `styles.css`.
- Both `q2 render` and `q2 preview` consume the same `quarto-sass` bundle, so
  every port fixes both surfaces — but the preview needs a WASM rebuild +
  server restart to reflect the change (see bd-btjkyylx / CLAUDE.md
  "Verifying Rust changes in `q2 preview`").
