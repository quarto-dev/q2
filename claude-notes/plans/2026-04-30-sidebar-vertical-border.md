# Sidebar vertical border (Q1 parity)

## Overview

Quarto 1 renders a faint vertical line between a docked sidebar and
the main content. Quarto 2 does not. The example
`examples/websites/03-nested-sidebar` demonstrates the gap: the
`q1-site/` output has the line; the Q2 `_site/` output does not.

This plan ports the Q1 behavior plus the customization story behind
it. The fix is small in CSS, but it forces us to introduce a
mechanism Q2 doesn't have yet — a place where document/project
metadata can inject SCSS variables ahead of the framework defaults.
Several future tickets (sidebar bg/fg, navbar bg/fg, footer bg/fg,
brand colors, …) need the same hook, so this work pays for itself.

Related issues / context:

- Website epic: `bd-0tr6`.
- No prior work on this in the tree (verified: no `border` field on
  `Sidebar`, no `@if $sidebar-border` block in `_bootstrap-rules.scss`,
  no doc-derived variables seam in `CompileThemeCssStage`).

## How Q1 does it

### The CSS rule

Q1's emitted Bootstrap CSS contains:

```css
.sidebar.sidebar-navigation:not(.rollup) {
  border-right: 1px solid #dee2e6 !important;
}
```

Verified in
`examples/websites/03-nested-sidebar/q1-site/site_libs/bootstrap/bootstrap-cf5ee1d16def7895729d1ac812351ea4.min.css`.

### The SCSS source

The rule lives in
`external-sources/quarto-cli/src/resources/projects/website/navigation/quarto-nav.scss:552-556`:

```scss
@if $sidebar-border {
  .sidebar.sidebar-navigation:not(.rollup) {
    border-right: 1px solid $table-border-color !important;
  }
}
```

Two variables drive the result:

1. **`$sidebar-border`** — boolean gate. Default `false` (in
   `_bootstrap-variables.scss:173`).
2. **`$table-border-color`** — the line color. Bootstrap's default
   `#dee2e6` (i.e. `$gray-300`). Any Bootswatch or custom theme that
   overrides `$table-border-color` automatically retints the line —
   no special handling needed for theme parity.

### The default override

`$sidebar-border: false !default;` is the framework default.
`format-html-scss.ts:631-642` overrides it per-document:

```ts
const sidebarBorder = sidebar[kBorder];
variables.push(
  outputVariable(
    sassVariable(
      "sidebar-border",
      sidebarBorder !== undefined
        ? sidebarBorder
        : sidebar.style === "docked",
    ),
  ),
);
```

Translation:

- If user wrote `sidebar.border: true|false` in YAML, use that.
- Otherwise: `true` for `style: docked`, `false` for `style: floating`.
- The synthesized `$sidebar-border: <bool>;` snippet is fed in
  *before* `_bootstrap-variables.scss`'s `!default` line, so it wins
  the `!default` race.

So in `03-nested-sidebar`, both sidebars are `style: docked` and
neither sets `border:`, which yields `$sidebar-border: true` and the
rule fires.

## Q2 status (what's missing)

1. **Rule absent.** The `@if $sidebar-border` block was never ported
   to `resources/scss/bootstrap/_bootstrap-rules.scss`. The variable
   declaration is present (`_bootstrap-variables.scss:173`) but
   nothing consumes it.
2. **No doc-derived SCSS variables seam.** `CompileThemeCssStage`
   only reads `theme:` configuration from `doc.ast.meta`; it never
   threads website/sidebar config into the SCSS bundle. There is no
   place today where Q2 can emit "this document needs
   `$sidebar-border: true`".
3. **`Sidebar` struct lacks `border`.**
   `crates/quarto-navigation/src/sidebar.rs:339` has `style`,
   `background`, etc. but no `border: Option<bool>` field.

Verified absence of the rule in compiled output:
`examples/websites/03-nested-sidebar/_site/site_libs/quarto/quarto-theme-345008c71cc05875.css`
contains zero `border-right` rules on `.sidebar`.

## Plan

TDD per CLAUDE.md: each phase writes the test first, watches it
fail, then implements.

### Phase 1 — port the SCSS rule ✅

- [x] **Test (compile-level):** added two tests in
  `crates/quarto-sass/src/compile.rs`:
  - `test_sidebar_border_rule_emits_when_variable_is_true` — feeds a
    `$sidebar-border: true;` user layer (no `!default`) into
    `assemble_with_user_layers`, compiles via
    `compile_scss_with_embedded`, and asserts the emitted CSS
    contains `.sidebar.sidebar-navigation:not(.rollup)` with a
    `border-right: 1px solid …` and `!important`. Confirmed to fail
    before the rule was added.
  - `test_sidebar_border_rule_absent_when_variable_is_false` — guards
    the off path so we don't accidentally hardcode the rule outside
    the `@if`.
- [x] Added the `@if $sidebar-border { … }` block to
  `resources/scss/bootstrap/_bootstrap-rules.scss` directly after
  the `body.docked` block (sits with its visual neighbors). Color
  uses `$table-border-color` so Bootswatch / custom themes that
  retint table borders retint this separator automatically.
- [x] Both tests pass; full `cargo nextest run -p quarto-sass` (145
  tests) passes — no regressions.

### Phase 2 — doc-derived SCSS variables seam ✅

- [x] **Tests added in `crates/quarto-core/src/stage/stages/compile_theme_css.rs`:**
  - Unit tests for `derive_doc_scss_layer`:
    `doc_scss_layer_empty_meta_is_empty`,
    `doc_scss_layer_docked_sidebar_emits_border_true`,
    `doc_scss_layer_floating_sidebar_emits_border_false`. Last two
    also check the assignment is unconditional (no `!default`).
  - Stage-level: `stage_emits_sidebar_border_rule_for_docked_sidebar`
    (failed before wiring, passes after) and the off-path guard
    `stage_does_not_emit_sidebar_border_rule_for_plain_doc`.
  - Cache-correctness: `stage_distinguishes_docked_vs_floating_in_cache`
    proves two docs whose only difference is sidebar style get
    distinct cache entries — fails without the cache-key change,
    passes with it.
- [x] `derive_doc_scss_layer(meta: &ConfigValue) -> SassLayer` reads
  `meta["website"]["sidebar"]`, parses with
  `Sidebar::parse_list_from_config`, and emits
  `$sidebar-border: <bool>;` (no `!default`) for the **first**
  sidebar — `(style == Docked)` for now (Phase 3 plumbs the
  explicit `border:` knob).
- [x] Added `compile_with_doc_vars(config, context, doc_vars)` to
  `quarto-sass` (native + WASM symmetric). The doc-vars layer is
  pushed as the **last** user layer so `merge_layers()` promotes
  it to the front of the merged-defaults section, winning the
  `!default` race. When `doc_vars.is_empty()`, the function
  delegates to `compile_default_css` / `compile_theme_css` so the
  fast `OnceLock`-backed default path is preserved for plain docs.
- [x] `CompileThemeCssStage::run` refactor: build `doc_vars` once,
  branch on `(has_themes, has_doc_vars)`. The fast default path
  (no themes, no doc-vars) keeps its fixed `default_minified` /
  `default_expanded` key; the themed and/or doc-vars-bearing path
  routes through the fingerprinted `cache_key()` (now extended to
  hash `doc_vars.defaults`). Cache + compile flow goes through the
  new `compile_with_doc_vars_via_runtime` wrapper that abstracts
  native vs. WASM. Removed the now-unused `compile_scss` helper.
- [x] All 6 new tests pass. Full `cargo nextest run -p quarto-core`
  (1465 tests) and `cargo nextest run -p quarto-sass` (145 tests)
  pass — no regressions.

**Code-quality flag (raised by user during Phase 2, deferred):** the
hash inputs for `cache_key` are now hand-assembled — `SCSS_RESOURCES_HASH`,
each `ThemeSpec` identity, custom-theme contents, doc-vars defaults,
minified flag. As more inputs accrete (sidebar bg/fg, navbar bg/fg,
brand colors, …) this approach becomes increasingly easy to break
silently — forgetting to add a new input to the key produces stale
cache hits, and the type system doesn't catch it. A more structured
shape would force the inputs to flow through one canonical location.
Two approaches worth considering:

1. **Single `CompileInputs` value type.** Group everything that affects
   the compile (theme_config, doc_vars, minified, plus any future
   inputs) into a struct, derive `Hash`/`Serialize`, and have
   `cache_key` take `&CompileInputs`. The `compile_with_doc_vars`
   call site takes the same struct. Adding a new input means adding
   a field — both the cache key AND the compile see it automatically,
   and tests for "key changes when input X changes" become trivial.
2. **Hash the assembled SCSS string.** Whatever inputs we add, the
   compile produces a single SCSS bundle string. Hashing that string
   directly (post-assembly, pre-compile) is automatically complete
   — but is more expensive (assembles SCSS even on cache miss path
   pre-compute), and assemble-failure paths get awkward.

Recommend (1) as a follow-up after Phase 4. Filed as a follow-up
issue.

### Phase 3 — Q1 parity for the `border:` knob ✅

- [x] Tests added in `crates/quarto-navigation/src/sidebar.rs`:
  `parse_sidebar_border_absent_is_none`, `parse_sidebar_border_true`,
  `parse_sidebar_border_false`, `sidebar_border_round_trip`,
  `sidebar_border_round_trip_omitted_when_none`.
- [x] Stage-level tests added: `doc_scss_layer_explicit_border_true_overrides_floating_default`,
  `doc_scss_layer_explicit_border_false_overrides_docked_default`,
  and end-to-end `stage_omits_sidebar_border_rule_when_docked_overrides_to_false`.
- [x] Added `pub border: Option<bool>` to `Sidebar`. Parsed in
  `from_config_value` via `cv.get("border").and_then(as_bool)`.
  Round-trip serialized in `to_config_value` via `bool_entry`,
  omitted when `None` (so `None` doesn't round-trip to `false`).
- [x] `derive_doc_scss_layer` now uses
  `first.border.unwrap_or_else(|| first.style == Docked)` for the
  Q1-parity precedence.
- [x] Full quarto-navigation (128) + quarto-core (1465) + quarto-sass
  (145) tests = 1741 pass with no regressions.

### Phase 4.5 — appearance polish (post-review feedback) ✅

User noticed two visual issues on the re-rendered `03-nested-sidebar`:

1. Toggle dongles touched the right border in Q2; Q1 had visible
   breathing room.
2. Q2's border stopped partway down on short pages; Q1 extended it
   all the way to the viewport bottom.

Both were caused by missing Q1 rules. Tests + ports:

- [x] `test_quarto_sidebar_children_have_right_padding` and
  `test_quarto_container_min_height_fills_viewport` added in
  `crates/quarto-sass/src/compile.rs`. Both fail before the ports.
- [x] Ported `#quarto-sidebar > * { padding-right: 1em }` from
  `quarto-cli/.../quarto-nav.scss:623-628` into
  `_bootstrap-rules.scss`. Toggle dongles now sit 1em away from
  the right border.
- [x] Ported `.quarto-container { min-height: calc(100vh - 132px) }`
  from `quarto-cli/.../quarto-nav.scss:53-55` into
  `_bootstrap-rules.scss`. The 132px constant matches Q1's
  navbar+footer composite (~64px navbar + 68px footer/margin).
- [x] Re-captured `phase5-single-doc-baseline` styles.css hash
  with a comment documenting why (the new rules don't match
  anything in a single-doc body so doc.html is unchanged).
- [x] Re-rendered `examples/websites/03-nested-sidebar`. Theme
  fingerprint shifted to `1c77b4407474e2eb`; all three sidebar
  rules present:
  ```
  .quarto-container{min-height:calc(100vh - 132px)}
  #quarto-sidebar>*{padding-right:1em}
  .sidebar.sidebar-navigation:not(.rollup){border-right:1px solid #dededf !important}
  ```
- [x] Full quarto-sass + quarto-core (1615 tests) pass; full
  `cargo xtask verify --skip-hub-build` clean.

### Phase 4 — verification & docs ✅

- [x] **End-to-end binary run:**
  ```
  cd examples/websites/03-nested-sidebar
  rm -rf _site
  cargo run --bin q2 -- render
  ```
  produced `_site/site_libs/quarto/quarto-theme-72a684b11d7c7a4c.css`
  (note the new fingerprint vs. the pre-fix `345008c71cc05875.css`,
  confirming the doc-vars layer affected the SCSS). `grep -oE
  '\.sidebar\.sidebar-navigation:not\(\.rollup\)\{[^}]*\}'` returns:
  ```
  .sidebar.sidebar-navigation:not(.rollup){border-right:1px solid #dededf !important}
  ```
  Same shape as Q1; the color `#dededf` is Q2's `$table-border-color`
  default and themes naturally override it. Verified the guide page
  `_site/guide/first-steps.html` references this theme CSS.
- [x] `cargo xtask verify --skip-hub-build` — all checks pass.
- [x] Documented `sidebar.border` and the implicit defaults in
  `docs/navigation.qmd` under a new "Sidebar appearance" section.

## Out of scope (linked follow-ups)

The doc-derived SCSS variables seam unlocks a family of Q1 knobs
that aren't yet ported. Each gets its own ticket once Phase 2 lands:

- `$sidebar-bg` / `$sidebar-fg` from `website.sidebar.background` /
  `…foreground`.
- `$navbar-bg` / `$navbar-fg` (already partially there in struct).
- `$footer-bg` / `$footer-fg` from `website.page-footer.background` /
  `…foreground`.
- Brand-color forwarding into Bootstrap colors.

These should reuse `derive_doc_scss_layer` from Phase 2, not invent
parallel mechanisms.

## Decisions / open questions

- **Per-document vs per-sidebar.** Q1's `format-html-scss.ts`
  emits a single `$sidebar-border` derived from the *first* sidebar
  it sees (the format-level config). For multi-sidebar sites
  (`03-nested-sidebar` itself), this matters only if sidebars
  differ. Mirror Q1: take the first sidebar's setting. Document in
  code.
- **Cache key.** The generic theme cache (`cache_key()`) hashes
  `ThemeConfig` but does not see metadata-derived doc vars.
  Phase 2 must extend the cache key to include the serialized
  doc-vars layer, otherwise two docs with different sidebar configs
  collide.
- **Why not just hardcode `$sidebar-border: true` in the Quarto
  layer's defaults?** Because Q1 lets users say
  `sidebar.border: false` to suppress the line, and Bootswatch
  themes can override `$table-border-color`. We need the indirection
  for both.
