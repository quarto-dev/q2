# Footer/nav config markdown drops Link attributes and unwraps attributed Spans (bd-footer-link-attrs-dropped-1axx82op)

**Date:** 2026-08-19
**Braid:** bd-footer-link-attrs-dropped-1axx82op
**Checkout:** invoked in the bd-nn2fou8h worktree, on `main` @ `87c0e21a` (v0.25.0) — no dedicated branch created; user decides where implementation lands.
**Status:** Design aligned 2026-08-20; implementation approved and in progress on branch `braid/bd-footer-link-attrs-dropped-1axx82op`.

## Triage verdict

**Ready to design.** Root cause confirmed at HEAD with a minimal in-repo repro; the fix is small and self-contained in `crates/quarto-navigation/src/render_html.rs::push_inline`, with the `Image` arm and the body HTML writer as working references. Only output-shape choices (title precedence, span-wrapping policy, attribute ordering) need user alignment.

## Issue context

Filed 2026-08-20 (UTC) by Claude (q2-connect-docs), bug, P2, labels `navigation`/`parity`, status open. Config-authored markdown in nav/footer text regions renders links without their `{#id .cls key=val}` attributes and unwraps attributed spans entirely, while images keep every attribute. Happens identically at region level (`page-footer.center`) and item level (`item.text`), in the same render where body markdown keeps everything.

Real-world hit: the Connect docs' cookie-preferences footer control is a link whose `#open_preferences_center` id is what the cookie-consent JS hooks on; written as markdown the id is silently dropped and the cookie dialog dies on every page. The port currently works around it with raw ``` `<a ...>`{=html} ``` inline HTML.

Q1 parity target: Q1 preserves link id/class/title and attributed spans at region level. (Q1 doesn't parse item-level `text:` as markdown at all, so q2's item-level parsing is an improvement; the attr drop is still a defect on both levels.)

## Dependency graph

**Empty in this skein** — no `dep tree` / `dep list` edges. Context instead comes from two textual references in the description:

- **bd-page-footer-image-items-stmpikgo** (closed, fixed in PR #551, 2026-08-19): the sibling defect family — lone-image items dropped, item `text:` targets unresolved. Its fix built the machinery this strand's fix sits next to: `parse_config_string_as_markdown`, `rewrite_config_inlines`/`rewrite_config_blocks` (now in `crates/quarto-core/src/transforms/navigation_href.rs`), item-text rewriting on footer+navbar+sidebar. Good model for scope ("all three nav surfaces") and for where e2e tests live.
- **br-footer-link-attrs-dropped-0ltf6v96**: origin strand in the external q2-connect-docs skein (not accessible here; its substance is folded into this strand's description).

No incoming `blocks` pressure; urgency is the Connect-docs port's workaround (P1 context there, filed P2 here).

## What the code looks like today

All description claims verified at `main` @ `87c0e21a`:

- `crates/quarto-navigation/src/render_html.rs::push_inline` (the strand says `inline_to_html`; actual names are `push_inline` + wrapper `inlines_to_html` — minor drift, same code):
  - **Link arm** (`:1019`): emits `href` and the *target* title only; never reads `l.attr`.
  - **Span arm** (`:1033`): `// Drop attributes for simplicity; render content.` — unconditional unwrap.
  - **Image arm** (`:1060`): full treatment — src, alt, target title, then id/class/kv — the control that works.
- **Body-writer parity reference**: `crates/pampa/src/writers/html.rs:966-1003`. Link emits `href`, then `write_attr(&link.attr)` (id, class, kvs), then target title. Span always emits a real `<span>` with attrs.
- **Rewriter unaffected**: `rewrite_config_inlines` (`crates/quarto-core/src/transforms/navigation_href.rs:726`) already recurses into `Inline::Span` content, so links inside spans keep getting href-rewritten once spans stop being unwrapped. Render-side fix only.
- `push_inline` is shared by all nav surfaces (footer regions, item text on footer/navbar/sidebar, navbar title), so one fix covers region- and item-level alike.
- **Also spotted (not in the strand)**: `Inline::Code` carries an `attr` too (`quarto-pandoc-types/src/inline.rs:206`), and the Code arm (`render_html.rs:1014`) drops it. Same defect class; scope question below.

**Reproduced at HEAD**: `claude-notes/plans/footer-link-attrs-investigation/repro/` — `cargo run --bin q2 -- render <dir>`, inspect `_site/index.html`. Footer link renders as bare `<a href>`, span unwraps, image keeps class+style, body control keeps everything. README there has the captured output.

## Design decisions (aligned with Carlos, 2026-08-20)

1. **Title precedence:** target title wins; a `title` kv is suppressed only when a target title is present (pandoc parity, never a duplicate attribute). Applied uniformly to Link *and* Image (the Image arm had the same latent duplicate-title path). The body writer's own duplicate-title quirk is filed as **bd-nkk2z7on** (discovered-from this strand, P3).
2. **Span wrapping:** attr-only — a span with a non-empty attr renders `<span ...>content</span>`; an attr-less span stays unwrapped as today.
3. **Code attrs:** in scope, fixed in the same pass via the same helper.
4. **Snapshot churn:** treated as mechanical during implementation, **plus a spot check of the resulting snapshot diffs before declaring the work finished** (per Carlos).

Emission order (matches the existing Image arm): tag-specific attrs first (`href`/`src`+`alt`), then target `title`, then `id`, `class`, kvs in insertion order (Attr's kv store is a `LinkedHashMap`). Helper: `push_attr_html(out, attr, suppress_title_kv)` in `render_html.rs`.

## Work items

- [x] Phase 0 — TDD: failing unit tests in `render_html.rs` (link attrs, title precedence, attributed span, attr-less span unwrapped, code attrs) + failing e2e in `crates/quarto-core/tests/integration/navbar_footer_pipeline.rs` (region- and item-level). Verified failures: 3/4 unit tests failed as predicted (`link_target_title_suppresses_title_kv` passes pre-fix by coincidence — with all attrs dropped, target-title-only output matches the desired precedence output; kept as a regression guard); e2e failed showing `<a href="https://example.com/">prefs</a> and sp`.
- [x] Phase 1 — `push_attr_html` helper; Link, Span, Code arms fixed; Image arm refactored onto the helper (byte-identical output for images without a `title` kv; with target title + `title` kv, images now also suppress the kv).
- [x] Phase 2 — All 12,951 workspace tests pass. **Snapshot spot check: zero `.snap` files changed, and `grep -rl 'nav-footer|footer-items' --include='*.snap' crates/` finds no snapshot covering footer/nav HTML at all — zero churn is structural.** Clippy clean on quarto-navigation.
- [x] Phase 3 — End-to-end verified through the real binary:
  `cargo run --bin q2 -- render claude-notes/plans/footer-link-attrs-investigation/repro`, then inspected `_site/index.html`:
  ```html
  <div class="nav-footer-center"><a href="https://example.com/prefs" id="open_preferences_center" class="footer-link" title="Cookie Preferences">cookie prefs</a> and <span id="sp" class="sp-cls">attributed span</span> and <img src="images/logo.svg" alt="logo" class="footer-logo" style="height: 22px;"></div>
  ...
  <li class="nav-item"><a href="https://example.com/item" id="item-id" class="item-cls">item link</a></li>
  ```
  Region- and item-level links keep id/class/title; attributed span keeps its wrapper; image control unchanged.
- [x] Phase 4 — full `cargo xtask verify` green (Rust + WASM + hub-client legs, exit 0). Committed as f434e0e0; strand commented. Not pushed (awaiting approval).

## Risks / tradeoffs (draft)

- Low risk overall: pure renderer change, no parsing or rewriting involved; the rewriter already handles Span recursion.
- Emitting arbitrary kv attrs into `<a>` follows the existing Image-arm policy (everything `escape_attr`-escaped); no new escaping surface.
- Existing e2e/snapshot tests asserting current footer HTML (bare `<a>`, unwrapped spans) may need updates — expected, will be itemized.
- `quarto-navigation` is in the WASM dependency closure, so full `cargo xtask verify` (not `--skip-hub-build`) before commit.
