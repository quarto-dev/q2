# Quarto 1 TypeScript format-orchestration research (Phase 0)

**Date:** 2026-07-13 (**reconstructed 2026-08-20** from the session transcript;
the original file was written by a subagent but never committed and was lost when
the worktree switched branches — findings below are faithful to that work, but
some `src/...:line` anchors should be re-verified against the checkout).
**Status:** Complete — answers the Phase 0 open questions in the epic plan.
**Companion:** [`2026-07-09-q1-filter-catalog.md`](2026-07-09-q1-filter-catalog.md)
(the Lua half; this doc is the TypeScript half — how Pandoc is actually invoked).

Reference checkout: `external-sources/quarto-cli/` (NOT version-controlled, NOT in CI).

---

## Headline: the epic's Tier-3 assumption was wrong for 4 of 5 formats

The epic assumed **dashboard, email, confluence, hugo, llms-txt** each need a custom
Pandoc Lua writer. Verified false for four:

| Format | What it actually is | Evidence |
|---|---|---|
| dashboard | **HTML-based** — `pandocTo: "html"`; behavior is a DOM postprocessor + JS/CSS deps, not a Lua Writer | `src/format/dashboard/format-dashboard.ts:313` (`pandocTo: "html"`), `:75` (`htmlFormat(8,5)`) |
| email | **HTML-based** — `emailFormat() = mergeConfigs(htmlFormat(7,5))`, no extra logic | `src/format/email/format-email.ts:5-9` |
| hugo | **Built-in gfm + option overrides** — `hugo-md` = `gfm+yaml_metadata_block+...`; site build delegated to the external `hugo` CLI; no `format-hugo.ts` | `src/resources/extensions/quarto/hugo/_extension.yml:6-26` |
| confluence | **Split** — project/preview format `confluence-html` is HTML-based; only the separate `confluence-publish` artifact uses a real custom Lua Writer (`publish.lua`, Confluence Storage-Format XML) | `_extension.yml`; `src/publish/confluence/confluence.ts` |
| llms-txt | **Not a Pandoc format** — website post-processing: render HTML, then a *second* `pandoc -f html -t gfm-raw_html` pass per page, aggregated by a project `postRender` hook | `src/project/types/website/website-llms.ts` |

**Implication:** genuine Tier-3 shrinks to `confluence-publish` (narrow, low-priority).
dashboard/email/hugo are HTML-tier (ride Q2's native HTML writer); llms-txt is website
orchestration — both leave this epic's Pandoc-hybrid scope.

## Revised format tiering

- **Tier 1 (pure Pandoc writer + Lua):** latex→`.tex`, docx, odt, jats(+variants), asciidoc,
  gfm/commonmark/commonmark_x, ipynb.
- **Tier 2 (external post-process after Pandoc):** pdf (latexmk/tectonic), beamer (same),
  context (via `pdf-engine: context`), typst (own compiler), epub (mostly Pandoc's own zip;
  cover/mediabag prep before the call), pptx (grouped with docx).
- **Tier 3 (custom Lua writer):** `confluence-publish` only.
- **Reclassified out of the epic:** dashboard, email, hugo (HTML-tier); llms-txt (website).

## The `QUARTO_FILTER_PARAMS` mechanism (the decision-relevant part)

Q1 configures its entire Lua chain through a base64-JSON blob in the
`QUARTO_FILTER_PARAMS` **environment variable**, decoded by `init.lua` which is auto-run
because Quarto overrides `--data-dir`:

1. TS: `pandocEnv["QUARTO_FILTER_PARAMS"] = encodeBase64(JSON.stringify(paramsJson))`
   (`src/command/render/pandoc.ts:~333`); `paramsJson = filterParamsJson(...)`
   (`src/command/render/filters.ts:128`).
2. `--data-dir` always overridden to `resourcePath("pandoc/datadir")` (`pandoc.ts:~1078`),
   stripping any user value. Pandoc auto-runs that datadir's `init.lua` before filters.
3. `init.lua:~596` base64-decodes the env var and **defines the global `param(name, default)`**
   that every filter reads. **Without this, `main.lua` cannot run** — `param()` is undefined.

**`active-filters` is NOT the filter-chain switch.** It is a 3-key boolean bag built at
`filters.ts:188` — `{ normalization, crossref, jats_subarticle }` — consumed by 3 sites plus
a generic `quarto.doc.is_filter_active(name)` escape hatch (`init.lua`). `configurefilters.lua:7`
passes it straight into `quarto_global_state.active_filters`.

**The real filter-chain composition is a different key, `quarto-filters`** (built by
`resolveFilters()` in `filters.ts`/`defaults.ts`), carrying `entryPoints` (user filters tagged
with one of the 8 named positions). The actual pandoc `--lua-filter`/`filters:` surface for a
Q1 render is almost always **one file** (`main.lua`) plus optionally the `citeproc` marker; all
~138 built-ins and every user filter are statically `import()`-ed inside `main.lua` and spliced
into one internal table via `inject_user_filters_at_entry_points` driven by
`quarto-filters.entryPoints`.

**The crossref gate conflation (motivates the New-4 upstream change):** `enable-crossref`
(`main.lua:226`) gates BOTH the crossref numbering/index/resolve group AND the caption
decoration in render handlers (`decorate_caption_with_crossref` returns early when false,
`floatreftarget.lua:197`). Category *registration* is separate (in `quarto-init/metainit.lua`,
not gated by `enable-crossref`). So a single flag conflates "assign numbers" and "present
numbers" — which is exactly the split the upstream contribution decouples.

## Tier-1 invocation notes (per format, condensed)

- **latex/pdf:** `latexFormat()`; `format: latex` emits `.tex` (ext wins over inner `pdf`);
  `createPdfFormat` sets `pdf-engine: lualatex`, `standalone`, KOMA template context, gated on
  `isLatexPdfEngine`. `format: pdf` rewrites `--to` to `latex` in the PDF recipe.
- **docx/odt:** `createWordprocessorFormat` — page-width/fig defaults only. **Quarto does no
  `--reference-doc` resolution** — the user value passes straight through to Pandoc. docx adds
  5 callout-icon filter params via `formatExtras`.
- **pptx:** `output-divs: false` (overrides base), fig defaults; no reference-doc, no post-step.
- **epub:** `createEbookFormat` — callout/epub CSS includes, `merge-includes: false`; Pandoc's
  writer emits the final zip. No template context.
- **jats:** template context (`template.xml` + front/authors/institution/affiliation/name
  partials); sub-article postprocessor; XML lint.
- **asciidoc/gfm/commonmark/ipynb:** plaintext/markdown defaults; gfm ≈ `commonmark+<long ext
  chain>`; `output-divs: false` for the markdown family; ipynb has an in-process notebook-JSON
  postprocessor.

## Post-processing summary (Tier-2)

pdf/beamer → latexmk/tectonic (`src/command/render/latexmk/`); context → `context` binary;
typst → own compiler + package staging (`output-typst.ts`); epub → Pandoc's zip; docx/odt/pptx →
none (delegated to Pandoc); jats/ipynb → in-process postprocessors, not separate steps.
svg→pdf image conversion is a Lua *filter* (`pdf-images.lua`), not TS — belongs to the vendored
side.

## Key consequences for the hybrid (JSON-AST handoff)

Because Q2 hands Pandoc a **JSON AST** (`-f json`), several Q1 TS steps are moot: the
markdown-temp-file, `--metadata-file`, and the always-on custom `qmd-reader.lua` all fall away.
**But** Q2's own JSON writer must then carry document-level metadata (title, date, authors) into
the Pandoc `Meta` block directly (the Meta-contract seam).

## Open questions (deferred)

1. `layoutFilterParams` / `citeIndexFilterParams` blob contributors — not enumerated.
2. `projType.filterParams(options)` per-project-type hooks — not enumerated (book/website).
3. Exact `QUARTO_FILTER_PARAMS` key list — partially enumerated above; re-derive fully in P4.
