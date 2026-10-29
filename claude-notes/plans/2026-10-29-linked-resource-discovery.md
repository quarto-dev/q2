---
title: 'Plan: auto-discover linked local files as page resources'
date: 2026-10-29
description: 'Adds an AST walk over Link targets that publishes existing in-project, non-document files through the resources channel, controllable from Lua (meta.resources) and YAML (resource-discovery).'
---

**Braid:** `bd-twfep08m` (feature). Side bug found while investigating: `bd-mejgdrlh`.
**Related:** `bd-o8pr` (project resources; plan `2026-05-03-project-resources.md`),
`bd-teh4hbli` (resources vs. sync-server upload trust boundary).
**Status:** approved 2026-10-29; Phases 0–2 implemented on
`braid/bd-twfep08m-linked-resource-discovery`; Phase 3 in progress.

Implementation notes (deviations from the text below):

- The transform takes the runtime by injection
  (`LinkResourceDiscoveryTransform::new(runtime)`, the `TitleBannerTransform`
  pattern): `RenderContext` carries no runtime.
- `ResourceOrigin::AutoDiscovery { via, source }`: the field is `via`
  because `kind` is the enum's serde tag in the render manifest.
- `Q-5-52` fails the document by returning `QuartoError::Parse` from the
  transform; `AstTransformsStage` now maps that to
  `PipelineError::Structured`, so the span resolves into `_quarto.yml` /
  `_metadata.yml` as well as front matter.
- The stale-output guard is a static rule: a linked file with a
  render-output extension (`html pdf docx pptx odt epub typ tex`) whose
  path minus extension is a project input is skipped.
- Pandoc-hybrid formats (docx/pptx/typst) run post filters inside pandoc,
  after `ResourceReportStage`, so `meta.resources` pruning cannot work
  there; the Lua docs point those users at `resource-discovery: false`.
- `-M resource-discovery:false` / `:flase` verified by hand with the CLI
  (the integration harness has no `-M` plumbing).

## Overview

A forwarded report (Connect docs migration, q2 0.38.0): a website page with

```markdown
- [Dashboard](dashboard.json){download="my-dashboard.json"}
- [Data](data.csv)
```

renders the `href`s unchanged but never copies `dashboard.json` / `data.csv`
into `_site/`. The render is silent. Quarto 1 copies them.

The port design said "use `resources:` because we can't afford an HTML
postprocessor". That stays true, and `resources:` stays the documented,
general mechanism. But it is too coarse as the *only* answer: a Markdown link
to a local file is an unambiguous, AST-visible statement that the file must
exist next to the page, exactly like an image — and we already auto-copy
images. This plan adds **auto-discovery of Link targets** on the Markdown AST
and feeds the result into the existing `resources` channel, with two
controls: a Lua escape hatch (edit `meta.resources`) and a YAML key
(`resource-discovery: false`) that merges like any other metadata.

## Current state (measured, q2 at `2ee0bd966`)

All repros are in the appendix and run with `target/debug/q2 render`.

| # | Probe | Result |
|---|---|---|
| E1 | Issue repro (`dashboard.json`, `data.csv`, `../guide/data.csv`) | Only `index.html` written. No warning. `../guide/data.csv` is normalized to `data.csv`. |
| E2 | Lua `Meta` filter edits `meta.resources` + calls `quarto.doc.add_resource` | Additions via meta **and** via API are copied. Removing an author-declared entry has **no effect** (still copied). Lua sees YAML entries as `Inlines`. |
| E3 | `resources:` at top level of `_quarto.yml` and in `sub/_metadata.yml` | Both merge into each document's `meta.resources` (array concat). Each entry resolves against the file that declared it. |
| E4 | Link-shape survey (see below) | Only the Markdown **image** is copied. No link target is. |
| E5 | `navbar: - href: files/report.pdf`; link built by a pre-filter; link URL from `{{{< meta >}}}` | None copied (Q1 copies the navbar PDF). Filter- and shortcode-produced links render correctly. |

E4 details — how each link renders today, and what discovery should do:

| Source | Rendered `href` | Should discovery copy it? |
|---|---|---|
| `frag.csv#row=3`, `query.csv?x=1` | unchanged | yes — strip `#…` / `?…` before the lookup |
| `my%20file.csv` | unchanged | yes — percent-decode before the lookup (Q1 decodes too). A literal space is a parse error (Q-2-33). |
| `other.qmd` | `other.html` | no — project document |
| `_draft.qmd` | unchanged + Q-13-4 warning | no — never publish `.qmd` source via discovery |
| `#top`, `mailto:` | unchanged | no |
| `notebook.ipynb`, `script.R`, `notes.md`, `raw.html` (none rendered) | unchanged | yes, unless the file is in the project index (see Decision 4) |
| `dir/` | unchanged | no in v1 (Q1 maps to `dir/index.html`) |
| `dir/inner.csv`, `/guide/root.csv` | unchanged / `root.csv` | yes |
| `missing.csv` | unchanged | no (nothing to copy); no warning in v1 (Decision 6) |
| `../../outside.csv` (escapes the project) | **`../outside.csv`** — silently retargeted | no. The retarget is a separate bug: `bd-mejgdrlh` |
| footnote link | unchanged | yes |
| `<a href="rawhtml.csv">` (raw HTML) | unchanged | no — raw HTML is opaque by design |
| `` `[code](code.csv)` ``, `[x]{data-href="attr.csv"}` | n/a | no — not Link nodes (per your scoping) |
| link inside an included file (`{{{< include ../partials/_part.qmd >}}}`) | unchanged, **not** rebased | resolved against the *including* page, as rendered (Q1 does the same textual include) |

### Relevant code

- `transforms/resource_collector.rs` — `ResourceCollectorTransform`
  (Finalization phase, the last transform in `AstTransformsStage`). Walks the
  AST, collects `Image` targets into `ctx.resource_copies`, and recurses into
  `Link` *content* only. The `Link` target is never looked at. This is the
  whole bug.
- `transforms/link_rewrite.rs` — `LinkRewriteTransform` (start of
  Finalization). Rewrites `.qmd`/`.md` links through the `ProjectIndex`
  (Q-13-4 on miss) and page-relativizes static hrefs.
  `navigation_href::{is_external, resolve_to_project_root}` are the helpers to reuse.
- `project_resources.rs` — `ResourceOrigin` (already reserves a commented-out
  `AutoDiscovery` variant for exactly this), `DocumentResourceReport`,
  `resolve_reported_resources` (literal paths, doc-dir anchored;
  **returns `Err` for out-of-project**, so discovery must filter first).
- `stage/stages/resource_report.rs` — `ResourceReportStage`, after
  `UserFiltersStage::post`. Diffs post-filter `meta.resources` against the
  profile snapshot. Additions → report (`LuaFilter` origin). Removals are
  ignored, by design: the "additivity defense".
- Pipeline order (`pipeline.rs`, HTML):
  `… EngineExecution … LmdExecute → UserFilters::pre → AstTransforms
  [… shortcodes … LinkRewrite … ResourceCollector] → UserFilters::post →
  ResourceReport → … ApplyTemplate`. Report entries are copied by the
  orchestrator after `post_render`. Image copies go through
  `ctx.resource_copies`, flushed per document (and early, before
  `typst compile`, via `ResourceCopyFlushStage`).

### Q1 reference

Q1 does this in an HTML postprocessor (`core/html.ts` `kHtmlResourceTags`,
`project/types/website/website-resources.ts` `resolveResourceRefs`). It covers
`a[href]`, `img[src|data-src]`, `link[href]`, `script[src]`, `embed`, `iframe`,
`source`, reveal `section[data-background-*]` and CSS `url()`. It skips
`scheme:` and `#` hrefs, percent-decodes, maps `dir/` to `dir/index.html`,
and keeps only files that exist. Because it scans the final HTML, it also
catches navbar/sidebar hrefs, template output and raw HTML. An AST walk
cannot see those.

## Feasibility of the two controls

### 1. Lua filters editing `meta.resources` — feasible, with two caveats

What already works (E2): a filter that *adds* to `meta.resources` gets the
file published. What does **not** work: *removing* an author-declared entry.
This is deliberate (bd-o8pr "additivity defense", pinned by
`filter_removing_meta_resources_does_not_drop_them`), so that a filter that
rebuilds `meta` sloppily cannot unpublish a file the author declared.

The defense works by diffing against the frozen profile snapshot. Discovered
entries are not in that snapshot, so if discovery **writes its results into
`meta.resources` before the filters that should be able to veto them**,
removal works for discovered entries with no new mechanism:
`ResourceReportStage` publishes only what is still present after the post
filters. Author-declared entries stay defended.

Caveat A — **ordering.** A filter can only remove entries that exist when it
runs. If discovery runs where I recommend (start of Finalization, see
Decision 2), **post** filters (`at: post-quarto` and later) can remove
discovered entries. **Pre** filters cannot, because the entries don't exist
yet. Pre filters can still turn discovery off by setting
`meta["resource-discovery"] = false`, since the walk reads the live metadata.
So the Lua story is "set the flag in any filter, or prune
`meta.resources` in a post filter". Alternatively, discovery could run before
`UserFilters::pre` (your original framing), so that every filter sees the
entries. That costs coverage: shortcode output and pre-filter-built links
would not be discovered. See Decision 2.

Caveat B — **attribution.** Today every non-snapshot addition is labelled
`ResourceOrigin::LuaFilter`. We'd record the discovered set on
`StageContext` (e.g. `ctx.discovered_resources`) so `ResourceReportStage`
can tag surviving entries `AutoDiscovery { kind: Link, source }` and leave
real filter additions as `LuaFilter`. This matters for the render manifest
and for diagnostics.

A small wart you'll see from Lua: YAML-authored entries arrive as `Inlines`,
while discovered ones would be plain strings. `pandoc.utils.stringify`
handles both. We document that.

### 2. A YAML key with normal metadata merging — feasible, cheap

Stages already read flags from the merged `doc.ast.meta` (e.g. `code-copy`
in `ClipboardJsStage`). E3 shows that top-level keys in `_quarto.yml` and
`_metadata.yml` merge into each document, so a document-level key gets
project, directory, document and format-level (`format: html: …`) scoping
with no extra code. Proposal:

```yaml
resource-discovery: false     # default: true
```

The boolean form is reserved to grow into a map later
(`resource-discovery: {links: false, navigation: true}`) without breaking
anyone. It does **not** turn off image copying. Images are a rendering
requirement, not discovery (see Decision 1). The key should also be added to
the schema/docs wherever frontmatter keys are registered (to confirm during
Phase 0; I didn't find a q2-side schema entry for comparable keys like
`code-copy`).

## Proposed design

A new `LinkResourceDiscoveryTransform` (AST transform, `TransformPhase::Finalization`),
inserted **immediately before `LinkRewriteTransform`** in every pipeline that
has it (HTML, revealjs, the pandoc-hybrid leg). It:

1. Returns early if merged `resource-discovery` is `false`, if there is no
   resource resolver, or in VFS-root (hub-client) mode, matching
   the gates of `ResourceCollectorTransform`.
2. Walks all `Inline::Link` nodes (same traversal as the collector, including
   footnotes, tables, captions and `Custom` slots). It reads `target.0` while
   it is still **source-relative**, before LinkRewrite page-relativizes it.
3. For each target: skip it if `is_external` or `#`-only; strip `?…`/`#…`;
   percent-decode; resolve against the doc dir (leading `/` → project root),
   via the same helper LinkRewrite uses. Then keep it only if **all** hold:
   - it is an existing regular file (`runtime.is_file`), canonicalized
     *within* the project root (symlinks included), so we never hit
     the hard `OutOfProject` error in `resolve_reported_resources`;
   - it is not under the output dir, `.quarto/`, `_freeze/`, or a dot-path;
   - it is not a `.qmd` (any) and not a project input in the `ProjectIndex`;
   - its output path does not collide with a known render output (guards a
     stale in-place `report.pdf` overwriting a freshly rendered one, since
     report copies happen after all renders).
4. Dedupes against itself and against the strings already in
   `meta.resources`. It appends survivors (doc-relative, `/`-separated) to
   `meta.resources` and records them in `ctx.discovered_resources`.

`ResourceReportStage` then classifies post-filter additions as
`AutoDiscovery` (if in `ctx.discovered_resources`) or `LuaFilter`. Copying,
the manifest and publish need no changes. `ResourceOrigin::AutoDiscovery {
kind: AutoDiscoveryKind::Link, source }` replaces the reserved comment.

This is an AST transform rather than a standalone `PipelineStage` because the
right moment is *inside* `AstTransformsStage`: after shortcodes resolve,
before LinkRewrite. "Disable the Rust stage" becomes "the transform checks
the flag and no-ops". The effect is the same, and it keeps the stage list
untouched.

## Decisions

**Resolved 2026-10-29 (review with Carlos):** 1 images stay on the
collector; 2 Finalization placement, and post-filter-only pruning gets
documented for the Lua route; 3 author entries stay defended; 4 exclusions
as proposed; 5 every format; 6 no missing-target warning; 7 the key is
`resource-discovery`. Allowed values are below (confirmed; bad values are
an error, not a warning).

### `resource-discovery` values

- **v1 accepts `true` (default) and `false` only.** The key works anywhere
  merged metadata does: front matter, `_metadata.yml`, the top level of
  `_quarto.yml`, `format: <fmt>:`, and `-M resource-discovery:false`. Normal
  merging applies, so the innermost layer wins.
- **Any other value is an error** with a new `Q-5-N` code (a docs page
  under `docs/errors/project/`) naming the key and the value, and the
  document fails to render. (Revised in review: a warning that keeps
  discovery on would let a non-strict render-then-publish copy files the
  author meant to keep out, e.g. after typing `resource-discovery: flase`.)
- **A map is reserved, not shipped.** When the navigation follow-up lands it
  becomes `resource-discovery: {links: bool, navigation: bool}`, with each
  omitted key `true` and a boolean meaning "all kinds". We don't ship the
  map form with only `links` now. A map with a single member is noise, and
  the boolean form stays valid forever, so adding the map later breaks
  nobody.
- **Known wrinkle for the map form, to settle then:** metadata merging
  replaces a value wholesale when the two layers hold different types. So a project
  `false` with a document `{links: true}` yields navigation = `true`
  (omitted key defaults to on), not the project's `false`. That is
  defensible (the inner layer is explicit), but the docs will need to say
  so.

### Original options

1. **Images: leave them on the existing collector (recommended).** You asked
   for image *and* link targets. Images are already copied by
   `ResourceCollectorTransform` via `ctx.resource_copies`, at the very end of
   the transform list (so it sees images added by any transform or pre
   filter), with Q-5-6 on a missing file. That channel must stay because
   Typst needs image bytes in the output dir *before* `typst compile`
   (`ResourceCopyFlushStage`), while `resources` entries are copied after
   the whole project renders. Mirroring images into `meta.resources` as
   well would be confusing: removing one from Lua wouldn't stop the copy.
   Discovery is therefore links-only. If you want a Lua veto for images too,
   that would be a separate change to the collector.
2. **Placement: start of Finalization (recommended) vs. before
   `UserFilters::pre`.** Finalization sees shortcode output (`[x]({{{< meta
   f >}}})`, extension shortcodes that emit download links), pre-filter links,
   callouts and other resolved custom nodes. Only post filters can prune
   entries there. Before-pre lets every filter prune, but it misses all of
   those links. I think coverage wins, because the flag covers "turn it
   off" from any filter.
3. **Author-declared removals stay defended (recommended).** Making
   `meta.resources` fully authoritative would drop the bd-o8pr defense. Your
   escape hatch only needs discovered entries to be removable, so I'd keep it.
4. **Which document-like targets to exclude.** Proposed: never `.qmd`, plus
   anything in the project index. So an un-rendered `notebook.ipynb`,
   `notes.md` or `raw.html` *is* copied, which is what Q1 does and is the
   usual "download the notebook" case. `_`-prefixed files: copy, like Q1?
   Copying `_draft.qmd` is already prevented by the `.qmd` rule.
5. **Formats.** The `resources` channel is format-agnostic (bd-o8pr decision
   8), so discovery runs for every format. Copying a linked CSV next to
   `_book/book.pdf` is harmless. Alternatively, restrict to HTML-family
   outputs to match Q1 exactly. I lean format-agnostic.
6. **No missing-target warning in v1.** The issue asks for one "if q2 does
   not copy by design". With discovery, the remaining misses are mostly
   legitimate: other-format outputs (`[PDF](index.pdf)`), generated files
   (`search.json`, `sitemap.xml`, listing feeds), post-render script
   products. A warning would be noisy. Q-13-4 already covers `.qmd`. I'd file
   a lint-level follow-up instead.
7. **Key name.** `resource-discovery` (my pick), `discover-resources`, or
   `link-resources`. Whatever we pick should read well as a future map.

## Alternatives considered

- **Extend `ResourceCollectorTransform` to Link targets, writing to
  `ctx.resource_copies`.** Smallest diff, and it makes the issue's repro
  pass. But it gives no Lua control (filters never see `resource_copies`),
  no manifest attribution, and copies happen per document rather than
  through the deduped project-level resource step. Good as a stop-gap,
  wrong as the design. Not recommended.
- **Final-HTML href scan (Q1 parity).** A tokenizer-only pass over the
  rendered HTML (no DOM, no rewriting) would be far cheaper than Q1's
  postprocessor and would also catch navbar/sidebar/footer hrefs, template
  output and raw HTML. It still contradicts the "q2 doesn't look inside HTML
  you author" contract in `docs/guides/projects/paths.qmd`. It also loses
  source locations and the Lua/metadata control, and it doesn't exist on
  the pandoc-hybrid legs. Not recommended. But see the next point.
- **Navigation hrefs (follow-up, recommended).** E5 shows that
  `navbar`/`sidebar` `href: files/report.pdf` isn't copied either. Q1 users
  do this a lot. It doesn't need HTML scanning: the nav transforms already
  resolve those hrefs from config, so they can feed the same channel as
  `AutoDiscovery { kind: Navigation }` at project scope. I propose a
  follow-up strand under the same key.
- **Per-link opt-out** (`[x](big.zip){.no-copy}` or similar). Cheap to add
  on top of this design. Maybe it's worth it in place of, or in addition to,
  Lua pruning. Not proposed for v1 unless you want it.

## Interactions and risks

- **Preview / hub-client.** VFS-root mode skips discovery (as with images).
  The single-file `q2 preview` dependency sync reads front-matter
  `resources:` from a fresh parse, not pipeline output, so discovered
  entries do **not** widen the sync-upload surface (`bd-teh4hbli` stays
  untouched). Whether linked files *resolve* in project-mode preview is a
  separate check (Phase 3).
- **Large files.** On by default means a link to a 2 GB dataset gets copied,
  same as Q1. The key or a post filter is the opt-out.
- **Incremental renders.** Discovery runs every render, so no profile
  version bump.
- **`..`-clamping bug (`bd-mejgdrlh`).** Independent of this work.
  Discovery's own containment check must not rely on
  the clamp in `resolve_to_project_root`. A target that escapes the project must
  be rejected, not silently retargeted.
- **Windows.** Decoded paths go through `Path::join`, and `meta.resources`
  strings are `/`-separated (`quarto_util::to_forward_slashes`). Cross-check
  with `--target x86_64-pc-windows-msvc`.

## Phases

### Phase 0 — Tests first

Integration tests in `crates/quarto-core/tests/integration/` (extend
`project_resources.rs` or add `linked_resources.rs` and register it in
`main.rs`), driven by fixtures modelled on the appendix repros:

- [x] Issue repro: `dashboard.json`, `data.csv`, `../guide/data.csv` land in
      `_site/guide/`; `download=` attribute untouched.
- [x] Fragment/query/percent-encoded targets are copied (file looked up
      without the tail, decoded).
- [x] Not copied: `.qmd` (rendered or `_`-prefixed), project inputs,
      missing files, out-of-project targets (no error, no copy),
      hidden paths, files under `_site/`, raw-HTML hrefs, code spans,
      attribute values, directory links.
- [x] Root-relative (`/guide/x.csv`) and footnote/table/callout links copied.
- [x] Shortcode-produced link target (`{{{< meta >}}}`) copied; pre-filter-built
      `pandoc.Link` copied.
- [x] `resource-discovery: false` in front matter, in `_metadata.yml`, at the
      top level of `_quarto.yml`, and under `format: html:` each disable it;
      a document-level `true` overrides a project-level `false`;
      `-M resource-discovery:false` disables it; a non-boolean value
      (`resource-discovery: "no"`, `flase`, a map) is an error with the
      new code, and nothing is discovered or copied for that document.
- [x] Post filter removing a discovered entry from `meta.resources` →
      not copied; removing an author entry → still copied (existing
      defense); pre filter setting `resource-discovery = false` → nothing
      discovered.
- [x] Render manifest lists discovered files with `AutoDiscovery` origin;
      filter additions keep `LuaFilter`.
- [x] Stale-output guard: `report.pdf` in source next to a `report.qmd` that
      also renders to PDF is not copied over the fresh output.
- [x] Unit tests for the URL → candidate path normalization (strip tail,
      decode, root-relative, escape rejection) and for
      `ResourceReportStage` classification.

### Phase 1 — Implementation

- [x] `ResourceOrigin::AutoDiscovery { kind, source }` + `AutoDiscoveryKind::Link`;
      update `resolve_reported_resources` and every exhaustive match.
- [x] `LinkResourceDiscoveryTransform` + `ctx.discovered_resources`
      (on `RenderContext`, plumbed through the `StageContext` swap like
      `resource_copies`).
- [x] Register before `LinkRewriteTransform` in each pipeline builder.
- [x] `ResourceReportStage` classification.
- [x] `resource-discovery` key read from merged meta (boolean; anything
      else is an error); allocate the `Q-5-N` code and its docs
      page; schema entry if applicable.

### Phase 2 — Docs

- [x] `docs/guides/projects/paths.qmd`: say that Markdown links to local files
      are copied, list what is and isn't discovered (raw HTML, attributes,
      code, nav config until the follow-up lands), and document
      `resource-discovery` and the post-filter pruning recipe.
- [x] Lua API docs: `meta.resources` semantics (additions publish;
      discovered entries are prunable **only in post filters** (`at:
      post-quarto` or later), because they don't exist yet when pre filters
      run; author entries are never prunable). Include a worked post-filter
      example, and the pre-filter alternative
      `meta["resource-discovery"] = false`. Note that entries may be
      `Inlines` or strings, so use `pandoc.utils.stringify`.
- [x] ~~Changelog entry.~~ n/a: the only changelog (`hub-client/changelog.md`) covers hub-client-facing changes, and discovery is a no-op in hub-client (VFS-root mode).

### Phase 3 — Verification

- [ ] `cargo xtask verify`; Windows cross-clippy for touched crates.
- [ ] Re-run the appendix repros; check `q2 preview` project mode serves a
      linked CSV.
- [ ] File follow-ups: navigation hrefs (`AutoDiscovery { kind: Navigation }`),
      missing-target lint, optional per-link opt-out.

## Appendix — repros

All are self-contained website projects; render with `q2 render` from the
project directory. (Built under the session scratchpad; recreate anywhere.)

**E1 — issue repro**

```text
_quarto.yml        project: {type: website}; website: {title: t}
index.qmd          [guide](guide/index.qmd)
guide/index.qmd    - [Dashboard](dashboard.json){download="my-dashboard.json"}
                   - [Data](data.csv)
                   - [From home](../guide/data.csv)
guide/dashboard.json, guide/data.csv
```

Result: `_site/guide/index.html` only.

**E2 — Lua and `meta.resources`** (`sub/index.qmd` declares
`resources: [declared.txt, removed.txt]` and `filters: [filter.lua]`)

```lua
function Meta(meta)
  local out = pandoc.List()
  for _, r in ipairs(meta.resources or {}) do
    if pandoc.utils.stringify(r) ~= "removed.txt" then out:insert(r) end
  end
  out:insert("added-by-meta.txt")
  meta.resources = out
  quarto.doc.add_resource("added-by-api.txt")
  return meta
end
```

Result: `declared.txt`, `removed.txt`, `added-by-meta.txt` and `added-by-api.txt`
are all copied. The removal was ignored.

**E3 — merging.** Top-level `resources: [top.txt]` in `_quarto.yml` and
`resources: [dirmeta.txt]` in `sub/_metadata.yml`. A filter printing
`meta.resources` shows `top.txt` on every page and `top.txt, dirmeta.txt` on
`sub/index.qmd`. Output contains `_site/top.txt` and `_site/sub/dirmeta.txt`.

**E4 — link-shape survey.** `guide/index.qmd` with every row of the E4 table
(frag/query, `%20`, `.qmd`, `_draft.qmd`, `#top`, `mailto:`, `.ipynb`/`.R`/`.md`/`.html`,
`dir/`, `dir/inner.csv`, `/guide/root.csv`, `missing.csv`, `../../outside.csv`,
footnote, raw `<a>`, code span, `data-href` attribute, an include from
`../partials/`) and one `![img](img.png)`. Only `_site/guide/img.png` is copied.

**E5 — nav and generated links.** `navbar: left: [{href: files/report.pdf}]`,
a pre filter that turns paragraph `MARK` into `pandoc.Link(…, "pre.csv")`, and
`[x]({{{< meta link >}}})` with `link: sc.csv`. Hrefs render correctly. None of
the files are copied.
