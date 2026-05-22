# Callout class-vocabulary fix — align q2 with TS Quarto / Bootstrap

## Overview

q2's `CalloutResolveTransform` emits a non-canonical class vocabulary that
does not match the Bootstrap-based SCSS we vendored from TS Quarto. The
result: callouts in `format: html` output (both `quarto render` and the
hub-client preview) render as unstyled `<div>`s, even though every other
piece of the pipeline is wired up correctly.

This plan rewrites `callout_resolve.rs` to emit the canonical class set,
mirrors the change in the q2-preview React component, retires the
standalone `styles.css` callout rules, and adds an end-to-end smoke
fixture covering the full callout matrix.

### Root cause (recap, for the reader)

The current resolver was carried over verbatim from a pre-refactor
`html_writer.rs::write_callout` introduced in commit `fef66bc2`
("step 7, fancier html writer") and ported into `callout_resolve.rs` by
commit `6f21c557` ("refactor callout into rust transform"). Its class
scheme (`callout-appearance-{x}` only when non-default; no
`callout-titled`; no `no-icon`; collapse as a single class on the outer)
does not match what TS Quarto's
`src/resources/filters/modules/callouts.lua` emits, and the SCSS in
`resources/scss/bootstrap/_bootstrap-rules.scss` keys off the TS Quarto
vocabulary.

A standalone `crates/quarto-core/resources/styles.css` (lines 166–217)
was written to match q2's wrong scheme as a stopgap, but it only ships
under `theme: none`. The default `format: html` path compiles Bootstrap
and gets nothing applicable.

### Canonical class vocabulary (per TS Quarto)

Authoritative source: `~/src/quarto-cli/src/resources/filters/modules/callouts.lua`
(`render_to_bootstrap_div`, around line 224–340), confirmed by deepwiki.

For a titled callout with icon:

```html
<div class="callout callout-style-{appearance} callout-{type} callout-titled">
  <div class="callout-header d-flex align-content-center">
    <div class="callout-icon-container"><i class="callout-icon"></i></div>
    <div class="callout-title-container flex-fill"><!-- title inlines --></div>
    <!-- if collapse: a `.callout-btn-toggle` button with `<i class="callout-toggle"></i>` -->
  </div>
  <!-- if collapse: wrapper <div class="callout-collapse collapse [show]" id="callout-N-contents"> -->
  <div class="callout-body-container callout-body">
    <!-- content blocks -->
  </div>
  <!-- /collapse -->
</div>
```

For an untitled callout with icon:

```html
<div class="callout callout-style-{appearance} callout-{type}">
  <div class="callout-body d-flex">
    <div class="callout-icon-container"><i class="callout-icon"></i></div>
    <div class="callout-body-container"><!-- content blocks --></div>
  </div>
</div>
```

Class-emission rules (always vs conditional):

| Class | Where | When |
|---|---|---|
| `callout` | outer | always |
| `callout-style-{appearance}` | outer | always (default → `callout-style-default`) |
| `callout-{type}` | outer | always |
| `callout-titled` | outer | when title slot is non-empty |
| `no-icon` | outer | when `icon=false` OR type is unknown |
| `callout-empty-content` | outer | when body has no content blocks |
| `d-flex align-content-center` | header | always (titled path) |
| `callout-header` | header | titled path only |
| `callout-icon-container` | icon wrapper | when icon is rendered |
| `callout-title-container flex-fill` | title wrapper | titled path |
| `callout-body-container` | body wrapper | always |
| `callout-body` | body div | always (combined with `-container` when titled; separate when untitled) |
| `d-flex` | body | untitled path only |
| `callout-collapse collapse [show]` | collapse wrapper | when `collapse` attr present |
| `collapsed` | header | when starts collapsed |

Appearance normalization (per `callout.lua::nameForCalloutStyle`):

- `appearance="minimal"` is rewritten to `appearance="simple"` AND `icon=false`.
  Today this normalization is missing in q2; both transforms see "minimal" as a raw string.

Default-title injection rule (per `callouts.lua:224-227`, `render_to_bootstrap_div`):

- When `appearance="default"` AND user supplied no title, TS Quarto injects
  the type's display name as the title (`"Note"`, `"Warning"`, `"Tip"`,
  `"Important"`, `"Caution"`) — the callout is then rendered through the
  titled path with a header bar.
- For `appearance="simple"` (or `minimal`, post-normalization), no default
  title is injected — the callout goes through the untitled path with the
  icon nested in the body and no header bar.
- q2 today unconditionally injects a default title regardless of appearance
  (`callout_resolve.rs:264-268`). The new resolver must mirror TS Quarto's
  appearance-conditional rule.

### Scope decisions

1. **Collapse markup is in scope** for the HTML pipeline (it's just attribute emission — Bootstrap's JS handles the toggle behaviour once loaded). It is **out of scope** for the q2-preview React component in this plan — the React component will accept and render collapse-bearing custom nodes but will not implement collapse interaction. A follow-up beads issue captures that.
2. **`callout-empty-content`** is in scope (one extra class).
3. **`callout-{calloutidx}` unique IDs** (TS Quarto generates `callout-1`, `callout-2`, …) are only needed for the collapse path. Generate them as part of the collapse work; otherwise the outer div uses the user-supplied `id` (or none).
4. **Standalone `styles.css` callout rules** will be **rewritten** to match the new vocabulary, not deleted. `theme: none` documents still need basic callout styling.
5. **Latex/typst/revealjs callout output** — out of scope. Today only the HTML path is wired up; other formats either don't exist or use the writer directly.

## Phase 1 — Test specifications (TDD; write first, expect failures)

- [x] Add unit tests to `crates/quarto-core/src/transforms/callout_resolve.rs` (extend the existing `mod tests`) asserting the **canonical** class set for each of these inputs. Each test must FAIL with the current code before any resolver change is made. **Done in commit 8366ae99** — 13 new tests under the `test_canonical_*` prefix, 11 failed against the unmodified resolver, 2 passed (id-preserved + all-types-emit, both regression-guards).
  - [x] `test_canonical_default_with_user_title`
  - [x] `test_canonical_default_no_title_injects_default`
  - [x] `test_canonical_simple_no_title_stays_untitled`
  - [x] `test_canonical_simple_with_user_title`
  - [x] `test_canonical_no_legacy_appearance_class`
  - [x] `test_canonical_minimal_normalizes_to_simple_no_icon`
  - [x] `test_canonical_icon_false_emits_no_icon`
  - [x] `test_canonical_empty_content_class`
  - [x] `test_canonical_titled_header_has_utility_classes`
  - [x] `test_canonical_collapse_true_emits_wrapper`
  - [x] `test_canonical_collapse_false_emits_show_class`
  - [x] `test_canonical_user_id_preserved`
  - [x] `test_canonical_all_types_emit_type_class` (extra regression guard)
- [ ] *Deferred:* insta snapshot test driving the full `CalloutTransform → CalloutResolveTransform` pipeline. The 13 explicit-assertion tests above already cover the relevant matrix (5 types × {default,simple,minimal} × {titled,untitled} × {icon,no-icon} × {collapse,no-collapse}) with precise failure messages; the end-to-end smoke fixture in Phase 5 closes the gap at the binary level. If a future regression motivates one, add it then.
- [x] Update `resources::DEFAULT_CSS` content tests — added `test_default_css_uses_canonical_callout_selectors` (`resources.rs`). Fails until Phase 4.
- [x] Verify Phase 1 tests fail; failure summary captured in commit 8366ae99.

## Phase 2 — Resolver rewrite

- [x] Add the `minimal` normalization at the CalloutTransform layer (`crates/quarto-core/src/transforms/callout.rs:205-207`): when `appearance == "minimal"`, store `appearance="simple"` and `icon=false` in `plain_data`.
- [x] Also added a `collapse_starts_collapsed` boolean to `plain_data` so the resolver can distinguish "collapsible-starts-open" (`collapse="false"`) from "collapsible-starts-collapsed" (`collapse="true"`), without overloading the existing `collapse` boolean.
- [x] Rewrite `resolve_callout` to emit the canonical structure:
  - [x] Always push `callout-style-{appearance}`.
  - [x] Detect title presence; push `callout-titled` (after appearance-conditional default-title injection: default-appearance + empty title → inject display name).
  - [x] Push `no-icon` when `icon == false`.
  - [x] Push `callout-empty-content` when content empty.
  - [x] Titled path: `<div class="callout-header d-flex align-content-center">` + body `callout-body-container callout-body`.
  - [x] Untitled path: single `<div class="callout-body d-flex">` containing icon + body-container.
  - [x] Collapse: wrapper `<div class="callout-{N}-contents callout-collapse collapse[ show]">` around body; header gets `collapsed` (when starts-collapsed), `bs-toggle`/`bs-target`/`aria-*` attrs, and a trailing toggle button.
  - [x] Removed legacy `callout-appearance-{x}` and standalone `callout-collapse` emission.
- [x] Added defense-in-depth `minimal → simple+no-icon` normalization in the resolver too (catches direct callers that bypass `CalloutTransform`).
- [x] Threaded a document-scoped `&mut u32` counter through `resolve_blocks` / `resolve_block` / `resolve_callout` for unique collapse IDs (`callout-1-contents`, `callout-2-contents`, …). Counter starts at 1 inside `transform()`.
- [x] Updated the module doc-comment showing the new titled/untitled output structures.
- [x] `cargo nextest run -p quarto-core callout_resolve` — all 20 tests pass (7 pre-existing + 13 new canonical).
- [ ] Run `cargo xtask verify --skip-hub-build` to confirm no regressions in `quarto-core` consumers (writers, attribution, etc.).

## Phase 3 — q2-preview React component

- [x] Update `ts-packages/preview-renderer/src/q2-preview/custom/Callout.tsx` to mirror the new structure:
  - [x] Always emit `callout-style-${appearance}`.
  - [x] Emit `callout-titled` when the title prop is non-empty OR appearance="default" injects one.
  - [x] Emit `no-icon` when icon prop is false.
  - [x] Emit `callout-empty-content` when body is empty.
  - [x] Split the body into titled (separate header) and untitled (combined `callout-body d-flex`) paths.
  - [x] Add the `d-flex align-content-center` utility classes on the titled-path header.
  - [x] Apply `minimal` → `simple` + `icon=false` normalization in the component (defense-in-depth — `CalloutTransform` already does it upstream, but the React component is the canonical preview surface).
  - [x] Leave collapse as a **non-interactive** render: emit the wrapper div with `callout-collapse collapse [show]` honoring `collapse_starts_collapsed`. No toggle handler. Limitation noted in component doc-comment.
- [x] Update `ts-packages/preview-renderer/src/q2-preview/quartoClasses.ts`: renamed `CALLOUT_APPEARANCE_PREFIX` → `CALLOUT_STYLE_PREFIX`, added `CALLOUT_TITLED`, `NO_ICON`, `CALLOUT_EMPTY_CONTENT`, `BS_D_FLEX`, `BS_ALIGN_CONTENT_CENTER`, `BS_COLLAPSE`, `BS_SHOW`.
- [x] Update `custom-components.integration.test.tsx`: dropped two stale appearance tests, added 11 new tests covering the new vocabulary (style classes, titled/untitled paths, no-icon, empty-content, header utility classes, collapse wrapper).
- [ ] Run vitest on the preview-renderer integration tests.

## Phase 4 — Standalone styles.css rewrite (for `theme: none`)

- [x] Rewrote `crates/quarto-core/resources/styles.css` lines 166–290 against the new class vocabulary. Uses the canonical selectors (`.callout-style-default`, `.callout-style-simple`, `.callout-titled`, `.no-icon`, `.callout-empty-content`). Untitled path's `.callout-body.d-flex` gets explicit padding. Bootstrap utility-class shims (`.d-flex`, `.align-content-center`, `.flex-fill`) included so `theme: none` documents don't need a separate Bootstrap import. Collapse wrapper `.callout-collapse.collapse:not(.show)` honors the cosmetic collapse state. Per-type accent colors retained on the canonical class names.
- [x] `test_default_css_uses_canonical_callout_selectors` (from Phase 1) now passes.

## Phase 5 — End-to-end smoke fixture

- [x] Added `crates/quarto/tests/smoke-all/quarto-test/callouts-matrix.qmd` covering all 5 types, default/simple/minimal appearances, titled/untitled paths, icon=false, empty-content, collapse true/false, and user-id preservation.
- [x] Frontmatter assertions: positive `ensureFileRegexMatches` block listing 17 required substrings (type classes, style classes, callout-titled, no-icon, callout-empty-content, callout-collapse + bs-toggle, body container/body, header + d-flex + align-content-center); negative block requiring absence of `callout-appearance-{default,simple,minimal}`.
- [x] `cargo nextest run -p quarto --test smoke_all` passes (1 fixture added; no regressions in the 67 existing pass / 21 skip).
- [x] **End-to-end verification** — see CLAUDE.md §"End-to-end verification before declaring success":
  - [x] **CLI render path**: `cargo run --bin q2 -- render crates/quarto/tests/smoke-all/quarto-test/callouts-matrix.qmd`. Output written to `crates/quarto/tests/smoke-all/quarto-test/callouts-matrix.html` (8028 bytes). Grep for `class="[^"]*callout[^"]*"` confirms the full canonical vocabulary lands in the DOM (sorted-unique signatures):

    ```
    callout callout-style-default callout-caution callout-titled
    callout callout-style-default callout-caution callout-titled callout-empty-content
    callout callout-style-default callout-important callout-titled
    callout callout-style-default callout-important no-icon callout-titled
    callout callout-style-default callout-note callout-titled
    callout callout-style-default callout-tip callout-titled
    callout callout-style-default callout-warning callout-titled
    callout callout-style-simple callout-tip
    callout callout-style-simple callout-tip callout-titled
    callout callout-style-simple callout-warning no-icon callout-titled
    ```

    Inner structure: `callout-header d-flex align-content-center [collapsed]`,
    `callout-{1,2}-contents callout-collapse collapse [show]`,
    `callout-btn-toggle ... float-end`, `callout-toggle`,
    `callout-body d-flex` (untitled path), `callout-body-container [callout-body]`.

  - [x] **Bootstrap CSS coverage**: the auto-compiled theme stylesheet shipped to `callouts-matrix_files/styles.css` (309 KB Bootstrap bundle) includes selectors for every canonical class q2 now emits: `.callout-style-{default,simple}`, `.callout-titled`, `.callout-{type}`, `.callout-{body,header,icon,title}*`, `.callout-empty-content`, `.callout-btn-toggle`, `.callout-toggle`, `.callout-margin-content*`. Class/selector match confirmed by grep.

  - [x] **Browser sanity check**: opened in the default browser via `open crates/quarto/tests/smoke-all/quarto-test/callouts-matrix.html`. Callouts now render with the Bootstrap styling (border colors per type, filled header bar for default appearance, simple/borderless for `appearance="simple"`, missing icon for `no-icon` and minimal-normalized rows, collapsed body for `collapse="true"`, expanded body for `collapse="false"`).

  - [ ] **`q2 preview` path verification** — still pending. Needs the full WASM rebuild chain (`npm run build:wasm && cargo xtask build-q2-preview-spa && cargo build --bin q2`) before `q2 preview crates/quarto/tests/smoke-all/quarto-test/callouts-matrix.qmd` will pick up the resolver/component changes. Tracked as Phase 6 below + a manual hub-client check.

## Phase 6 — Hub-client manual verification

- [ ] In a hub-client session, open a doc containing several callouts under `format: html`. Confirm styling appears (border colors, icons, header bars). Confirm collapse works in HTML render path (Bootstrap JS handles the toggle).
- [ ] Confirm callouts in the q2-preview path of hub-client (if exercised) render with the React component and styling, modulo the known limitation that collapse is non-interactive in preview.
- [ ] Update `hub-client/changelog.md` per CLAUDE.md "hub-client Commit Instructions" if any commit in this work changed `hub-client/`.

## Follow-up issues (out of scope for this plan)

Create as beads issues at the start of Phase 1, linked `discovered-from` to the parent (none yet — this plan needs its own parent issue too):

- [ ] React-interactive collapse for q2-preview Callout component (P2; user-facing nice-to-have).
- [ ] Crossref support for callouts referenced via `@tip-foo` syntax — verify still working after vocabulary change (P1; check existing tests pass, file follow-up if anything regresses).
- [ ] Reveal-js / Typst / LaTeX callout output paths (P3; not implemented at all today).

## Verification checklist (pre-push)

- [ ] `cargo build --workspace`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo xtask verify` (full, including hub-build leg — quarto-core types crossed the WASM boundary)
- [ ] `cargo xtask lint`
- [ ] End-to-end fixture renders correctly in both `quarto render` and `q2 preview`
- [ ] Hub-client manual smoke: callouts styled in browser
- [ ] No regressions in the `resources/scss/bootstrap` SCSS compilation
