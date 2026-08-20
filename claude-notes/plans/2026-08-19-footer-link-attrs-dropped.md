# Footer/nav config markdown drops Link attributes and unwraps attributed Spans (bd-footer-link-attrs-dropped-1axx82op)

**Date:** 2026-08-19
**Braid:** bd-footer-link-attrs-dropped-1axx82op
**Checkout:** invoked in the bd-nn2fou8h worktree, on `main` @ `87c0e21a` (v0.25.0) — no dedicated branch created; user decides where implementation lands.
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Unit tests in `render_html.rs`'s in-file test module (link with id/class/kv/title, attributed span, attr-less span, title-precedence case); e2e regression in `crates/quarto-core/tests/integration/navbar_footer_pipeline.rs` (or `navigation_e2e.rs`) driving a real project render with region- and item-level cases. Write, verify failures.
- **Phase 1 — Attr-emission helper.** Factor the Image arm's id/class/kv emission into a helper (`push_attr_html` or similar) in `render_html.rs`.
- **Phase 2 — Link arm.** Use the helper; settle title precedence per design answer. Image arm switches to the helper too.
- **Phase 3 — Span arm.** Emit `<span ...>` wrapper per the chosen policy (attr-only vs always).
- **Phase 4 — (If in scope) Code arm attrs.**
- **Phase 5 — End-to-end verification.** Re-render the investigation repro; inspect footer HTML; record invocation + output snippet. Full `cargo nextest run --workspace` + `cargo xtask verify` (WASM leg — quarto-navigation feeds the preview path).
- **Phase 6 — Close out.** Snapshot-change report if any `.snap` files move; update strand; docs likely not needed (bug fix, no new surface).

## Open design questions for the user

1. **Title precedence.** When a link has both a target title (`[l](u "T1")`) and an attr kv title (`{title="T2"}`), which wins? The strand notes pandoc's HTML writer emits the target title. The body writer (`pampa/html.rs`) emits attr kvs *before* the target title, so a duplicate `title=` can occur and browsers keep the first (attr wins) — arguably a latent bug there too. Options: (a) match the body writer byte-for-byte (consistency, keeps its quirk), (b) target title wins, kv title suppressed when target title present (pandoc parity, no duplicate attribute). My lean: (b), and optionally file a discovered-from strand for the body writer's duplicate-title quirk.
2. **Span wrapping policy.** Strand suggests wrapping only when attr is non-empty (attr-less spans stay unwrapped, minimizing footer-markup churn); the body writer always emits `<span>`. Which do you want for nav surfaces? My lean: attr-only, per the strand.
3. **Code attrs in scope?** `Inline::Code` in nav text also drops its attr (e.g. `` `x`{.numberLines} `` — admittedly exotic in a footer). Fix in the same pass, or file separately as discovered-from? My lean: same pass, it's the same helper call.
4. **Attribute ordering / snapshot churn.** Reusing the helper in the Image arm keeps its current order (id, class, kvs after src/alt/title) — Link would match. OK to treat any resulting snapshot diffs as mechanical, reported per the snapshot policy?

## Risks / tradeoffs (draft)

- Low risk overall: pure renderer change, no parsing or rewriting involved; the rewriter already handles Span recursion.
- Emitting arbitrary kv attrs into `<a>` follows the existing Image-arm policy (everything `escape_attr`-escaped); no new escaping surface.
- Existing e2e/snapshot tests asserting current footer HTML (bare `<a>`, unwrapped spans) may need updates — expected, will be itemized.
- `quarto-navigation` is in the WASM dependency closure, so full `cargo xtask verify` (not `--skip-hub-build`) before commit.
