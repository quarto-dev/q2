# Video shortcode support in `q2 preview` / hub-client

**Strand:** bd-5b21rbaq
**Date:** 2026-06-22
**Status:** Diagnosis complete; plan drafted; awaiting go-ahead to implement.

## Overview

The Quarto `{{< video ... >}}` shortcode embeds an `<iframe>` player in
`q2 render` (HTML and revealjs) but degrades to a **plain `<a>` link** in
`q2 preview` and the hub-client WASM preview. This plan diagnoses the root
cause and proposes a fix.

Running example:
`/Users/cscheid/Desktop/daily-log/2026/06/22/update/slides.qmd`
(copied to `/tmp/video-repro/slides.qmd` for reproduction).

## Reproduction (verified 2026-06-22)

All three paths exercised end-to-end through the real `q2` binary.

| Path | Invocation | Result |
|------|-----------|--------|
| **render html** | `q2 render /tmp/video-repro/slides.qmd` | ✅ Full-width `<iframe>` — `<div class="quarto-video ratio ratio-16x9"><iframe ... src="https://www.youtube.com/embed/sAWFsP0Bbbk" ...></iframe></div>`. Verified in browser (full YouTube player). |
| **render revealjs** | `q2 render` on a `format: revealjs` copy | ✅ Bare `<iframe>` (no wrapping `quarto-video`/ratio div) — plays, but small/unsized. |
| **preview html** | `q2 preview --no-browser --port 8766 /tmp/video-repro/slides.qmd` | ❌ Plain link: `<a href="https://youtu.be/sAWFsP0Bbbk">https://youtu.be/sAWFsP0Bbbk</a>`. Verified in browser (a11y snapshot node `link "https://youtu.be/sAWFsP0Bbbk"`). |

## Root cause

The `video` shortcode is a **built-in Lua extension**
(`resources/extensions/quarto/video/video.lua`). Its dispatch is gated on the
output format:

```lua
if quarto.doc.is_format("html:js") then
    return htmlVideo(...)        -- emits the <iframe>
elseif quarto.doc.is_format("asciidoc") then ...
elseif quarto.doc.is_format("markdown") then
    return pandoc.Link(...)      -- a link
else
    return pandoc.Link(srcValue, srcValue)   -- FALLBACK: the plain link we see
end
```

`quarto.doc.is_format(query)` (in `crates/pampa/src/lua/quarto_doc.rs`,
`is_format_match` / `is_html_output`) matches the Lua `FORMAT` global against
the query. `FORMAT` is set verbatim from the pipeline's `target_format` string
(`crates/pampa/src/lua/shortcode.rs:102`, and `filter.rs:186` for user
filters).

- **render html:** `target_format = "html"` → `is_html_output("html")` is
  true → `is_format("html:js")` true → iframe. ✅
- **render revealjs:** `target_format = "revealjs"` → `is_html_output` true →
  iframe (no wrapper div because `video.lua` special-cases `isRevealJS()`). ✅
- **preview html:** `target_format = "q2-preview"` (a **preview pseudo-format**,
  see `map_format_for_preview` in `crates/wasm-quarto-hub-client/src/lib.rs:662`
  and `builtin_pseudo_format` in `crates/quarto-core/src/format.rs:115`).
  `is_html_output("q2-preview")` is **false** (the pseudo-format is not in the
  HTML family list) → `is_format("html:js")` false → falls through to the
  fallback `pandoc.Link`. ❌
- **preview revealjs:** `target_format = "q2-slides"` → same problem.

### Key insight: this is **not** video-specific

Any Lua shortcode or filter (built-in or user) that gates on
`is_format("html")` / `"html:js"` / `"revealjs"` will misbehave under preview,
because the Lua `FORMAT` global carries the q2 preview pseudo-format
(`q2-preview` / `q2-slides`) instead of the canonical pandoc output format it
emulates. Video is just the first place we noticed. The fix belongs at the
format boundary, not in `video.lua`.

`builtin_pseudo_format()` already encodes the base mapping
(`q2-preview`/`q2-debug`/`q2-sandboxed-preview` → `html`, `q2-slides` → `html` w/ preview
kind), but for **Lua FORMAT purposes** `q2-slides` should resolve to
`revealjs` (so `is_format("revealjs")` is true), matching what
`is_revealjs_target("q2-slides")` already does for pipeline decisions.

Verified safe: **no** `.lua` under `resources/` references `q2-preview` /
`q2-slides`, so normalizing the FORMAT global cannot break a deliberate
pseudo-format check.

## Proposed fix

**Normalize the Lua `FORMAT` global to the canonical pandoc output format at
the quarto-core boundary**, so preview Lua shortcodes/filters see exactly what
render sees. Keep `target_format` unchanged for q2 pipeline decisions
(`is_revealjs_target`, `pipeline_kind`, the SPA's AST-vs-HTML branch) — only the
value handed to the Lua engines changes.

### Layering choice (recommended)

Do the mapping in `quarto-core` (which already owns `format.rs` and the
pseudo-format vocabulary), **not** in pampa's `is_format`. pampa is the
lower-level qmd-parser + Lua-runtime crate and should stay a pure pandoc-format
matcher with no knowledge of q2 preview pseudo-formats. This matches the
"format-agnostic core, format knowledge lives in quarto-core" sensibility.

Add to `crates/quarto-core/src/format.rs` (next to `builtin_pseudo_format`):

```rust
/// The canonical pandoc output format a Lua filter/shortcode should see as its
/// `FORMAT` global. Preview pseudo-formats resolve to the real format they
/// emulate so `is_format("html:js")`, `is_format("revealjs")`, etc. behave
/// identically in preview and render.
pub fn lua_format_for(target_format: &str) -> &str {
    match target_format {
        "q2-preview" | "q2-debug" | "q2-sandboxed-preview" => "html",
        "q2-slides" => "revealjs",
        other => other,
    }
}
```

Then in `build_transform_pipeline` (`crates/quarto-core/src/pipeline.rs`
~L1108-1124), compute `let lua_format = lua_format_for(&target_format).to_string();`
**before** `target_format` is moved, and pass `lua_format` to
`ShortcodeResolveTransform::with_lua_support(...)` instead of `target_format`.
Keep the existing `is_revealjs = is_revealjs_target(&target_format)` line as-is.

Apply the same normalization on the **user Lua filter** path
(`pampa::lua::apply_lua_filters`, reached via `unified_filter.rs:239`) — trace
where that call's `target_format` originates in the preview pipeline and feed it
the normalized value too.

### Alternatives considered (and why not)

- **Teach pampa's `is_html_output` about `q2-*`** — leaks q2 preview vocabulary
  into the parser/runtime crate; pollutes the pure format matcher. Rejected.
- **Patch `video.lua` to accept `q2-preview`** — fixes only video; every other
  format-gated Lua filter stays broken; and it bakes preview pseudo-formats into
  a resource we keep in sync with TS Quarto. Rejected.

## Secondary issue: revealjs iframe sizing (separate)

`q2 render` with `format: revealjs` emits a **bare** `<iframe>` (no
`quarto-video`/`ratio-16x9` wrapper, because `video.lua` skips the wrapper for
revealjs), so the player renders small/unsized on the slide. TS Quarto sizes
reveal video via reveal/quarto SCSS. This is **independent** of the preview
bug and is about (S)CSS, not Lua dispatch. Tracked as a follow-up item below;
consider a separate strand if it grows.

## Work items

### Phase 0 — Tests first (TDD)
- [x] Failing end-to-end test: render the video shortcode through the **preview**
      pipeline (target_format `q2-preview`) and assert the output contains an
      `<iframe>` (currently a link). Routed through `render_qmd_to_preview_ast`
      (real preview entry), not `HtmlRenderConfig::default()`.
      → `crates/quarto-core/tests/integration/video_shortcode_preview.rs`.
- [x] Analogous `q2-slides` test asserting iframe under reveal preview.
- [x] Baseline html-render test (iframe) to prove the harness loads the
      built-in video extension.
- [x] Ran the new tests; html baseline **passes**, both preview tests **fail**
      for the expected reason — AST shows a `Link` at video.lua `lua_line:362`
      (the fallback `pandoc.Link`), confirming the shortcode ran and degraded.
- [x] Unit test for `lua_format_for` in `format.rs` (each pseudo-format → base,
      real formats pass through). Added alongside Phase 1.

### Phase 1 — Normalize Lua FORMAT (fixes video + all format-gated shortcodes)
- [x] Add `lua_format_for` to `crates/quarto-core/src/format.rs`.
- [x] Use it in `build_transform_pipeline` for the shortcode transform
      (`crates/quarto-core/src/pipeline.rs` ~L1116). `target_format` is kept for
      `is_revealjs`; only the Lua-facing value is normalized.
- [x] Ran new + unit tests: all 3 video tests pass, both `lua_format_for` unit
      tests pass.

### Phase 2 — Same normalization for user Lua filters
- [x] Traced the user-filter FORMAT source: `UserFiltersStage`
      (`crates/quarto-core/src/stage/stages/user_filters.rs:134`) used
      `ctx.format.identifier.as_str()`, which collapses **both** `q2-preview`
      **and** `q2-slides` to `html` — so `q2-preview` was already correct, but a
      user filter in **reveal preview** (`q2-slides`) wrongly saw `html` instead
      of `revealjs`.
- [x] Introduced `Format::lua_format()` (identifier base + revealjs override,
      so extension formats like `acm-pdf` → `pdf` stay correct) as the
      `Format`-aware companion to the string-only `lua_format_for`. Switched
      `UserFiltersStage` to it.
- [x] Unit tests: `test_format_lua_format_canonicalizes` (matrix incl.
      `q2-slides`→`revealjs`, `acm-pdf`→`pdf`) and
      `test_lua_format_helpers_agree_on_shared_cases` (the two helpers can't
      drift on shared cases). Existing `user_filters` stage tests still pass.
- Note: the end-to-end FORMAT-normalization mechanism is already exercised
  through the preview pipeline by the video shortcode tests; a dedicated
  user-Lua-filter e2e (authoring a filter that branches on
  `is_format("revealjs")`) is deferred — the unit coverage on `lua_format()`
  plus the shortcode e2e cover the logic. Revisit if we want belt-and-braces.

### Phase 3 — End-to-end verification (binary + browser)
- [x] Rebuilt the WASM + SPA chain so the embedded preview image is fresh:
      `cd hub-client && npm run build:wasm` (exit 0) →
      `cargo xtask build-q2-preview-spa` (exit 0) → `cargo build --bin q2`
      (exit 0; binary stamped 15:55). Per CLAUDE.md "Verifying Rust changes in
      `q2 preview`".
- [x] `q2 preview --no-browser --port 8767 /tmp/video-repro/slides.qmd`;
      confirmed via Chrome DevTools the video is now an embedded YouTube
      `<iframe>` (`url="https://www.youtube.com/embed/sAWFsP0Bbbk"`, a11y node
      `Iframe → RootWebArea "Hello Posit Assistant + Quarto Hub - YouTube"` with
      a "Play video" button), replacing the prior `link` node. Screenshot shows
      the full player, identical to `q2 render`.
- Note: `q2 preview` exercises the same WASM render path as the hub-client, so
  this confirms both. A separate live hub-client session wasn't needed.

**Observed output (preview, after fix):**
```
uid Iframe
  RootWebArea "Hello Posit Assistant + Quarto Hub - YouTube"
              url="https://www.youtube.com/embed/sAWFsP0Bbbk"
    button "Play video"
    link "Watch on YouTube" url="https://www.youtube.com/watch?v=sAWFsP0Bbbk"
```
(Before the fix this node was: `link "https://youtu.be/sAWFsP0Bbbk"`.)

### Phase 4 — Workspace verification
- [x] `cargo nextest run --workspace` — **10319 passed, 197 skipped, 0 failed**.
- [x] `cargo xtask verify` (full) — **✓ All verification steps passed!**
      (14/14 steps). First run was red only on a pre-existing missing
      `vitest-axe` dep in `ts-packages/preview-renderer` (unrelated to this
      change); `npm install` installed it, re-run is fully green.
- Note: `crates/wasm-quarto-hub-client/Cargo.lock` picked up a `0.4.0 → 0.5.0`
  version sync during `npm run build:wasm` — it was lagging the already-released
  0.5.0 workspace bump. Legitimate; include in the commit.

### Phase 5 — revealjs video iframe sizing (this strand, after the preview fix)
User confirmed: fold the sizing fix into this strand, after the larger fix.

Findings so far:
- `q2 render` `format: revealjs` emits a **bare** `<iframe>` (video.lua skips the
  `quarto-video`/`ratio` wrapper for reveal) with no width/height.
- Reveal core CSS only sets `.reveal iframe { z-index: 1 }` (reveal.scss:224) —
  no sizing — so the iframe falls back to the browser default (~300×150) and
  renders tiny on the slide. TS Quarto's `quarto.scss` has **no** `iframe`
  rule either, so the sizing must come from elsewhere (still to pin down).
- [x] Reproduced + measured (render reveal, browser): the video `<iframe>` is a
      **direct child of `section`** with no width/height → browser default
      **300×150** (rendered 222×111 after reveal scale), tiny on a 778px-wide
      slide. Screenshot captured.
- [x] **Mechanism confirmed empirically:** adding class `r-stretch` to the
      iframe + `Reveal.layout()` resizes it **222×111 → 778×414** (reveal sets
      inline `height:559px; width:1050px`). So reveal core *does* stretch
      `section > iframe.r-stretch` — **no custom (S)CSS needed**, identical to
      how images are stretched.
- [x] Found that **Q1's `applyStretch` is images-only**
      (`external-sources/.../format-reveal.ts:949`) — it does **not** stretch
      video/iframe. So a bare reveal video is *also* small in Q1; giving the
      iframe `r-stretch` is a q2 **enhancement**, not just parity.

**Decision needed — where to inject `r-stretch` on the reveal video iframe:**
- **A. Extend `RevealAutoStretchTransform`** (`crates/quarto-core/src/revealjs/
  auto_stretch.rs`) to recognize a standalone video `RawBlock(html,<iframe>)`
  and add `r-stretch`. Pro: same home as image stretch; can reuse the
  single-media gating + `auto-stretch:false`/`.nostretch` opt-outs. Con: the
  iframe is opaque raw HTML, so it means editing an HTML string.
- **B. video.lua adds `class="r-stretch"`** to the iframe when `isRevealJS()`
  (it already branches on reveal). Pro: built at the source, no Rust HTML
  surgery. Con: unconditional (ignores opt-outs), diverges from upstream
  video.lua (acceptable — q2 has no DOM postprocessor and Q1 doesn't stretch
  video anyway).

- [x] Decided: **conditional B + meta-bridge fix** (user's call).
- [x] Meta-bridge fix: `shortcode_to_lua_args` (`shortcode_resolve.rs`) now
      forwards boolean/numeric/inline scalars to the Lua handler `meta`, not
      just strings — so `auto-stretch: false` reaches `video.lua`.
- [x] `video.lua`: handler reads `meta['auto-stretch']` and the explicit
      `width`/`height`; passes `stretchReveal = autoStretch and not
      hasExplicitSize` to `htmlVideo`, which adds `class="r-stretch"` to the
      reveal iframe when set.
- [x] TDD tests (`revealjs_features.rs`): default reveal video → `r-stretch`
      (failed before, passes after); explicit `width` → no `r-stretch`;
      `auto-stretch: false` → no `r-stretch` (proves the bridge forwards the
      bool). Plus html-path guard (`video_shortcode_preview.rs`): html keeps
      `quarto-video`, never `r-stretch`. All pass.
- [x] q2-slides **preview parity** asserted in `video_shortcode_preview.rs`
      (slides preview AST carries `r-stretch`).
- [x] Browser (native render, new binary): reveal video now sized **778×414**
      (reveal inline `height:559px; width:1050px`) filling the slide, vs the
      prior 300×150. Screenshot captured.
- [x] Full quarto-core suite (2404 tests) green after the bridge change.
- [x] Rebuilt WASM→SPA→binary; checked the slides **preview** in the browser.
      **Finding (honest):** the preview AST correctly carries
      `<iframe class="r-stretch">`, BUT it is **not** visually stretched in
      q2-slides preview (stays ~222×111). Cause: the q2-preview SPA reveal
      renderer wraps the RawBlock iframe in a bare `<div>` —
      `section > div > iframe.r-stretch` — so reveal's direct-child stretch
      selector (`section > .r-stretch`) misses it, and the preview doesn't run
      the global `window.Reveal` layout the way native render does.

**Preview reveal-sizing fix (follow-up bd-xfw2omlt, now DONE):** RawBlock.tsx
mirrors a root-level `r-stretch` onto its wrapper `<div>` (so reveal stretches
the wrapper that React's `dangerouslySetInnerHTML` forces), and a
`quarto-reveal.css` rule `.reveal .slides section > div.r-stretch > iframe { … }`
makes the iframe fill it. TDD: `RawBlock.test.tsx`. Verified in the browser via
the real `q2 preview` binary — slides-preview video now 778×452 (was 222×111).
456 unit + 484 integration preview-renderer tests pass.

**hub-client end-to-end verification (real product).** Ran the local stack —
`q2 hub --no-project` (port 3000, anonymous: auth is opt-in via
`--oidc-client-id`) + `npm run dev:fresh` (build shows commit `82dbb636`) — and
drove it via Chrome DevTools: created a project, added a `format: revealjs`
`slides.qmd` with `{{< video … >}}`. Observed in the live editor's preview pane:
the video embeds as a YouTube `<iframe>` (Phase 1 fix) AND, on the active
"Video" slide, auto-stretches to fill it (678×394; wrapper carries `r-stretch`,
reveal sets `height:700px; width:1050px`). Note: reveal switches between
**scroll view** (narrow/tall pane → r-stretch wrapper collapses to height 0)
and **classic slide view** (wider pane → stretches correctly) based on pane
size. Verified in classic mode. The scroll-view r-stretch-collapse is a
reveal-scroll-mode trait, not specific to this fix — worth a separate look if
scroll view becomes the hub-client default.

**Scope outcome:** both originally-reported issues are fixed and verified —
(1) preview/hub-client video was a plain link → now an embedded iframe;
(2) `q2 render` revealjs video was tiny → now stretched (778×414). The
**q2-slides preview** video-sizing is a *newly-discovered third issue* in the
SPA reveal renderer (div-wraps RawBlocks, no reveal.js autostretch). It is
out of scope for the video shortcode and should be its **own strand** (likely a
preview-SPA CSS rule like `.reveal section > div > iframe.r-stretch { … }`,
since the preview can't rely on reveal's JS stretch). Filed as a follow-up.

## Key references
- `resources/extensions/quarto/video/video.lua` — dispatch + builders.
- `crates/pampa/src/lua/quarto_doc.rs` — `is_format_match`, `is_html_output`,
  FORMAT global read.
- `crates/pampa/src/lua/shortcode.rs:102`, `filter.rs:186` — FORMAT global set.
- `crates/quarto-core/src/format.rs:115` — `builtin_pseudo_format`,
  `is_revealjs_target`.
- `crates/quarto-core/src/pipeline.rs:1108-1124` — `build_transform_pipeline`;
  `:1418-1430` — `build_q2_preview_transform_pipeline`.
- `crates/wasm-quarto-hub-client/src/lib.rs:662` — `map_format_for_preview`;
  `:1333` — preview format selection.
