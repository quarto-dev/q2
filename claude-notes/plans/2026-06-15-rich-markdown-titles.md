# Rich Markdown in document titles is stringified

**Strand:** bd-5706gcrq

## Overview

Document `title` (and sibling title-block fields) that contain inline
Markdown — code spans, emphasis, strong, math, links — are rendered as
**plain text** in the `<h1 class="title">` element. The inline markup is
silently dropped.

**Reproduction** (`q2 render`, default full-HTML mode — the docs website):

```
title: Multiformat branding with `_brand.yml`
```

```bash
cargo run --bin q2 -- render docs/guides/authoring/brand.qmd
grep -o '<h1[^>]*>.*</h1>' docs/_site/guides/authoring/brand.html
# observed:  <h1 class="title">Multiformat branding with _brand.yml</h1>
# expected:  <h1 class="title">Multiformat branding with <code>_brand.yml</code></h1>
```

The backticks are consumed (so it *is* parsed as a code span), but the
span structure is flattened to its text content before reaching the
template. The head `<title>` tag is correct to be plain (`pagetitle`,
see below); only the body title block is wrong.

The user reports this in both `q2 render` and `q2 preview`. Both share
the `quarto-core` template path, so a single fix covers both — to be
confirmed by end-to-end verification of each (see CLAUDE.md "Verifying
Rust changes in `q2 preview`").

Related context: this is part of the website epic
(`claude-notes/plans/2026-04-23-website-project-epic.md`).

## Root-cause diagnosis

The title originates in YAML front matter and, because front matter is
interpreted as `InterpretationContext::DocumentMetadata`, it is parsed
into `ConfigValueKind::PandocInlines` — e.g.
`[Str("Multiformat branding with "), Code(_, "_brand.yml")]`. So far so
good: the rich structure exists. It is then **flattened to plain text**
at the point where metadata becomes a template variable.

There are **two independent flattening sites**, one per title-block mode:

### Site 1 — Full HTML template mode (the reported bug; default for the docs site)

The full template (`crates/quarto-core/src/template.rs:219`) is:

```
<h1 class="title">$title$</h1>
```

The `$title$` variable is bound by
`add_metadata_to_context_except` → `config_value_to_template_value`
(`template.rs:590-687`). For a `PandocInlines` value that function does:

```rust
// template.rs:660-663
ConfigValueKind::PandocInlines(content) => {
    let text = inlines_to_text(content);   // <-- flattens to plain text
    TemplateValue::String(text)
}
```

`inlines_to_text` (`template.rs:689+`) turns `Inline::Code(c)` into bare
`c.text`, dropping the `<code>` wrapper. The doctemplate engine emits
the resulting `TemplateValue::String` **raw** (no escaping —
`evaluator.rs:render_variable`), so whatever string we put here is
emitted verbatim into the h1.

> Side note / latent bug: because doctemplate emits the string raw, a
> plain-text title containing `&` or `<` is currently emitted
> unescaped, producing invalid HTML. Rendering through the HTML writer
> (the proposed fix) also fixes this.

### Site 2 — Minimal HTML mode

`TitleBlockTransform` (`crates/quarto-core/src/transforms/title_block.rs`)
injects an `h1` *into the AST* when the template won't generate one. The
reachable trigger today is **minimal HTML mode** (`theme: none`/`pandoc`,
or `minimal: true`), where `is_minimal_html(meta)` is true. It builds the
header from `extract_title` → `extract_plain_text` → `inlines_to_plain_text`
(`title_block.rs:121-149`), then `create_title_header(&title_text)` wraps
a single plain `Str` inline. Same flattening, different mechanism.

> The `should_add_h1` `else` branch (`title_block.rs:70-74`) also fires
> for any **non-HTML** format ("always add the h1"). This is currently
> **dead in practice**: the only render/output stage in the pipeline is
> `crates/quarto-core/src/stage/stages/render_html.rs`. There is no
> PDF/DOCX/Typst writer in Q2 yet — the `FormatIdentifier::{Pdf,Docx,
> Typst,…}` variants exist only for format-string parsing. When a
> non-HTML writer lands, fixing Site 2 to preserve inlines means those
> formats inherit correct behavior for free; until then the only
> observable Site 2 path is minimal HTML.

### What is *correctly* plain and must stay that way

`pagetitle` feeds the head `<title>` tag (`template.rs:166-167`) and is
derived as plain text by `derive_pagetitle`
(`crates/pampa/src/template/config_merge.rs:179-225`). HTML tags are
invalid inside `<title>`, so this must remain plain text. Likewise the
`<meta name="description">` / `keywords` / `author` *attribute* contexts
must stay plain. The fix must therefore be **targeted at body-rendered
title-block fields**, not a blanket "render all inline metadata as HTML".

### The available rendering primitive

`crates/quarto-core` already depends on `pampa`, and
`pampa::writers::html::write_inlines_to(&[Inline], W)`
(`crates/pampa/src/writers/html.rs:1801`) renders inlines to HTML with
default config (notably no source-location `data-` attributes). This is
the entry point the fix will call to turn title inlines into an HTML
string.

## Design decisions

1. **Which fields become rich HTML?** — DECIDED. Allowlist for the body
   title block: `title`, `subtitle` (inlines), `abstract` (blocks).
   `author`/`date` richness is **deferred to a follow-up** (author can be
   an object/list and also feeds a `<meta>` attribute; out of scope here).
2. **Scope of this strand.** — DECIDED. Fix **both** Site 1 (full HTML —
   the reported bug) and Site 2 (minimal HTML). Non-HTML formats don't
   render in Q2 yet, so they are out of scope regardless.
3. **Source annotations on the h1** — DECIDED (pending final user
   confirmation). The h1 is built in `ApplyTemplateStage` from an
   already-serialized body string + the `meta` ConfigValue; the AST and
   the body's pointer-keyed source map are **not** available there.
   The CLI render path emits the body with `include_source_locations:
   false`, so annotations only have value in the preview path.
   **This strand ships a clean h1** (no `data-` annotations) via
   `write_inlines_to` — which correctly matches the unannotated body in
   the render path and fixes the bug in both paths. **Source-annotated
   h1 for the preview path is a follow-up strand**, where we will check
   whether the HTML writer can annotate directly from each inline's
   `source_info` (the front-matter substring spans exist —
   `crates/pampa/src/pandoc/meta.rs:358`) rather than the pointer-keyed
   `set_source_map` map that isn't present at template-apply time.

## Work items

> TDD: tests first, confirm they fail, then implement, then full suite.

### Phase 1 — Failing tests (write first, verify they fail)

- [x] Unit test in `template.rs`: a `title` with a code span renders
      `<h1 class="title">…<code>_brand.yml</code>…</h1>` (failed as
      expected — produced plain text). Emphasis covered too.
      (`title_code_span_renders_as_html_code_element`,
      `title_emphasis_renders_as_html_em_element`,
      `subtitle_inline_markup_renders_as_html`)
- [x] Unit/regression test that `pagetitle` (head `<title>`) stays plain
      text for the same rich title (guard against over-reach).
      (`pagetitle_stays_plain_text_when_title_is_rich`)
- [x] Unit test that an unrelated inline-valued metadata field used in an
      attribute context is **not** turned into HTML (guard the allowlist).
      (`non_titleblock_inline_field_is_not_htmlized`)
- [x] Site 2: Test in `title_block.rs`: minimal-HTML-mode title header
      preserves inline structure (`Code` inline survives in the injected
      `Header`, not flattened to `Str`), and the whole injected subtree
      keeps title-block Generated provenance.
      (`test_minimal_mode_preserves_inline_markup_in_title`)
- [x] End-to-end: `q2 render docs/guides/authoring/brand.qmd` — h1 now
      `<h1 class="title">Multiformat branding with <code>_brand.yml</code></h1>`;
      head `<title>` stays plain (`… _brand.yml – Quarto 2`). Inspected.

### Phase 2 — Implementation

- [x] Site 1: `add_metadata_to_context{,_except}` now route through
      `metadata_entry_to_template_value`, which renders the allowlisted
      fields (`RICH_TITLE_BLOCK_FIELDS = title, subtitle, abstract`)
      `PandocInlines`/`PandocBlocks` to HTML via
      `pampa::writers::html::write_inlines_to` / `write_blocks_to`
      (`titleblock_field_to_html`). All other fields flatten as before.
- [x] Site 2: `extract_title` → `extract_title_inlines` (returns title as
      Pandoc inlines; string scalar → single `Str`, blocks flattened to
      inlines via `blocks_to_inlines`); `create_title_header` now takes
      `Vec<Inline>` and recursively stamps the synthetic title-block
      `Generated` provenance over the whole subtree (`stamp_generated` /
      `child_inlines_mut`), preserving the atomic boundary + no-preimage
      contract while keeping inline markup. Removed the now-unused local
      plain-text helpers (`extract_plain_text`, `inlines_to_plain_text`,
      `blocks_to_plain_text`).
      Verified e2e: `theme: none` fixture renders
      `<h1>Branding with <code>_brand.yml</code></h1>`.
- [x] Confirmed no double-escaping: doctemplate emits strings raw, so the
      HTML-writer output is correct as-is (verified via e2e render).

### Phase 3 — Verification

- [x] `cargo nextest run --workspace`: 10,049 tests passed, 0 failures.
- [x] End-to-end `q2 render docs/guides/authoring/brand.qmd`: h1 =
      `<h1 class="title">Multiformat branding with <code>_brand.yml</code></h1>`;
      head `<title>` plain. Inspected.
- [ ] End-to-end `q2 preview` of the same file (after the full WASM
      rebuild chain) — **not yet run in a browser.** The WASM/hub-client
      build leg of `cargo xtask verify` passed (the preview SPA was
      rebuilt and bundled), and the preview shares the same `quarto-core`
      template path that the render e2e exercised, so the fix is present
      in the preview image. A live browser confirmation is still
      outstanding — flagging per CLAUDE.md rather than claiming it.
- [x] `cargo xtask verify` (full): all 14 steps passed, including the WASM
      build and hub-client build+tests. (First run failed on a clippy
      `unused-import` for a test-only `Code` import — moved into the test
      module; re-verified clean. Note: piping `xtask verify` through
      `tail` masks its exit code — check the unpiped status.)
- [x] Snapshot review: **zero `.snap` files changed** (`git status` shows
      only the two source files + this plan). Title-block snapshots, if
      any existed for rich titles, were unaffected.

## Files in scope

- `crates/quarto-core/src/template.rs` — Site 1 (primary).
- `crates/quarto-core/src/transforms/title_block.rs` — Site 2.
- `crates/pampa/src/writers/html.rs` — `write_inlines_to` (consumer; no
  change expected).
- `crates/pampa/src/template/config_merge.rs` — `derive_pagetitle`
  (must remain plain; no change expected, guard with test).
