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

- [ ] Is the rule **already present** in Q2's SCSS? (grep
      `_bootstrap-rules.scss`, `title-block.scss`, `copy-code.scss`,
      `highlight.scss`, `embed-example.scss`, page-footer layer.)
- [ ] Does Q2's HTML writer / transforms **emit the DOM** the selector targets?
      (grep `crates/pampa/src/writers/`, `crates/quarto-core/src/transforms/`;
      render a fixture exercising the feature and inspect the HTML.)
- [ ] **Categorize:** `PORT-NOW` / `BLOCKED-ON-EMITTER` (name the emitter) /
      `ALREADY-PRESENT` / `INTENTIONALLY-DROPPED` (with reason).
- [ ] Write the inventory table to
      `claude-notes/research/2026-07-DD-quarto-rules-scss-inventory.md`.
- [ ] File child strands under bd-4doe9lvt from the inventory (see Phase 2).

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

## Code pointers for the audit (captured while warm, bd-btjkyylx session)

Where the relevant machinery lives — a fresh session can start here instead of
re-deriving:

- **SCSS bundle assembly:** `crates/quarto-sass/src/bundle.rs`.
  `load_quarto_layer()` builds the Bootstrap layer from
  `_bootstrap-rules.scss`; `load_title_block_layer()` loads
  `title-block.scss`. There is **no** `load_quarto_rules_layer()` — that's the
  gap. When the audit finds port-now rules that don't belong in either existing
  layer, decide whether to (a) add them to `_bootstrap-rules.scss`, (b) add to
  `title-block.scss`, or (c) introduce a new dedicated layer (as was done for
  `copy-code.scss` / `highlight.scss` / `embed-example.scss`).
- **Layer resources (embedded dirs):** `crates/quarto-sass/src/resources.rs` —
  `include_dir!` of `resources/scss/html/templates` (`TEMPLATES_DIR`) and the
  bootstrap dir. Target-agnostic, so edits reach both native and WASM.
- **Compile entry / theme pipeline:**
  `crates/quarto-core/src/stage/stages/compile_theme_css.rs`
  (`compile_theme_css` → `quarto_sass::compile_default_css`). The WASM preview
  path is `wasm-quarto-hub-client::compile_default_bootstrap_css` → same
  `compile_theme_css`.
- **DOM emission checks (the "does Q2 emit this?" question):** the HTML writer
  in `crates/pampa/src/writers/` and the AST transforms in
  `crates/quarto-core/src/transforms/`. Grep these for the class names a
  selector targets; if nothing emits the class, the rule is
  BLOCKED-ON-EMITTER.
- **TDD hook:** extend `test_compile_default_css` in
  `crates/quarto-sass/src/compile.rs` with a compile-output assertion per
  ported rule (the bd-btjkyylx pattern — assert on a minified substring unique
  to the new rule).
- **Baseline to re-capture on `styles.css` shifts:**
  `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`,
  checked by `single_doc_render_unchanged_under_scope_refactor` in
  `crates/quarto-core/tests/integration/artifact_scoping_pipeline.rs`. Copy the
  failing test's `left:` hash; add a `# Re-captured` note.
- **The TS Quarto source of truth:**
  `external-sources/quarto-cli/src/resources/formats/html/_quarto-rules.scss`
  (774 lines). Cross-reference `_quarto-rules-*.scss` siblings too (copy-code,
  code-filename) — some were already extracted into Q2 dedicated layers.

## Notes

- The byte-identity baseline
  `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`
  has a strong convention of a `# Re-captured <date> (<strand>): …` comment per
  intentional CSS change — follow it for every port that shifts `styles.css`.
- Both `q2 render` and `q2 preview` consume the same `quarto-sass` bundle, so
  every port fixes both surfaces — but the preview needs a WASM rebuild +
  server restart to reflect the change (see bd-btjkyylx / CLAUDE.md
  "Verifying Rust changes in `q2 preview`").
