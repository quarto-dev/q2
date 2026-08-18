# Code trace — bd-page-footer-image-items-stmpikgo (2026-08-18, main @ 5b6774d1)

Both defects confirmed present at HEAD by code inspection. The strand's file
references are all current; nothing in the area has been refactored since filing.

## Defect 1 — lone-image item renders empty

Full mechanism, three hops:

1. `ConfigMarkdownTransform` (`crates/quarto-core/src/transforms/config_markdown.rs`)
   matches `page-footer.**.text` in `MARKDOWN_CONFIG_PATHS` and calls
   `parse_scalar_string_in_place` → `pampa::pandoc::meta::parse_config_string_as_markdown`.

2. `parse_yaml_string_as_markdown_to_config` (`crates/pampa/src/pandoc/meta.rs:49`)
   parses the string as a qmd document. The qmd reader's postprocess pass
   (`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:978`, `with_paragraph`)
   desugars a single-image paragraph into `Block::Figure`. Back in `meta.rs:75`,
   the "unwrap to inlines" check only matches `blocks.len() == 1 && Paragraph`,
   so a lone image stays `ConfigValueKind::PandocBlocks([Figure])` while any
   sibling inline keeps the block a `Paragraph` → `PandocInlines`.

3. `render_text` (`crates/quarto-navigation/src/render_html.rs:892`), `PandocBlocks`
   branch, calls `block_inlines` (line 913) which matches only
   `Plain | Paragraph | Header` and returns `None` for `Figure` → empty string.

Note the figure desugar only fires when the image has a non-empty caption
(`!image.content.is_empty()`), which is why `![lone image](...)` (alt text =
caption) vanishes; `![](...)` would survive. Consistent with the strand's table.

## Defect 2 — item text inlines never routed through the href/src rewriter

`FooterRenderTransform::transform` (`crates/quarto-core/src/transforms/footer_render.rs:89-106`)
calls `rewrite_region_hrefs` per region:

- `FooterRegion::Text(cv)` → `rewrite_config_inlines(...)` — rewrites Link + Image
  targets inside the parsed inlines. This is the region-level control that works.
  (Note: only the `PandocInlines` kind is matched — a `PandocBlocks` text region,
  e.g. multi-paragraph `!md` or a lone image pre-fix, is silently skipped too.)
- `FooterRegion::Items(items)` → `rewrite_items_hrefs(...)` (line 147) — rewrites
  `item.href`, recurses into `item.menu`, **never looks at `item.text` or
  `item.bare_text`**. That is the entire defect.

`rewrite_config_inlines` lives in
`crates/quarto-core/src/transforms/navigation_href.rs:488` and already handles
both `Inline::Link` and `Inline::Image` recursively. The fix is plumbing, not
new machinery.

## Scope observation: navbar and sidebar have the same item-text gap

`navbar_render.rs` rewrites item hrefs via its own `rewrite_navigation_item_hrefs`
(line 163) and rewrites the navbar *title's* inlines (line 125-136), but item
`text:` inlines are not rewritten there either. Since
bd-page-footer-items-f4th80mj made `navbar.left/right.**.text` and
`sidebar.contents.**.text` markdown-parsed (see `MARKDOWN_CONFIG_PATHS`), a
navbar item `text: '![x](/images/logo.svg)'` presumably hits the same two
defects. Not verified end-to-end; the strand scopes to page-footer, but the fix
for defect 2 wants to be a shared helper.

## Item text kinds to handle

`NavigationItem.text: Option<ConfigValue>` (`crates/quarto-navigation/src/item.rs:47`)
and `bare_text: Option<Box<ConfigValue>>` (footer bare-scalar items, demoted to
text when unresolvable). After `ConfigMarkdownTransform` each may be:

- `PandocInlines` — the common case (single-paragraph markdown);
- `PandocBlocks` — lone image (today), or multi-block `!md` text;
- `Scalar(String)` — pre-transform / non-blessed paths (render_text escapes it).

A rewrite of item text must at least handle `PandocInlines`; whether
`PandocBlocks` is handled by a blocks-walking wrapper or made rarer by fixing
defect 1 at the parse level is a design question (see plan).

## Repro

`repro/` here is a copy of the sources from
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/page-footer-image-items/`
(same tables in its README.md). Run `cargo run --bin q2 -- render <this dir>/repro`
and inspect `_site/deep/deeper/index.html`. Verified at HEAD 5b6774d1 — see the
plan's "What the code looks like today" for the observed output.
