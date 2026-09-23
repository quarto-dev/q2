# P4 — Vendored-Q1 run machinery (transport)

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see `git log --oneline -- claude-notes/plans/2026-08-20-pandoc-hybrid-P4-run-machinery.md`
for the full correction history. Latest (round 4 review, three dispatched Opus reviewers):
corrected the "Q2 owns presentation defaults" mechanism, which set only `crossref-<type>-title`
while the reference-text path (`refPrefix()`) reads a different, unfed `-prefix` param — verified
directly against Q1's real `crossref/format.lua`, this makes theorem-family cross-references
render "thm. 1" instead of "Theorem 1" in both shipping v1 formats; corrected the shim splice from
"one line" to "two lines in two regions" and fixed its placement (inside the vendored tree, not
"alongside" it); extended the `pandoc`-nonzero-exit stderr policy to capture unconditionally (a
successful render currently drops every Q1 `warn()`, including the epic's own documented
`order`-missing degradation) and picked subsystem number 18; put the error-docs pages/sidebar
obligation explicitly on the checklist; measured a real pandoc-version mismatch (vendored Lua
implies pandoc 3.10, CI runs 3.8.3, dev-setup floors at 3.6) and added a checklist item to fix it;
relabeled the env-block size-bound item as still-open, not closed. (Prior pass, same day: an
implementation-feasibility review found five wiring gaps — two vendoring roots not one; the
shim's loading mechanism; the binary-output data contract; no error-catalog subsystem; unpinned
base64/no size bound — closed with concrete decisions, see the "Finding" section below.)
**Status:** Complete (2026-09-19) — see the Coarse checklist below and the implementation ledger.
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P4-implementation.md`](2026-09-18-pandoc-hybrid-P4-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
Make Q1's vendored `main.lua` **runnable at all** from Q2, and prove the transport
(JSON AST → pandoc → vendored Lua → bytes). This is the "does it run" layer (independent of
whether output is *correct* — that's P5–P7).

## `QUARTO_FILTER_PARAMS` re-derivation (the three deferred open questions — resolved)

`filterParamsJson` (`filters.ts:128-201`) builds the whole blob by spreading ~15 contributor
calls into one object, then adding a handful of top-level literal keys. Full enumeration,
grouped by v1 (single-document docx/pptx, no `_quarto.yml` project) relevance:

| Contributor | Keys contributed | Relevant to docx/pptx v1? |
|---|---|---|
| `extractIncludeParams` | `include-in-header`/`include-before-body`/`include-after-body` + smart-include text/file merges | **Yes** — generic, format-agnostic |
| `initFilterParams` | none returned (side effect: sets `QUARTO_FILTER_DEPENDENCY_FILE` env, Windows codepage) | Yes, but it's an env/side-effect step, not a param key |
| `ipynbFilterParams` | `ipynb-title-block-template`; `enable-crossref: false` + `ipynb-produce-source-notebook: true` when that render flag is set | No — ipynb-only |
| `projectFilterParams` | delegates to `projType.filterParams` (see Q2 below) + `project-output-dir`/`project-offset` when `options.project` is set | **Partially** — the two additional keys only fire under a real `_quarto.yml` project; N/A for a bare single-doc render |
| `quartoColumnParams` (`extractColumnParams`) | `reference-location: margin`, `citation-location` | No — HTML/typst margin-notes feature |
| `quartoFilterParams` | ~28 keys (corrected 2026-09-17 — actual enumerated count, not "~20"): `output-divs`, `mediabag-dir`, `fig-align`/`fig-pos`/`fig-env`, `code-fold`, `html-table-processing`, `use-rsvg-convert`, `tbl-colwidths`, `shortcodes` (+extension-contributed), `html-math-method`, `fig-responsive`, `output-location`, `code-line-numbers`, `keep-hidden`, `remove-hidden`, `clear-hidden-classes`, `unroll-markdown-cells`, `clear-cell-options`, `cite-method`, `pdf-engine`, `has-bootstrap`, `has-resource-path`, `quarto-source`, `quarto-profile`, `quarto-version`, `quarto-cli-path`, `code-annotations` | **Yes, the core set** — format-render/metadata derived, no project dependency. **Reads `options.project.isSingleFile` unconditionally** (`filters.ts:663`) — see the "synthetic project" note below |
| `crossrefFilterParams` | `listings`, `number-sections`, `number-offset`, `number-depth`; `crossref-index-file` only when **not** `options.project?.isSingleFile` | **Partially** — the first four are format-agnostic and relevant; `crossref-index-file` is project-scoped (cross-document index), N/A for v1 |
| `citeIndexFilterParams` | `cites-index-file`, only when `options.project && projectIsBook(...)` | **No for v1** — book-project-only |
| `layoutFilterParams` | `page-width` (if set), `adaptive-text-highlighting`/`text-highlighting` (bool, if the format has adaptive/text highlighting) | Yes — format-derived, no project dependency. **Corrected 2026-09-17:** only `page-width` is `format.render`-derived; the other two keys are derived from the separate `defaults: FormatPandoc` parameter (`hasAdaptiveTheme(defaults)`/`hasTextHighlighting(defaults)`) — doesn't change relevance/scope, but P4's params-blob builder needs both sources plumbed in, not just `format.render`. |
| `languageFilterParams` | `code-summary`, `toc-title-document`; per-category `crossref-{fig,tbl,lst,thm,lem,cor,prp,cnj,def,exm,exr}-prefix`; every `format.language` key starting with `callout-`/`crossref-`/`environment-` | **Yes — required**, not optional. This is where the localized "Figure"/"Table"/theorem-family title strings Q1's `crossref.categories` init reads come from (`mainstateinit.lua`). **Source: Q2's own `RefTypeRegistry`, not a relay of Q1's locale files** — decided with Gordon 2026-09-17, see the "Q2 owns presentation defaults" finding below. |
| `jatsFilterParams` | `jats-subarticle-id`, only when `jats-subarticle` metadata is set | No — JATS-only |
| `notebookContextFilterParams` | `notebook-context`, only when notebooks are present | No — engine/notebook-execution feature |
| `filterParams` (caller-supplied, e.g. format's own `formatExtras`) | format-specific extras — docx contributes 5 callout-icon params per the TS research doc's Tier-1 notes | **Yes** — this is the one docx-specific contributor; P7 (per-format tail) owns deriving these, P4 just needs to plumb an extension point for them |
| `extractCustomFormatParams` | `quarto-custom-format`, only if metadata sets it | No — custom-format extension feature |
| `extractTypstFilterParams` | typst-specific | No — typst only |
| top-level literals | `results-file` (metadata path), `quarto-filters` (filter spec — **N/A**, see below), `active-filters` (`{normalization, crossref, jats_subarticle}` bag), `format-identifier`, `is-shiny-python`/`shiny-python-exec`, `execution-engine`, `brand`, `quarto-environment` (`{paths: {Rscript, TinyTexBinDir, Typst}}`) | Mixed — see notes below |

### Open question 1 — `layoutFilterParams`/`citeIndexFilterParams` (resolved)

Both are small and fully deterministic, not "not enumerated" as the research doc left them:
`layoutFilterParams` (`layout.ts:23-43`) is 3 keys, purely `format.render`-derived, no project
dependency at all. `citeIndexFilterParams` (`project-cites.ts:22-33`) is 1 key
(`cites-index-file`), gated entirely on `projectIsBook(options.project)` — for any non-book
render (which is every v1 target) it returns `{}`. **No further work needed**; the table above
is the enumeration.

### Open question 2 — `projType.filterParams(options)` hooks (resolved)

Three project types implement the hook (the `filterParams` signature is declared at
`project/types/types.ts:62` — corrected 2026-09-17; a prior pass cited `project/types.ts:62`,
a different, nested file that only holds `isSingleFile`/etc., not the hook signature):

- **book** (`book.ts:155-173`) — only fires if `options.format.extensions?.book` is set;
  contributes `crossref-resolve-refs: false` for multi-file book formats (defers ref resolution
  to a later cross-chapter merge step) or `single-file-book: true` otherwise, plus whatever the
  specific book-extension's own `filterParams` adds.
- **website** (`website.ts:395-410`) — contributes `draft-mode`/`drafts` (absolute paths), only
  when draft config or draft mode is active; otherwise returns `undefined`.
- **manuscript** (`manuscript.ts:359-397`) — contributes `manuscript-url`/`notebook-links` when
  manuscript-specific metadata is present.

**All three are gated on `options.project` matching that specific project type.** For a
standalone single-document render (no `_quarto.yml`, or a project of a different type), **none
of the three hooks fire at all** — `projectFilterParams` (`filters.ts:498-526`) falls through to
`{}`. **Recommendation: treat all three as out of scope for this epic's v1** (docx/pptx,
single-document). Q2 doesn't have book/website/manuscript project-mode Pandoc-hybrid rendering
in scope yet (the website project epic's `DocumentProfile` work is a separate, later
initiative per this repo's CLAUDE.md) — revisit this table's book/website/manuscript rows only
when/if that becomes real. Nothing here blocks P4's transport smoke.

### Open question 3 — exact `QUARTO_FILTER_PARAMS` key list (resolved — see table above)

**Recommendation for what P4 actually needs to build for the transport smoke and for
docx/pptx v1**, in priority order:
1. `quartoFilterParams`'s ~28 keys (corrected 2026-09-17, was "~20" here too — the table above
   was fixed earlier this pass but this recommendation-list copy wasn't, the same
   never-landed-fix pattern this epic keeps rediscovering) (the core, format-agnostic set) — required.
2. `languageFilterParams`'s crossref/callout/environment title strings — required (not
   optional; without them Q1's category init has nothing to read for caption prefixes).
   **Sourced from Q2's `RefTypeRegistry`** (see the finding below), not from Q1's own vendored
   locale YAML — this is the mechanism, decided with Gordon, that resolves P5's Theorem-`kind`
   question and the general "who owns presentation defaults" policy question.
3. `crossrefFilterParams`'s 4 non-project keys (`listings`, `number-sections`,
   `number-offset`, `number-depth`) — required once P3's `crossref-numbering` param is added
   alongside them.
4. `extractIncludeParams`'s include-file plumbing — required (generic).
5. The top-level literals `results-file`, `format-identifier`, `execution-engine`,
   `quarto-environment` — required (structural, not format-specific).
6. Everything else in the table (ipynb, columns, custom-format, typst, jats, notebook-context,
   book/website/manuscript project hooks) — **explicitly deferred**, each for the reason given
   in its row. Don't build stubs for these; a missing key reads as Lua's own default (most
   `param()` calls in the vendored source pass an explicit default), so omission is safe, not
   silently wrong.

**The `quarto-filters` note the table above points to (added 2026-09-17 — the table's "N/A, see
below" pointed at nothing; found by an epic-wide review, I9).** `quarto-filters` carries the
filter-spec Q1 TS uses to splice **user Lua filters** into `main.lua`'s pipeline via
`inject_user_filters_at_entry_points`. Q2 has its own, independent user-filter mechanism
(`UserFiltersStage::pre()`/`post()`, bracketing the transform pipeline) and doesn't run user
filters through Q1's Lua at all — so this key is genuinely N/A, not a gap. **But this has a
consequence worth stating explicitly, not just implying by omission:** for a `--to docx` render,
`UserFiltersStage::post()` runs on the **pre-cut** AST — full of `__quarto_custom_node` wrappers,
not the rendered shapes an HTML render (or Q1 itself) would show a filter. A user filter matching
on `Div`/`Figure` will see something different from both Q1's behavior and Q2's own HTML path.
Declaring this out of scope for v1 is defensible (matches the epic's own docx/pptx-golden-parity
framing, which implicitly assumes filter-free fixtures), but it needs to be **declared**, not
left implicit — see design doc §8 for the corresponding cross-cutting-decisions note.

**`active-filters`: a static echo happens to be correct, but the reasoning below was wrong about
why — corrected 2026-09-17 (epic-wide review, I12).** `crossrefFilterActive()`
(`crossref.ts:27-29`, feeds the bag's `crossref` field) reads a *different* metadata key
(`format.metadata.crossref !== false`) than the `enable-crossref` param P3 audited — a second,
independent flag.

This plan originally grepped only for the *accessor* (`is_filter_active`) and found zero matches,
concluding "no built-in Q1 filter reads this bag at all." **That conclusion doesn't survive a
grep for the underlying *field*.** `quarto_global_state.active_filters` (populated directly from
this param by `quarto-init/configurefilters.lua:7`) is read directly, bypassing the accessor
entirely, at three real sites in built-in code: `main.lua:266-269` (`.normalization`),
`crossref/crossref.lua:154-157` (`.normalization` again), and `main.lua:475-486`
(`.jats_subarticle`, two branches). This also directly contradicts the filter-catalog research
doc, which correctly says "consumed by 3 sites" — this plan silently asserted the opposite
without reconciling against it, which the epic's own "treat research as a starting index, not
gospel" principle covers for *correcting* the research, not for contradicting it unremarked.

**The practical conclusion is unaffected: P4's proposed static echo
(`{normalization: true, crossref: true, jats_subarticle: false}`) still works**, because none of
the three real call sites need `.normalization`/`.jats_subarticle` to vary for a docx/pptx v1
render (normalization always runs; jats_subarticle is JATS-only, never true for this epic's
targets) — but it's a **behavioral choice that happens to be correct**, not an "echo for API
compatibility, nothing reads it" shim. Recorded accurately for whoever revisits this if a future
format needs `active-filters` to actually vary.

### The `options.project` non-nullability (implementation note for P4/P7)

`quartoFilterParams` dereferences `options.project.isSingleFile` **unconditionally**
(`filters.ts:663`, no optional-chaining), and the `ProjectContext`/`PandocOptions` types
(`project/types.ts:140,178`) declare `isSingleFile: boolean` as non-optional. Q1 TS never treats
a standalone render as project-less — it always synthesizes a project wrapper with
`isSingleFile: true` for a bare file render. **P4's invocation builder needs the equivalent**: a
minimal synthetic "project" value (at least `is_single_file`/`dir`) passed into the params-blob
builder even when Q2 has no real project, so the `quartoFilterParams`/`crossrefFilterParams`
logic above has something non-null to branch on. This is a small, concrete requirement the prior
draft's "build `QUARTO_FILTER_PARAMS` in Rust" bullet didn't call out.

## Finding — Q2 owns presentation defaults; the mechanism is `crossref-<type>-title` params, not constructor fields

Resolves P5's last open design question (the "constructor default-passthrough policy") and
generalizes it into a standing principle, decided with Gordon 2026-09-17.

**The census behind this** (full detail in P5): across all five Route-R types, every field
where Q1 has its own independent default-computation logic turned out to already be dead code
for Route R *except one* — Theorem's `kind`/display-title. Q2 already fully resolves
`active`/`appearance`/`icon`-style fields in Rust before the wire cut, so Q1's parallel
defaulting for those never fires. `kind` was the one live exception: Q2's `RefTypeRegistry` and
Q1's `theorem_types`/`crossref-<type>-title` locale path are two *independently maintained*
systems that happen to agree today (in English), with no construction forcing them to keep
agreeing.

**The mechanism is not the constructor.** The obvious-looking fix — pass Q2's `kind` through as
`theorem.lua`'s `name` field — is wrong: reading `captionPrefix` (`crossref/theorems.lua:79-90`,
the function that builds the caption for every non-LaTeX/Typst/JATS format, i.e. docx/pptx too)
shows `name` is an **optional parenthetical suffix** ("Theorem 1 (My Special Title)"), never a
replacement for the type label. Forcing `kind` through it would render "Theorem 1 (Theorem)" —
visibly wrong, not just architecturally impure.

**The actual label comes from `title(type, theoremType.title)`** (`crossref/format.lua:4-7`):
```lua
function title(type, default)
  default = param("crossref-" .. type .. "-title", default)
  return crossrefOption(type .. "-title", stringToInlines(default))
end
```
It checks the `crossref-<type>-title` **param first**, falling back to the static
`theorem_types`/`crossref.categories` default only if unset — an override channel Q1 already
built for its own i18n. **So the mechanism is: P4's params-blob builder sets
`crossref-<ref_type>-title` from Q2's `RefTypeRegistry` entry for every registered type**
(already localized via `RefTypeRegistry::localize_builtin_display_names`), overriding whatever
Q1's own locale files would otherwise supply — no shim-side special-casing, no constructor
change, and it generalizes for free to every registered ref-type (Callout categories, Float
kinds), not just Theorem.

**The standing principle (decided with Gordon), not just a Theorem fix:** Q2 owns any
presentation default it has an opinion on, now or in the future; Q1's own defaulting is only
ever load-bearing for a field Q2 has no data for at all. Concretely: today, Callout's icon
*glyph identity* (e.g. `fa-info`, per-type color, `callout.lua:245-272`) is Q1-only — Q2 has no
field for it, so Q1's default is the only option and stays in force. **If Q2 ever grows its own
icon-customization feature**, that field graduates the same way `kind` just did: Q2 becomes
authoritative for it, via whichever channel is appropriate then (a param override if one exists,
an upstream request if it doesn't) — not deferred to Q1's default merely because that's what
existed first. This is the resolution to record in P5 in place of "design a general policy":
there is no separate policy to design, just this one rule, applied per field as Q2 grows fields.

**Critical correction (2026-09-18, round 4 review — found independently by two dispatched
reviewers plus a third confirming pass): the mechanism above sets only `-title`, but this
principle's own downstream consumer — the cross-*reference* text, not the caption — reads a
different, unfed param.** `title()` (`crossref/format.lua:4-7`, quoted above) feeds *captions*
(`titlePrefix`/`captionPrefix`). But a resolved `@thm-x`/`@fig-x` reference (Route N,
`CrossrefResolvedRef`) goes through `refPrefix(type, upper)` instead
(`crossref/format.lua:66-79`), which reads `param("crossref-" .. type .. "-prefix")` **first**,
falling back to `crossref.categories.by_ref_type[type].prefix` and only then to a bare
`type .. "."` literal — it never reads `-title` at all. Q1's own orchestrator
(`filters.ts:465-494`, read directly) sets **both** families: it derives all 11
`crossref-<type>-prefix` keys from `language["crossref-<type>-title"]` for the theorem-family
types, plus copies every `format.language` key already starting with `crossref-` (the bulk
`-title`/`-prefix`/`-eq`/`-sec`/`-ch`/`-apx` copy). **Without the `-prefix` family, `refPrefix()`
falls through to its bare-literal default for every type `crossref.categories.by_ref_type` has no
entry for — which is every theorem-family type (`thm, lem, cor, prp, cnj, def, exm, exr`) plus
`eq`/`sec`, since `crossref.categories.all` has no theorem entries at all (confirmed by P6
Finding 1's own table).** Concretely: `@thm-pythagoras` renders as **"thm. 1"**, not "Theorem 1",
in both shipping v1 formats — and for `fig`/`tbl`/`lst` (which *do* have a `by_ref_type` entry),
the caption localizes via `-title` while the reference stays in Q1's hardcoded English
`category.prefix`, producing a within-document inconsistency in any non-English render. This is
not a hypothetical: verified `refPrefix`'s real body directly, and confirmed
`crossref.categories.all` (`mainstateinit.lua:32-119`) carries no theorem entries.
**Fix: emit both `crossref-<ref_type>-title` and `crossref-<ref_type>-prefix` from
`RefTypeRegistry`, for every registered type**, mirroring `filters.ts:484`'s own "derive `-prefix`
from `-title`" pattern — same source, same loop, one extra key per type.

## Finding — five wiring gaps closed (2026-09-18, implementation-feasibility review)

An implementation-feasibility review found several load-bearing gaps in this plan's "In scope"
list — none are new functionality, all are things the plan already promises but didn't specify
concretely enough to implement without inventing an answer.

1. **Vendoring is two roots, not one, and `include_dir!` doesn't materialize files to disk.**
   `main.lua` lives under `src/resources/filters/`, but `init.lua` (which this plan already lists)
   lives in a *different* tree, `src/resources/pandoc/datadir/`, and requires siblings there
   (`_base64.lua`, `_json.lua`, `_format.lua`, `_utils.lua`, `logging.lua`, `lpeg*.lua`,
   `readqmd.lua`) — so vendoring needs **two** roots: a datadir root (for `--data-dir`) and a
   filters root (for `-L main.lua`). Separately, `include_dir!` embeds bytes into the binary; it
   does not extract them to disk, and `main.lua` resolves every `require`/`dofile` via
   `PANDOC_SCRIPT_FILE:match(...)` — i.e. it needs **real files on disk** at invocation time. This
   repo already has the right primitive for this: `quarto_core::resources::ResourceBundle`
   ("extracted to a temporary directory on first access," `crates/quarto-core/src/resources.rs:
   25-43, 359-452`). Use it for both roots; decide temp-per-process vs. a cached versioned
   directory (matters for repeated renders and for a future `q2 preview`-equivalent, if one is
   ever built for Pandoc targets — not planned now).
2. **The shim needs a loading mechanism, and it's simpler than it first looks.** Route R needs
   `quarto.Callout`/`quarto.Theorem`/etc., which only exist inside `main.lua`'s own Lua
   interpreter state — a separate `pandoc --lua-filter shim.lua --lua-filter main.lua` pass
   **cannot work** (separate filter files get separate Lua states in pandoc's filter chain).
   Read `main.lua`'s actual filter-chain assembly (lines 709-727): `quarto_filter_list` is built
   by `tappend`-ing a sequence of named filter-group tables (`quarto_init_filters`,
   `quarto_normalize_filters`, `quarto_pre_filters`, `quarto_crossref_filters`,
   `quarto_layout_filters`, `quarto_post_filters`, `quarto_finalize_filters`), then (line 735)
   `inject_user_filters_at_entry_points` splices in any **end-user**-declared
   `quarto-filters.entryPoints` filters by name. **The shim is not an end-user filter and doesn't
   need that indirection at all** — P4 owns vendoring `main.lua` in the first place, so the
   natural mechanism is the same kind of small, marked patch P3 already uses for its 3-file
   crossref change: **two lines in two regions, not one line** (corrected 2026-09-18, round 4
   review, Reviewer A — the original text below said "add one line"; `quarto_pandoc_shim_filters`
   is a Lua global that exists only once the shim file has itself been `import()`ed, and
   `main.lua` loads every sibling file through its own `import()` helper
   (`main.lua:8-11`, `dofile`-based, resolved relative to `PANDOC_SCRIPT_FILE`'s directory) —
   called from a block of ~45 `import("./…")` lines at `main.lua:13-60`. So the patch needs an
   `import("./<shim>.lua")` line there **in addition to** the `tappend(quarto_filter_list,
   quarto_pandoc_shim_filters)` line at the splice point below): `tappend(quarto_filter_list,
   quarto_pandoc_shim_filters)`, requiring a **new**, non-upstream Lua file (P5's shim). **The
   shim file must live *inside* the vendored filters tree, as a sibling to `customnodes/*.lua`
   (matching P5's own wording), not "alongside" the `v1.11.3` tree as this Finding originally
   said** — `import()`'s path resolution is relative to `PANDOC_SCRIPT_FILE`'s own directory, so a
   file placed outside that tree needs `package.path` setup nobody has scoped; only the
   inside-the-tree placement works with the mechanism as described. Because the shim lives inside
   the pinned tree, a future re-vendor ("bump the pinned tag, re-run Layer-1/2") must explicitly
   carve the shim file out of a delete-and-recopy, or it gets deleted with the rest of the
   `v1.11.3` tree. **Position: between `quarto_init_filters` and `quarto_normalize_filters`**
   (i.e. right after line 712, before line 713) — the shim must convert wire-format
   `__quarto_custom_node` Divs into real Q1 scaffold objects *before* any of Q1's own
   content-detecting passes (`parse_floatreftargets`, `code_filename()`, etc., all in
   `quarto_normalize_filters`/`quarto_pre_filters`) get a chance to walk into their still-raw
   slot contents. **Verified independently in round 4 review (Reviewer B) that this position is
   load-bearing in *both* directions, not just convenient:** after `quarto_init_filters` is
   mandatory because `crossrefOption()` (which Route N calls) indexes `crossref.options`, which is
   `nil` until `init_crossref_options(meta)` runs inside `quarto_init_filters`'s first entry — a
   shim spliced any earlier would hard-crash on its first call; before `quarto_normalize_filters`
   is mandatory because the wire wrapper's `Div` **retains its original semantic classes**
   (`callout`, `callout-note`, `panel-tabset`, etc. — `pampa`'s writer only *prepends*
   `__quarto_custom_node`, it doesn't replace the class list), and at least three Q1
   `parse()`-keyed handlers (`Callout`, `Tabset`, `ConditionalBlock`) key on exactly those classes
   inside `quarto_normalize_filters`; splicing before it means the shim has already replaced the
   wire Div with a class-less Q1 scaffold by the time those handlers would otherwise misfire on
   it. **Anchor this patch by the named group boundary (`quarto_init_filters` /
   `quarto_normalize_filters`), not a line number**, in the marked-patch comment — `main.lua`'s
   group *contents* have been actively refactored for performance twice in the last two years
   (a 2024-11 traversal-engine migration touching every group table, a 2025-01 "combine finalize
   filters" merge), even though the top-level group *ordering* itself has been stable since 2023.
   This also resolves P4's own prior "`quarto-filters` is genuinely N/A" verdict: that verdict is
   still correct for *end-user* filters (Q2 runs those itself), it just isn't the mechanism for
   *our own* shim, which was never in scope for that param.
3. **Binary output needs a place in the pipeline data contract.** `RenderedOutput.content: String`
   (`stage/data.rs:449`) and the only file-render entry point,
   `render_qmd_to_html` → `RenderOutput { html: String, .. }` (`pipeline.rs:166-173,843`, called
   from `render_to_file.rs:358`), both assume text. docx/pptx are ZIP containers. **Decision:**
   `PandocWriteStage` writes the output file itself (the stage owns the `pandoc` subprocess and
   its stdout/exit code) and returns a `RenderedOutput` with an empty `content` and a populated
   output-path field — it does not thread bytes through `PipelineData`/`RenderedOutput` at all.
   This is the smallest change (no `Vec<u8>` ripple through every HTML stage or the wasm build)
   and matches how a subprocess-driven stage naturally behaves. **A new plan-list item, not
   previously owned by anyone:** P4 owns naming this entry point and its signature (a sibling to
   `render_qmd_to_html`, e.g. `render_qmd_to_pandoc`), and the Pandoc-leg **stage list** itself
   (see P1's new stage-exclude-list checklist item — the *transform* exclude-list is P1's, the
   *stage* list assembly that plugs `PandocWriteStage` in is P4's, since P4 is the plan that
   introduces the stage).
4. **Error handling gets one epic-wide decision: a new `pandoc` catalog subsystem.** Nothing in
   P1–P8 names a `Q-*` subsystem or error type for anything in this epic, and the catalog's 15
   existing subsystems (`yaml, markdown, writer, listing, xml, cli, navigation, template, project,
   include, internal, theme, lua, crossref, extension`) have no natural home for "pandoc binary
   missing," "pandoc exited nonzero," "unrecognized wire `type_name` reached the shim," or a
   Route-R constructor crash (e.g. the already-known nil-`type` Proof crash). **Decided:** new
   subsystem `pandoc`, **number 18** (2026-09-18, round 4 review — the 15 existing subsystems
   occupy `{0,1,2,3,5,7,9,10,11,12,13,14,15,16,17}`; gaps at 4/6/8 are undocumented as
   retired-or-reusable, so `18` is the unambiguous next number, not a judgment call between a gap
   and the top). This repo's lint rules require **two** things in the same commit as the first
   code, not one: a `docs/errors/pandoc/<code>.qmd` page (`error-docs-page-missing`) **and** a new
   `- section: "pandoc"` block in `docs/_quarto.yml`'s errors sidebar (`error-docs-sidebar-unlisted`
   — this is the exact scenario that rule was created for: `crossref`/`extension` once had no
   `- section:` block at all, making every one of their pages unreachable by navigation). The
   single most load-bearing case to specify: **when the `pandoc` subprocess exits nonzero**, wrap
   its stderr verbatim in the diagnostic (this is where a Lua traceback from a Route-R constructor
   crash, or a missing-filter error, will actually surface — treat stderr passthrough as the
   primary debugging channel, not an afterthought) and retain the temp JSON input for debugging
   rather than deleting it on failure.

   **Critical correction (2026-09-18, round 4 review — found independently by two dispatched
   reviewers): stderr-on-nonzero-exit-only silently drops the epic's own most-cited failure
   mode.** P3's audit table documents, in its own words, that a missing `order` on a labeled node
   "silently drops the caption prefix **with a warning** rather than erroring" — verified: the
   nil-guards at `crossref/tables.lua:229` and `floatreftarget.lua:218`/`272` are `warn()`-and-skip
   inside functions that have already passed their gate, i.e. they fire on a **zero-exit** render.
   The Finding above only specifies stderr capture "when the `pandoc` subprocess exits nonzero" —
   so every `quarto.warn()`/`quarto.error()` call from any vendored filter, on a successful render,
   currently has no described destination. This is also the only delivery channel for P5's
   unrecognized-`type_name` warning (P5's error-handling case 1 explicitly assumes a
   `Q-<pandoc>-*` warning gets emitted somewhere, but an unrecognized type is *by construction* a
   successful render, since the whole point of that case is not hard-failing). **Fix: capture the
   `pandoc` subprocess's stderr unconditionally, not only on nonzero exit, and re-emit any
   `[WARNING]`-shaped line through the `pandoc` catalog subsystem** (as a diagnostic, not a
   render failure) — this single change rescues P3's own documented degradation path, P5's
   unrecognized-type-name warning, and any other vendored-Q1 `quarto.warn()` call, all at once, and
   is exactly the kind of channel the repo's existing `Q-11-1` "Lua Filter Diagnostic — A
   diagnostic was emitted by a Lua filter via `quarto.warn()` or `quarto.error()`" code already
   describes (currently unclaimed by any actual emitter) — consider routing through that existing
   code rather than a new one where the shape matches.
5. **Two small pins the plan didn't state:** (a) Q1 encodes `QUARTO_FILTER_PARAMS` as
   **standard-alphabet, padded** base64 (`pandoc.ts:315,340` writer, `datadir/init.lua:596`
   decoder) — a Rust implementation must match this exact variant (not `URL_SAFE` or
   `STANDARD_NO_PAD`) or the vendored Lua **silently fails to decode — and this is not a crash**,
   verified 2026-09-18 (round 4 review, Reviewer C): the decoder failure means `param()` returns
   its caller-supplied default for *every* key, so the render **succeeds** with no crossref
   decoration, external-numbering mode never activated, and every `crossref-*-title`/`-prefix`
   override silently inert — a plausible-looking docx with none of this epic's numbering actually
   applied. Nothing in this plan currently asserts the blob decoded; add a cheap check (the shim
   asserts one sentinel param round-trips; the transport smoke checks it) rather than relying on
   "get bytes" alone, which this exact failure mode satisfies. (b) `extractIncludeParams`
   embeds include-file **text** into the blob, and Windows caps the whole environment block at
   32,767 characters; a modest `include-in-header` file plus base64's 4/3 expansion can approach
   that with no fallback on Q1's side either. **This remains an open decision, not a closed one**
   (corrected 2026-09-18, round 4 review, Reviewer C — this bullet's own closing sentence is an
   imperative, "state a bound + fallback... or an explicit accepted limitation," not a decision,
   unlike every other item in this Finding, which reads "**Decided:** ..."). Resolve it before
   implementation: state a bound + fallback (e.g. a temp file + a path param instead of inline
   text past some size) or an explicit accepted limitation — don't leave it undiscovered until a
   real user hits it. Note this risk lives on the one platform (Windows) with no CI test leg at
   all in this repo today (`test-suite.yml`'s matrix is `[ubuntu-latest, macos-latest]`), so make
   the bound check a unit-testable pure function so it's at least covered by
   `cargo nextest run --workspace` on the platforms CI does run.

## In scope
- **Trace the actual transitive `require()`/resource closure of `main.lua`** before vendoring —
  not just the documented top-level directories (`ast/`, `common/`, `modules/`, `customnodes/`,
  `normalize/`, `quarto-init/`, `quarto-pre/`, `crossref/`, `layout/`, `quarto-post/`,
  `quarto-finalize/`). Lua filters can pull in data files or runtime assumptions (e.g.
  `quarto.doc.output_directory()`) that a directory copy won't surface until it fails at
  runtime. Vendor into an in-repo dir (mirror `resources/scss/`, e.g. `resources/pandoc-filters/`)
  + `include_dir!`; pass `cargo xtask lint` (external-sources-in-macro). Include `init.lua` (or a
  trimmed `param()`-only variant). **Two vendoring roots needed, not one — see Finding 1 above**
  (`init.lua`'s actual tree is `src/resources/pandoc/datadir/`, separate from `src/resources/
  filters/`); use `ResourceBundle` to materialize both to disk at invocation time, per Finding 1.
- **Pin the vendored source to a quarto-cli *release*, not a dev commit** (resolved 2026-09-17,
  decided with Gordon — originally a P5 open question, resolved here since P4 owns the actual
  vendoring). Confirmed the local `quarto-cli` checkout (`dcffbfade8`) is exactly `v1.11.3` + one
  doc-only commit (`git describe`: `v1.11.3-1-gdcffbfade`; the one commit ahead touches only
  `.claude/rules/filters/overview.md`, zero diff under `src/resources/filters/`) — so **vendor
  from tag `v1.11.3`**, byte-identical to what's on disk today, no dev drift to absorb. Create
  `resources/pandoc-filters/README.md` mirroring `resources/scss/README.md`'s Source/Updating
  structure, recording the pinned tag (Bootstrap pins a release *number*; this pins a release
  *tag*, since quarto-cli's Lua filters have no per-file version). Drift detection needs no
  separate mechanism — P5's Layer-1/Layer-2 contract tests are the tripwire (same philosophy as
  P3's own 3-file patch), and a future re-vendor becomes a deliberate act: bump the pinned tag,
  re-run Layer-1/2, see what breaks.
- **Build `QUARTO_FILTER_PARAMS`** (base64-JSON, **standard alphabet + padding — see Finding 5**)
  in Rust; pass via env (**with a size bound + fallback for included text, see Finding 5**);
  point `--data-dir` at the vendored datadir so `init.lua` auto-runs and defines `param()`. Use
  the re-derivation above directly: build the required-key set (items 1-5), skip the deferred set
  (item 6), and pass a synthetic single-file "project" value per the note above. **Include one
  literal worked example** — the full JSON blob for the docx transport smoke, not just key names
  — so an implementer isn't re-deriving per-key value shapes (list vs. string, bool vs. object)
  from Q1 TS one at a time.
- **Build the Pandoc version-compatibility matrix** before enforcing anything: Q1's currently-
  pinned minimum vs. what Q2 will require, and any Lua-API differences across that range that
  could break vendored filters. Then **locate the `pandoc` binary** — already done, reuse it:
  `BinaryDependencies::discover` already sets `pandoc: runtime.find_binary("pandoc",
  "QUARTO_PANDOC")` (`crates/quarto-core/src/render.rs:154`) — this item is version-gating only,
  not binary location. Catalog error if absent/too old, per Finding 4's new `pandoc` subsystem.
- **Document the runtime-environment contract beyond params** — cwd conventions, temp-file
  layout, `--resource-path` — that Q1's Lua may assume from being launched by Deno's TS
  orchestrator, which a bare Rust-shelled `pandoc` invocation won't replicate for free.
- A `PandocWriteStage`: takes the neutral-core AST, serializes the wire format (P2) via pampa's
  Pandoc-superset JSON mode (**not** `raw: true`, which is explicitly non-Pandoc-compatible —
  `crates/pampa/src/writers/json.rs:40-81`; the superset shape's extra `astContext`/`s:` keys are
  confirmed harmless to real pandoc, verified against pandoc 3.8.1), shells to `pandoc -f json -t
  <fmt> --data-dir <materialized datadir> -L <materialized filters>/main.lua -o <output>` with
  the env blob, and **writes the output file itself** (see Finding 3 — binary bytes never enter
  `PipelineData`/`RenderedOutput`). Also owns bumping pampa's hardcoded `pandoc-api-version`
  (`[1, 23, 1]`, `json.rs:1869`) if the version-compatibility matrix above ever needs a newer
  floor — pandoc rejects an incompatible API version outright.

## Out of scope
- The shim / Route R/N (P5; corrected 2026-09-17, no Route-L type in the current inventory).
  Correct semantics / numbering (P5/P6). Per-format invocation detail
  (P7 — P4 uses a minimal defaults set for the smoke).
- book/website/manuscript project-mode `filterParams` hooks (see Open question 2) — no
  project-mode Pandoc-hybrid rendering in this epic's scope yet.

## Consumes / Produces (seams)
- **Consumes:** P2 wire format.
- **Produces for P5/P7:** a working "AST → bytes" pipe + the params-blob builder they extend.

## Coarse checklist
- [x] Re-derive the full `QUARTO_FILTER_PARAMS` key set (all three research-doc open questions
      resolved above); decided which keys the smoke/v1 minimally needs (items 1-5) vs. deferred
      (item 6, with reasons).
- [x] Trace `main.lua`'s transitive require/resource closure; vendor accordingly (not just top-level dirs) — **two roots, not one, see Finding 1**: the filters tree AND `init.lua`'s separate `src/resources/pandoc/datadir/` tree. `include_dir!` for storage, `ResourceBundle` to materialize both to disk at invocation time (`main.lua` needs real files, `include_dir!` alone doesn't extract them); lint green. **Done: Task 1.**
- [x] **New (2026-09-18): vendor the shim's loading mechanism** — a small, marked patch to
      `main.lua` inserting `tappend(quarto_filter_list, quarto_pandoc_shim_filters)` between
      `quarto_init_filters` and `quarto_normalize_filters` (per Finding 2), in the same spirit as
      P3's crossref patch. This is P4's item, not P5's — P4 owns the vendored `main.lua`. **Done: Task 8.**
- [x] **New (2026-09-18): decide and document the `pandoc`-nonzero-exit diagnostic** (stderr
      passthrough policy, temp-JSON retention on failure) and reserve the new `pandoc` error
      catalog subsystem, **number 18** (Finding 4) — needed before P5/P6/P7 each independently
      invent one. **Corrected/expanded 2026-09-18, round 4 review:** (a) capture the `pandoc`
      subprocess's stderr **unconditionally**, not only on nonzero exit, and re-emit any
      `[WARNING]`-shaped line as a diagnostic through the `pandoc` subsystem (or the existing,
      currently-unclaimed `Q-11-1` "Lua Filter Diagnostic" code) — without this, every Q1
      `quarto.warn()` on a successful render, including the epic's own documented `order`-missing
      degradation (P3) and P5's unrecognized-`type_name` warning, has no delivery channel; (b) put
      the error-docs work explicitly on this checklist, not just in the Finding prose: author
      `docs/errors/pandoc/` pages for the initial code set, add the new `- section: "pandoc"`
      block to `docs/_quarto.yml`'s errors sidebar, confirm `cargo xtask lint` green — this is the
      exact "no `- section:` block at all" scenario the sidebar lint rule was created to catch,
      and a Finding-only acknowledgment (as this item previously was) is where obligations in this
      epic have repeatedly gone to be forgotten. **Done: Task 6 (catalog subsystem 18 + docs pages
      + sidebar) and Task 10 (unconditional capture, Q-11-1 reuse, verbatim nonzero-exit wrap,
      temp-JSON retention).**
- [x] **New (2026-09-18): pin the base64 variant (standard + padded) and state a size bound +
      fallback for `QUARTO_FILTER_PARAMS`'s included text** (Finding 5) — a Windows correctness
      risk if left unbounded. **Done: Task 3 (codec + size-bound predicate); the fallback itself
      stays `accepted-untested`/undecided per Findings item 9 — a final decision, not a gap.**
- [x] Pin the vendored source to a release tag, not a dev commit — **resolved 2026-09-17,
      decided with Gordon** (see above): vendor from `v1.11.3`. Actually creating
      `resources/pandoc-filters/README.md` with this pin recorded happens alongside the vendoring
      work itself (the item above), not as a separate step.
- [x] Build the Pandoc version-compatibility matrix; min-version gate + catalog error (pandoc
      *location* is already done — `BinaryDependencies::discover`, see above — this item is
      version-gating only). **New, added 2026-09-18 (round 4 review, Reviewer D — measured, not
      estimated): the vendored Lua's implied pandoc floor is above what this repo's CI and
      dev-tooling currently provide.** Quarto `v1.11.3` (this epic's vendoring pin) bundles
      **pandoc 3.10** (`git show v1.11.3:configuration` → `export PANDOC=3.10`); this repo's CI
      installs **3.8.3** (`test-suite.yml:18`/`ts-test-suite.yml:18`, pinned "to match Quarto
      1.9"); `cargo xtask dev-setup`'s floor is **3.6** (`dev_setup.rs:295`). The vendored filters
      were authored and tested against a pandoc newer than what CI runs them against. **Make the
      pandoc version part of the pin, alongside the quarto-cli tag, in the same commit as
      vendoring:** record 3.10 in `resources/pandoc-filters/README.md`, bump CI's
      `PANDOC_VERSION` (`test-suite.yml`, `ts-test-suite.yml`) and `dev_setup.rs`'s floor to
      match, and state in P7 that the golden contract is **`(quarto tag, pandoc version)`**, not
      just the quarto tag — a routine `PANDOC_VERSION` bump for unrelated reasons can otherwise
      redden every P7 golden snapshot (writer-shape changes: `pStyle` values, `numbering.xml`,
      `<w:drawing>` wrapping) with no Q1 or Q2 change, and nothing currently ties a bump to a
      re-capture. Per this repo's own CLAUDE.md rule ("when you add a gating step to a CI
      workflow, add its `cargo xtask verify` counterpart in the same commit"), also add a pandoc
      presence+version preflight to `cargo xtask verify` itself (mirroring `dev_setup.rs`'s
      existing warn-only `check_pandoc`, promoted to a hard failure) — `verify` currently has zero
      pandoc awareness even though pandoc is already a *hard* dependency of `cargo nextest run
      --workspace` today (pampa's oracle tests `.expect()`-panic if pandoc is absent), and this
      epic is about to add its most environment-sensitive dependency yet with no `verify`
      counterpart. Note separately: Windows has **no CI test leg at all** (`test-suite.yml`'s
      matrix is `[ubuntu-latest, macos-latest]`), which is exactly the platform Finding 5(b)'s
      32,767-char env-block risk lives on — make that size-bound check a unit-testable pure
      function (e.g. `fn params_blob_exceeds_platform_limit(len) -> bool` with the Windows
      constant as data) so it is at least covered by `cargo nextest run --workspace` on the
      platforms that do run in CI, even though the platform it protects doesn't. **Done: Tasks 7-8.**
- [x] Document the runtime-environment contract (cwd/temp-file layout/`--resource-path`) Q1's Lua assumes. **Done: Task 2** (`harness.rs` module doc + `resources/pandoc-filters/README.md`).
- [x] Resolve the source of `languageFilterParams`'s `crossref-<type>-title` family —
      **resolved 2026-09-17, decided with Gordon** (see finding above): source from Q2's
      `RefTypeRegistry`, not Q1's own locale files. Closes P5's "constructor default-passthrough
      policy" open question as a standing principle, not just a Theorem-specific fix.
- [x] Build `languageFilterParams`'s params from `RefTypeRegistry`, one `crossref-<ref_type>-title`
      **and one `crossref-<ref_type>-prefix`** key per registered type (corrected 2026-09-18,
      round 4 review — see the Critical correction above: `-title` alone leaves the reference-text
      path, `refPrefix()`, reading an unfed param and falling through to a bare literal for every
      theorem-family type) — the actual implementation of the decision above. **Done: Task 5.**
- [x] `PandocWriteStage` (including the synthetic single-file "project" value for the params
      builder; writes its own output file, per Finding 3 — no bytes threaded through
      `PipelineData`); a new Pandoc-leg entry point (e.g. `render_qmd_to_pandoc`, sibling to
      `render_qmd_to_html`) and the Pandoc-leg stage list (P1 owns the transform exclude-list,
      P4 owns the stage list that plugs `PandocWriteStage` in); transport smoke: "run main.lua,
      get bytes" for one fixture, using pampa's Pandoc-superset JSON mode (not `raw: true`).
      **Done: Task 9 (`PandocWriteStage` + `render_qmd_to_pandoc` + stage list) and Task 11
      (transport smoke, reviewed and clean).**

**P4 status: complete.** All 11 implementation tasks (see
`2026-09-18-pandoc-hybrid-P4-implementation.md` and its ledger at
`.superpowers/sdd/2026-09-18-pandoc-hybrid-P4-implementation/progress.md`) landed, reviewed, and
verified green at the workspace-wide `cargo nextest run --workspace` phase boundary. Status line
below updated from "Shape draft" accordingly.
