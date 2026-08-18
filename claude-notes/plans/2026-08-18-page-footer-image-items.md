# page-footer item text: lone image dropped; no link/image target resolved (bd-page-footer-image-items-stmpikgo)

**Date:** 2026-08-18
**Braid:** bd-page-footer-image-items-stmpikgo
**Branch:** `main` (investigation committed in place; implementation branch TBD by user)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Both defects are confirmed at HEAD by code inspection and
end-to-end repro; the mechanisms are exactly as the strand describes, the fix
sites are small and known, and the only open questions are scope choices
(fix level for defect 1, whether defect 2's fix covers navbar/sidebar too).

## Issue context

Filed 2026-08-18 (today) by Carlos, P2 bug, labels `parity`, `websites`. Two
independent silent defects in page-footer *item* `text:` (the `- text: "..."`
list form), with the *region*-level `text:` form as a working control:

1. **A lone-image item renders empty.** `text: '![x](/images/logo.svg)'` →
   `<li class="nav-item"></li>`. Wrapping the image in a link or adding any
   sibling inline makes it survive.
2. **Nothing inside item `text:` is resolved.** Link/Image targets in item text
   are never rebased (`/images/logo.svg` stays root-absolute; `.qmd` never
   becomes `.html`), while the identical markdown in a region-level `text:`
   is fully resolved. 404s on any page below the site root; no diagnostic
   (body-content equivalents raise Q-5-6).

Real-world driver: the Posit Connect docs port (served from a subdirectory,
`https://docs.posit.co/connect/`) — its footer carries four images whose
obvious markdown-image workaround is defeated by exactly these two defects.
Origin strand in the connect-docs skein: `br-page-footer-image-items-sjto1juf`.
Filed P2 but is the blocker under a P1 there.

## Dependency graph

No parent/children; two `related` edges, both `in_progress`:

- **related: bd-page-footer-items-f4th80mj** (P1) — the strand that *made*
  item `text:` markdown-parsed at all (five defects: escaping, bare-string
  items, shortcodes, entities, href-less anchors). This new strand is the
  next layer of the same surface: text now parses, but images/links inside it
  aren't first-class. The `MARKDOWN_CONFIG_PATHS` entries for
  `page-footer.**.text` / navbar / sidebar carry its bd id in a comment.
- **related: bd-root-relative-paths-design-fc5pvkcv** (P1, design) — the
  three-case design for site-root-relative paths. Defect 2's machinery
  (`rewrite_config_inlines`, "Case C") was built under it; the call-site
  comment in `footer_render.rs` states the intent that this transform "owns"
  footer-inline resolution — item text was simply never routed through it.

No `discovered-from` edge in braid, but the description records the origin
session. No incoming `blocks` edges; urgency comes from the connect-docs port.

## What the code looks like today

Everything the strand points at exists unchanged at `main` @ 5b6774d1. Full
trace with line numbers: `page-footer-image-items-investigation/code-trace.md`.
Condensed:

- **Defect 1** is a three-hop interaction: `ConfigMarkdownTransform` parses
  item text via `pampa::pandoc::meta::parse_config_string_as_markdown`; the qmd
  reader's postprocess desugars a single-image paragraph into `Block::Figure`
  (`postprocess.rs:978`); `meta.rs:75` only unwraps a lone `Paragraph` to
  `PandocInlines`, so the value stays `PandocBlocks([Figure])`; and
  `render_text`'s `block_inlines` (`render_html.rs:913`) matches only
  `Plain|Paragraph|Header` → empty string.
- **Defect 2** is one missing call: `rewrite_items_hrefs`
  (`footer_render.rs:147`) rewrites `item.href` and recurses into `item.menu`
  but never touches `item.text`/`item.bare_text`, while the `Text` region
  branch one match-arm up calls `rewrite_config_inlines` (which already
  handles Link + Image recursively, `navigation_href.rs:488`).
- **Bonus finding (a):** the `Text` region branch matches only `PandocInlines`
  — a `PandocBlocks` text region (multi-paragraph `!md`, or a lone image
  today) silently skips the rewrite as well.
- **Bonus finding (b):** navbar (and presumably sidebar) item `text:` has the
  same defect-2 gap — `navbar_render.rs` rewrites item hrefs and the navbar
  *title's* inlines, but not item text inlines. Since f4th80mj blessed
  `navbar.**.text` / `sidebar.contents.**.text` as markdown, the same repro
  shape should misbehave there. Not yet verified end-to-end.

**Reproduced end-to-end at HEAD** (see § End-to-end verification below):
`repro/` under the investigation dir is a copy of the external repro
(`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/page-footer-image-items/`).

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Failing tests first:
  - unit: `parse_config_string_as_markdown("![x](y)")` yields inlines (or:
    `render_text` on a lone-image value is non-empty), per the chosen fix level;
  - unit: `rewrite_items_hrefs` rewrites Image `src` and Link `href` inside
    `item.text` (root-absolute and relative, `.qmd`→`.html`);
  - integration: end-to-end render of the repro project asserting the deep
    page's footer markup (drive the real binary path per CLAUDE.md).
- **Phase 1 — Defect 1:** make a lone-image config string survive to
  rendering (fix level per design question 1).
- **Phase 2 — Defect 2:** route item `text:`/`bare_text` inlines through
  `rewrite_config_inlines` in `rewrite_items_hrefs`, symmetric with the
  existing `menu` recursion; decide `PandocBlocks` handling.
- **Phase 3 — Scope extension (per design question 2):** same routing for
  navbar/sidebar item text, and/or the `PandocBlocks` Text-region gap.
- **Phase 4 — Diagnostics (per design question 4):** missing-resource
  warnings for footer references, if in scope.
- **Phase 5 — Docs** (user-facing only if behavior visibly changes; likely
  just changelog/strand notes).

## Open design questions for the user

1. **Fix level for defect 1 (lone image → Figure → dropped).** Two candidates:
   - **(a) In `parse_yaml_string_as_markdown_to_config`** (`meta.rs`): also
     unwrap a lone `Figure` back to its image → `PandocInlines([Image])`.
     Config strings are inline presentation contexts; a figure with caption
     semantics is arguably never wanted there. This fixes *every* consumer
     (item text, region text, navbar/sidebar text, titles) and — because the
     value becomes `PandocInlines` — the existing rewrite branches then work
     on it, fixing the lone-image resolution case for free.
   - **(b) In `render_text`/`block_inlines`**: teach the renderer to unwrap
     `Figure` to its image. Narrower, but leaves the value as `PandocBlocks`,
     which the rewrite branches skip — so defect 2's fix would then also need
     a blocks walker. My read is (a) is the right level; confirm?
2. **Scope of defect 2's fix.** Strand scopes to page-footer items. The navbar
   item-text gap (bonus finding b) is the same missing call in a sibling
   transform. Fix footer-only here and file a discovered-from strand for
   navbar/sidebar, or fix all three surfaces in this pass (shared helper on
   `NavigationItem`)?
3. **`PandocBlocks` handling in the rewriters.** With 1(a) chosen, lone images
   become inlines, but multi-block `!md` text (items or regions) still skips
   rewriting (bonus finding a). Add a `rewrite_config_blocks` walker now, or
   file separately? (`render_text` already renders blocks, so blocks *do*
   reach the page with unresolved targets today.)
4. **Diagnostics.** The strand notes a broken footer reference is silent while
   the same reference in body content raises Q-5-6. Does `rewrite_config_inlines`
   already emit Q-13-x diagnostics once the text is actually routed through it
   (i.e. does defect 2's fix close most of the gap for free), and is a
   missing-file warning for footer images in scope here or a separate strand?
5. **Priority.** Filed P2 to match the origin strand but described as "the
   blocker under a P1". Bump to P1?

## Risks / tradeoffs (draft)

- **Fix 1(a) changes a shared parse path** (`parse_config_string_as_markdown`
  feeds every blessed config key). Unwrapping lone figures there could change
  snapshot output for other keys — e.g. a `website.title` or `about` text that
  is a lone image (unlikely but possible). Snapshot churn must be reviewed per
  the CLAUDE.md snapshot policy. Note `!md`-tagged values take a different
  path (already `PandocBlocks` at load); decide whether they get the same
  unwrap or keep figure semantics.
- **The lone-figure unwrap must not fire for genuinely block contexts** — the
  same pampa parse is used for document metadata (`!md` in frontmatter?);
  verify the unwrap lives only on the config-string path (or is behaviorally
  safe everywhere).
- **Both related strands are `in_progress`** — f4th80mj (item-text rendering)
  and fc5pvkcv (path design) touch the same files; coordinate to avoid
  conflicting in-flight edits.
- The empty-caption edge (`![](x)`) does *not* desugar to Figure (postprocess
  requires non-empty caption) — tests should pin both variants.

## End-to-end verification (investigation)

Run at HEAD (`main` @ 5b6774d1, 2026-08-18), pre-flight
`cargo xtask verify --skip-hub-build` green first. Output inspected directly.

- Invocation: `cargo run --bin q2 -- render claude-notes/plans/page-footer-image-items-investigation/repro`
  ("Rendered 2 of 2 files", exit 0, **no diagnostics** — confirming the
  silent-failure claim).
- Observed footer in `_site/deep/deeper/index.html` (page two levels deep):

```html
<div class="nav-footer-left">
  <ul class="nav footer-items">
    <li class="nav-item"></li>                                     <!-- lone image: DROPPED (defect 1) -->
    <li class="nav-item"></li>                                     <!-- lone image, relative: DROPPED -->
    <li class="nav-item"><a href="https://posit.co"><img src="images/logo.svg" alt="wrapped in a link"></a></li>  <!-- survives but src unrebased -->
    <li class="nav-item"><img src="images/logo.svg" alt="image"> beside text</li>                                 <!-- survives but src unrebased -->
    <li class="nav-item"><img src="/images/logo.svg" alt="raw html"></li>
  </ul>
</div>
<div class="nav-footer-center"><img src="../../images/logo.svg" alt="region-level"> and a <a href="../../index.html">region-level link</a></div>  <!-- CONTROL: fully resolved -->
<div class="nav-footer-right">
  <ul class="nav footer-items">
    <li class="nav-item"><img src="/images/logo.svg" alt="item-level"> and an <a href="/index.qmd">item-level link</a></li>  <!-- defect 2: untouched -->
  </ul>
</div>
```

Matches the strand's tables exactly: lone-image items render empty; item-level
targets are untouched (root-absolute stays root-absolute, relative stays
page-relative-wrong, `.qmd` never becomes `.html`); the region-level control
in the same footer is fully resolved.
