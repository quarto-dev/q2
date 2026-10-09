---
title: 'Follow-up: `format: typst` smoke-all coverage beyond the orange-book epic'
date: 2026-09-29
---

**Date:** 2026-09-29
**Status:** Research/triage complete. Proposal below — **not started**, awaiting
Gordon's review before any fixture is copied or any Rust test is written.
**Base:** `origin/feature/typst-testing` tip (PR #745, still open at time of writing —
re-check `gh pr view 745 --json state` before branching implementation work off this;
if merged, re-base onto `main` first, per this file's own worktree note).
**Worktree:** `.worktrees/workspace-4`, branch `typst-testing/workspace-4`.
**Predecessor:** `claude-notes/plans/2026-09-27-typst-smoke-all-epic.md` (PR #745) —
ported 7 book/margin-layout fixtures + built the assertion vocabulary
(`ensureTypstFileRegexMatches`, `ensurePdfRegexMatches`, `ensurePdfTextPositions`) this
follow-up reuses as-is.

## The gap, corrected

The original handoff estimated ~27 curated + ~140 dated-regression fixtures (~165
candidates) beyond the 7 already ported. Both numbers needed correction:

- **Curated `smoke-all/typst/`:** ~27 holds up — confirmed 33 new single/directory
  candidates + `lof-lot` (orphaned, see below) + `myfonts/` (not a fixture, a font
  asset dir for `custom-fonts.qmd`).
- **Dated regressions:** ~140 was wrong. A plain grep for the substring "typst"
  over-counts (matches build-cache directories, sibling multi-format variants,
  unrelated prose). A strict top-level `format: typst` parse under-counts — it misses
  fixtures that declare `typst` only as a nested key under `_quarto.tests.<format>:`
  (the smoke-all harness renders whichever formats have a `tests:` block, independent
  of the front matter's own primary `format:`). Combining both patterns plus a
  `keep-typ: true` sweep for stragglers, the real count is **50 individual fixtures +
  1 project** (`2023/12/04/7784/`, `_quarto.yml`-driven, 3 files). Distribution: 2023×10,
  2024×15, 2025×9, 2026×15 (includes an 8-file cluster, `issue-13992-*`, all dated
  2026/02/04 — one issue, one test-per-Typst-construct: theorem/proof/table/listing/
  nested-callout/nested-tabset/figure/plain).

Real total candidate pool: **~84**, not ~165.

## What I actually rendered (not just read)

Built `q2` once (`cargo build -p quarto --bin q2`, clean, ~36s), then rendered a
representative sample directly against `external-sources/quarto-cli` fixtures — real
`q2 render <path> --to typst` invocations, artifacts inspected then removed from the
Q1 checkout afterward (all render output there is either gitignored or was untracked
cruft I created and cleaned up; `git status` on that checkout shows only one
pre-existing unrelated modification, `orange-book-margin/index.qmd`, not touched by
this session).

### Confirmed capability gaps (real Q2 bugs, not test-coverage gaps)

These are genuine defects, independently reproduced with minimal repros where useful.
Per this epic's own P5 precedent: **file separately, don't block the port on fixing
them.**

1. **`font-paths:` (and brand.yml file-based fonts) are never wired to Typst's
   `--font-path`.** `crates/quarto-core/src/stage/stages/typst_compile.rs`\'s
   `font_path_args()` only ever adds the vendored package-cache fonts dir — it never
   reads the `font-paths` metadata key at all (confirmed: zero matches for
   `"font-paths"` anywhere in `crates/**/*.rs`). Reproduced on 3 independent curated
   fixtures (`custom-fonts.qmd`, `font-paths/subdir-font-paths`,
   `font-paths/brand-font-paths`) plus `relative-font-path/report1` (whose own Q1 test
   asserts, via `printsMessage negate:true`, that *no* "unknown font family" warning
   should appear — Q2 prints it) **and** one dated regression
   (`2025/12/09/13775-brand-typst-citeproc.qmd`, brand-declared "Sarabun" not found).
   This is the single largest root cause behind the "fonts" theme — one fix likely
   clears most of that theme's failures at once.

2. **Fenced-div attribute parser rejects `.class #id` ordering.** Minimal repro
   (`::: {.foo #bar}`) fails with `Parse error: unexpected character or token`;
   `::: {#bar .foo}` (id first) parses fine. Not Typst-specific — this is a markdown-
   parser bug that would affect every format. Found via `callout-paragraph-alignment.qmd`
   (`::: {.callout-tip #tip-alignment}`).

3. **`{{< placeholder N >}}` inside a `layout=` panel with 7 images fails**
   (`layout/fraction-layout.qmd`): `error: failed to load file (is a directory)`.
   Looks like a path-collision when multiple identical placeholder shortcode calls
   resolve to the same generated path within one layout panel.

4. **Raw HTML with a base64 data-URI `<img>` fails typst compile**
   (`juice/test.qmd`): `error: file not found (searched at .../juice/<uuid>)` — the
   mediabag-extracted asset isn't where the generated `.typ` expects it.

5. **brand.yml logo `directional-padding` emits a bad Typst unit.** `typst compile`
   errors `invalid number suffix: px` — Q2 is passing a raw CSS-style `px` value into
   generated Typst source, which doesn't accept that suffix (needs `pt`/conversion).

6. **`authors.lua` (shared pandoc filter, not typst-specific) crashes on multi-author
   \+ `affiliation:`-as-string metadata.** `pandoc-template-features.qmd`:
   `attempt to index a nil value (field 'integer index')` in
   `modules/authors.lua:358`, called from `processAuthorMeta`. This is a shared-filter
   bug (author normalization runs for every format), just first surfaced here because
   no other ported fixture happens to combine multi-author + plain-string affiliation.

### Confirmed pure coverage gaps (Q2 already renders correctly, just untested)

Clean renders, no warnings: `callout.qmd`, `callout-no-icon.qmd`,
`code-listing-alignment.qmd`, `tbl-align-issue10086.qmd`, `theorem/theorem-simple.qmd`,
`columns/basic-two-column.qmd`, `css-property-processing/default.qmd`,
`toc/toc-title-auto-fallback.qmd`, `syntax-highlighting/idiomatic.qmd`,
`brand-yaml/color/foreground-background.qmd`,
`brand-yaml/font-filtering-fallback/font-filtering-fallback.qmd`,
`brand-yaml/typography/basefont-typst.qmd`. Also clean, across dated regressions:
`2023/09/26/6977.qmd`, `2024/07/03/10217.qmd`, `2026/06/29/14583-typst.qmd`,
`2026/02/04/issue-13992-theorem.qmd`. `2025/10/21/13589.qmd` prints a warning but it's
the fixture's own intentional assertion target (unknown-reference-type callout), not
a bug.

### Can't verify in this environment

`great-tables-oceania.qmd` and `pandas-cell-css-rules.qmd` both use the `jupyter`
engine, which isn't installed in this worktree/sandbox (`Engine 'jupyter' is
registered but its runtime is not available`). Their Typst-specific behavior is
unverified either way — needs a machine with a Python/Jupyter env to sample for real
before committing to porting them.

### Excluded from the candidate pool entirely

- `myfonts/` — font asset directory for `custom-fonts.qmd`, not a fixture.
- `lof-lot.pdf`/`lof-lot.typ` — committed reference output with **no corresponding
  source `.qmd` anywhere in the Q1 checkout** (confirmed by search, not assumed).
  Unportable as-is; flagging for Gordon rather than silently dropping — worth asking
  upstream Q1 if the source was ever committed, or treating as lost.

## Proposed port, grouped by theme

Every group below is *pure test-coverage* work (clean-rendering fixtures →
`ensureTypstFileRegexMatches`/`ensurePdfRegexMatches`/`ensurePdfTextPositions`
assertions), **except** where noted — those need the corresponding bug fixed first,
or need to be ported with an assertion that captures the *current* (buggy) behavior
and a follow-up bead linking to the filed bug, Gordon's call on which.

1. **Callouts & basic blocks** — `block-divs`, `callout`, `callout-no-icon`,
   `code-listing-alignment`, `definition-item-no-break`, `raw-set-page-no-extra-page`,
   `url-image-mediabag`. All have committed Q1 reference `.pdf`/`.typ` to diff
   against. `callout-paragraph-alignment` blocked on bug #2 above (parser rejects its
   own div syntax) — port once fixed, or file the bug and skip for now.

2. **TOC & tables** — `toc/toc-title-auto-fallback`, `tbl-align-issue10086`,
   `suppress-bibliography`. (`lof-lot` excluded, no source.)

3. **Typography & citeproc** — `typst-bibliography-leading-dot`, `typst-citeproc`,
   `typst-no-citeproc`, `typst-i18n`, `typst-subfig`, `typst-subfig-badid`. Small,
   focused, single-file — cheap to port, each exercises one citeproc/typography edge
   distinct from orange-book's numbering focus.

4. **Theorems & columns** — `theorem/*` (5), `columns/*` (4). Strongest
   `ensurePdfTextPositions` candidates in this batch — column layout and theorem-box
   placement are exactly the relational assertions that predicate was built for.

5. **Syntax highlighting & CSS processing** — `syntax-highlighting/*` (13),
   `css-property-processing/*` (3). `pandas-cell-css-rules.qmd` needs the jupyter-
   engine check above before deciding in/out; the rest render clean today.

6. **brand.yml** — `brand-yaml/color/*`, `brand-yaml/font-filtering*`,
   `brand-yaml/typography/*` port clean today. `brand-yaml/logo/*` is blocked on bug
   #5 (`directional-padding` px-unit crash) for at least that one file — sample the
   other logo variants (`customize-without-path`, `light-dark-variants`,
   `online-logo`) before deciding how much of `logo/` ports now vs. waits.

7. **Layout & juice** — `layout/overflowing-callout*`, `layout/unitless-image-width`
   look portable; `layout/fraction-layout` and `juice/*` are blocked on bugs #3/#4
   above respectively — file both, don't port until fixed (or port with a
   known-broken assertion + linked bug, if Gordon wants the regression pinned now).

8. **Fonts** — `custom-fonts`, `font-paths/*` (4), `relative-font-path`. All four are
   blocked on bug #1 — this is the fix-first-then-port case *the* epic's escape hatch
   was written for: one root-cause fix likely clears the whole theme at once. Worth
   fixing before porting rather than porting a theme that's currently 100% red.

9. **Dated regressions — do NOT bulk-port.** Per the handoff's own framing, 50
   one-off issue-regression fixtures is a decision for Gordon to make explicitly, not
   a default. Of the ~50, three sub-groups stand out as worth asking about
   specifically rather than defaulting to "skip all":
   - The **`issue-13992` cluster** (8 files, one issue, systematically covers
     conditional-visibility across theorem/proof/table/listing/nested-callout/
     nested-tabset/figure/plain) reads more like a thematic feature-test family than a
     one-off regression — closer in spirit to the curated directory than to the rest
     of the dated corpus.
   - The **`13775-*` trio** (brand+citeproc, `-html-variants`, `-latex-variants`
     siblings not in the typst set) is a real brand.yml+citeproc interaction, already
     hit by bug #1 above.
   - `2026/06/29/14583-typst.qmd` and `2026/02/12/mermaid-typst.qmd` render clean and
     look like they exercise typst-specific paths (mermaid diagram embedding) not
     covered elsewhere.
   Recommend asking Gordon: port these ~11-13 specifically, leave the remaining ~37
   dated regressions unported by default (available to mine later if a specific area
   needs regression coverage), rather than a wholesale decision either way.

## Bugs to file (before or independent of any porting)

1. `font-paths:` / brand file-fonts never reach Typst's `--font-path`
   (`typst_compile.rs::font_path_args`).
2. **RESOLVED — not a bug, no parser change needed.** Verified by a dedicated research
   pass (2026-09-29): real Pandoc (v3.11) is fully order-independent for div/span
   attributes (`{.foo #bar}` and `{#bar .foo}` both parse to the identical AST) — Gordon's
   suspicion was correct that Q1/pandoc accepts both orders. Q2's grammar, however, is
   **intentionally** order-sensitive by design:
   `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:559-605`
   (`_pandoc_attr_specifier` → `commonmark_specifier`) hard-requires id-first, then
   class(es), then key=value(s), with no grammar path back to an id after a class. This
   reads as a deliberate LR-grammar simplification (closest history: Carlos's
   `75f0d4d86` "make grammar tighter around attributes", no rationale text) — not
   something to widen. **The real gap is diagnostic-quality**, exactly as Gordon
   suspected: Q2 already has a purpose-built friendly diagnostic for the sibling case
   (`Q-2-3`, "Key-value Pair Before Class Specifier", `crates/pampa/resources/error-corpus/Q-2-3.json`)
   plus a whole Q1→Q2 porting-hazard detector for it
   (`crates/qmd-syntax-helper/src/conversions/attribute_ordering.rs`), but **no
   equivalent corpus entry exists for "class before id"** — it falls through to the
   generic fallback error at `crates/quarto-parse-errors/src/error_generation.rs:298`
   ("unexpected character or token here"). Recommended fix: add a new error-corpus entry
   (next free `Q-2-NN` code) modeled exactly on `Q-2-3.json`, titled something like
   "Class Specifier Before Id Specifier in Attribute", with cases covering `{.foo #bar}`
   and the callout fixture's `{.callout-tip #tip-alignment}`. This is a diagnostic
   addition, not a parser-behavior fix — no grammar change, no regression risk to the
   existing id-first requirement.
3. `{{< placeholder N >}}` produces an **empty image path** (`image("")`) in Typst
   output — confirmed broader than originally scoped. Originally described as a
   multi-image `layout=` panel path-collision; re-tested 2026-09-29 and reproduced with
   a **single, non-layout** `{{< placeholder 200 >}}` call (`typst-subfig.qmd`,
   `typst-subfig-badid.qmd`) and with all three `layout/overflowing-callout*`/
   `unitless-image-width.qmd` fixtures — every one hits the identical
   `#box(image("", width: ...))` / "failed to load file (is a directory)" failure, not
   just the 7-image layout panel case. The root cause is the placeholder shortcode
   itself never emitting a path for the typst target, independent of layout context.
   6 fixtures blocked on this (up from 1): `typst-subfig`, `typst-subfig-badid`,
   `layout/overflowing-callout`, `layout/overflowing-callout-7`,
   `layout/unitless-image-width`, `layout/fraction-layout`.
4. Base64 data-URI `<img>` in raw HTML → typst mediabag asset not found
   (`juice/test.qmd`; `juice/gt-table-images.qmd` not independently re-tested this
   session, presumed same theme, not confirmed).
5. brand.yml logo → Typst background-image generation is broken **far more broadly**
   than the original `directional-padding` px-unit-crash scoping. Re-tested 2026-09-29
   against 9 `brand-yaml/logo/*` variants (`customize-without-path`,
   `light-dark-variants`, `light-dark-variants-dark-mode`, `online-logo`, `padding`,
   `padding-xy`, `posit`, `quarto`, `relative-path`) — **all 9 failed**, every one with
   an empty/missing value somewhere in the generated
   `#set page(background: align(..., box(inset: ..., image("...", width: ...))))` call
   (empty image path, empty inset, empty alignment, empty width, in various
   combinations — e.g. `customize-without-path.qmd`\'s simple `padding: 2rem` produces
   `box(inset: , image("", width: 300px))`, and `posit/brand-logo.qmd` produces
   `align(, box(inset: , image("", width: )))` — nothing from `brand.logo` metadata is
   reaching the generated call). This is a large, close-to-total gap in brand.yml logo
   support for Typst, not a narrow unit-conversion bug — needs its own scoping pass
   before a fix is attempted.
6. **FIXED 2026-09-29.** `authors.lua` crashed on multi-author + string
   `affiliation:` (shared pandoc filter, cross-format — not typst-specific).
   Reconfirmed 2026-09-29 via two more independent repros:
   `columns/two-column-landscape.qmd` and `columns/two-column-title-block.qmd` (same
   `authors.lua:358: attempt to index a nil value (field 'integer index')` crash,
   called from `byAuthors`/`processAuthorMeta`).

   **Root cause, fully diagnosed 2026-09-29 (bug-fix session) — not a Lua bug, a
   double-normalization bug.** `authors.lua` is byte-identical between Q2's vendored
   copy and Q1's current upstream (`diff` confirmed empty) — it is not broken code, it
   is being fed data in the wrong shape. Confirmed via direct instrumentation
   (`io.stderr:write` probes temporarily added at the crash site and at
   `processAuthorMeta`\'s entry, then reverted — not committed) that:
   - `crates/quarto-core/src/transforms/authors_normalize.rs`'s
     `AuthorsNormalizeTransform` (registered unconditionally in `pipeline.rs` — "Runs
     right after metadata-normalize; format-agnostic like Q1's authors.lua pass", no
     format gate at all) runs for **every** render, Pandoc-hybrid targets (typst/docx/
     pptx/odt) included. It writes `meta['authors']`/`meta['affiliations']` *before*
     Pandoc ever runs, with affiliation ids as **plain strings** (`"aff-1"`,
     `crates/quarto-core/src/metadata/authors.rs:749`'s `format!("aff-{}", ...)`).
   - Then, for the Pandoc-hybrid leg specifically, the vendored Lua `authors.lua` runs
     *again* (via `normalize.lua`'s `Meta = function(meta) ... authors.processAuthorMeta(meta) ...`,
     unconditional for every Pandoc-hybrid render) and reads `meta['authors']` —
     picking up the **already-normalized** Rust output, not the raw `author:`/
     `affiliation:` frontmatter. `authors.lua`'s own `maybeAddAffiliation` (line
     310-312) assigns ids as `{ pandoc.Str(affiliationId) }` (an Inlines-shaped
     table, indexable as `[1].text`) — but Q2's Rust-written ids are plain strings, so
     `affiliation[kId][1]` fails, crashing at line 358/(817 in `byAuthors`).
   - **Confirmed Q1 does not have this bug**: real `quarto render
     pandoc-template-features.qmd --to typst` (the exact fixture that crashes under
     Q2) succeeds cleanly — Q1 has no Rust-side pre-normalization step, so the shared
     Lua only ever sees raw metadata, its only supported input shape.
   - **Fix is not "small, scoped" as originally estimated** — it's an architecture
     question, not a one-line patch. `AuthorsNormalizeTransform`\'s own outputs
     (`by-author`, `labels.abstract`, etc.) are read directly by the typst template
     (`resources/pandoc-filters/typst-template/typst-show.typ:8,10,30` — `$if(by-author)$`/
     `$for(by-author)$`/`$labels.abstract$`), so the transform cannot simply be
     deleted or format-gated off without confirming what replaces those template
     variables for the Pandoc-hybrid leg. The likely-correct direction: skip
     `AuthorsNormalizeTransform` (or at least its `authors`/`affiliations`/`by-author`/
     `labels` writes) when `ctx.format.identifier.is_pandoc_hybrid()` is true (a
     predicate that already exists, `crates/quarto-core/src/format.rs:245`) — for that
     route, the vendored Lua `authors.lua` is the sole, pre-existing, working source
     of those same keys (confirmed: `typst-template.typ:64` computes its own
     `has-title-block` locally from `authors`/`title`/`date`/`abstract`, **not** from
     any Rust-only `rendered.has-title-block` key, so at least that one key is safe).
     **Not yet verified**: whether docx/pptx/odt reference-doc templates consume any
     `AuthorsNormalizeTransform`-only key (e.g. `rendered.has-title-block`,
     `quarto-template-params.title-block-categories`, `author-meta`) that the Lua path
     does *not* equivalently provide — this must be checked before gating, or those
     formats could silently lose title-block rendering. No fix attempted or
     committed this session; `authors.lua` and all Rust files are back to their
     pre-investigation state (`git status` clean at the epic's HEAD).

   **Fix applied 2026-09-29 (follow-up session), revised same day after finding a
   preview regression in the first version.** First attempt gated the *entire*
   transform on `ctx.format.identifier.is_pandoc_hybrid()` (returning early before
   writing anything). That fixed the crash but broke something not caught until a
   follow-up review: `q2 preview` on a document whose frontmatter declares a
   pandoc-hybrid format (`format: typst`/`docx`/...) doesn't get the `q2-preview`
   pseudo-format substitution — `map_format_for_preview`\'s doc comment says
   "explicit non-html formats are honoured as-is" — so it falls through to the
   *native HTML pipeline* (`render_qmd_to_html`) with `ctx.format.identifier` still
   `Typst`/`Docx`/etc. `authors.lua` never runs on that leg (no real `pandoc`
   subprocess), so skipping the whole transform there left preview's title block
   with no author data at all. Confirmed empirically: reverted to the pre-gate
   code, rendered a `format: typst` doc through `render_qmd_to_html` directly, saw
   the author/affiliation correctly in the output HTML; re-applied the whole-
   transform gate, saw it vanish.

   **Root cause, precisely.** `authors.lua`'s `processAuthorMeta` explicitly
   *prefers* `meta['authors']` (plural) over raw `meta['author']` when the plural
   key is present ("prefer to render 'authors' if it is available"). Only
   `meta['authors']` and `meta['affiliations']` — the two keys this transform
   writes that *shadow* what `authors.lua` reads as its raw input — cause the
   crash. Every other key it derives (`by-author`, `by-affiliation`, `funding`,
   `labels`) is unconditionally recomputed and overwritten by `authors.lua`'s own
   `processAuthorMeta` once it runs on undisturbed raw data — so leaving Rust's
   copies in place for the real Pandoc-hybrid leg is harmless. `rendered.
   has-title-block` and `quarto-template-params.title-block-categories` are Q2-only
   keys `authors.lua` never reads at all (confirmed by grep, zero matches under
   `resources/`).

   **Final fix**: narrowed the gate to only the two raw-shadowing keys. In
   `AuthorsNormalizeTransform::transform`, snapshot `meta['authors']`/
   `meta['affiliations']` before calling `normalize_authors_meta` (only when
   `ctx.format.identifier.is_pandoc_hybrid()`), then restore the snapshot
   afterward (re-insert the original value, or remove the key if there was none).
   Every other derived key stays unconditional for every format. Added/replaced
   three unit tests in `transform_gate`:
   `typst_target_leaves_raw_authors_key_untouched_but_still_derives_by_author`,
   `typst_target_restores_a_preexisting_authors_key_rather_than_dropping_it` (the
   edge case where a document declares `authors:` directly rather than `author:`
   — the gate must restore the original, not just delete it), and
   `html_target_still_normalizes_authors_key_too`. Verified via direct
   `q2 render --to typst` that all three repro fixtures
   (`pandoc-template-features.qmd`, `columns/two-column-landscape.qmd`,
   `columns/two-column-title-block.qmd`) now render clean *and* that the compiled
   `.typ` output actually contains the correct author/affiliation text (not just
   "no crash"). Verified via a direct `render_qmd_to_html` call (the same code
   path `q2 preview` falls back to for a `format: typst` document) that the
   preview title block still renders the author/affiliation correctly.
   `columns/two-column-landscape` and `columns/two-column-title-block` ported (see
   Port session results below). `pandoc-template-features.qmd` is *not* ported —
   past the authors.lua crash, it now hits a distinct, unrelated gap: bug #17 below.

### New bugs found during the 2026-09-29 port session (not in the original 6)

7. **Pandoc definition-list syntax is not implemented at all**, and silently drops
   content. `Term\n: description` (no blank line needed) is Pandoc's definition-list
   syntax; Q2's grammar has no rule for it at all
   (`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` — zero mentions of
   definition lists) even though a `DefinitionList` AST node type exists and is handled
   throughout `crates/pampa` (writers, filters, Lua API) — nothing constructs one from
   this syntax. Instead, the leading `:` line is unconditionally parsed as an **orphaned
   table caption**: it prints `[Q-0-99] Caption found without a preceding table` and the
   description text is **dropped from the output entirely** (confirmed with a minimal
   repro: a term line + `: description text` anywhere, lipsum shortcode not required).
   Found via `definition-item-no-break.qmd`. This is a real content-loss bug, and looks
   like a missing-feature-sized grammar gap, not a quick fix.
8. **`tbl-align-issue10086.qmd` regression reintroduced**: Q2 wraps knitr/pandoc table
   output in an extra `#block[...]` between `#figure([` and `#table(...)` —
   `#figure([\n#block[\n#table(...` — which is *exactly* the double-nesting this
   upstream Q1 regression test (issue 10086) was written to guard against (its second,
   forbidden-pattern assertion checks for a *doubled* `#block[.../#figure(.../#block[.../#figure`
   sequence, which is absent, but the *required* pattern `#figure\(\[\n#table` — no
   intervening block — is also absent, because of the single extra `#block[` wrapper).
   Whatever code path adds this wrapper likely affects table column-alignment fidelity,
   which is the entire point of the regression test.
9. **TOC title auto-fallback: unused-variable bug, one-line fix identified (not yet
   applied).** `resources/pandoc-filters/typst-template/typst-template.typ:123-134`:
   ```
   if toc {
     let title = if toc_title == none { auto } else { toc_title }
     block(...)[
     #outline(
       title: toc_title,   // <- BUG: should be `title: title`
       ...
   ```
   The computed `title` fallback (`auto` when `toc_title` is `none`) is never used —
   `outline()` receives the raw `toc_title` (`none`) instead, so Typst's `outline(title:
   none)` suppresses the TOC heading entirely rather than falling back to Typst's own
   localized default (e.g. "Contents" in English). **Exact fix**: change
   `title: toc_title` to `title: title` at that line. Found via
   `toc/toc-title-auto-fallback.qmd`. This is the smallest, lowest-risk fix in this
   whole list — pure template typo, one line, no design questions.
10. **`typst-i18n.qmd` crashes the render**: `common/refs.lua:47: An error occurred:
    unknown float type 'Figura'` (a Spanish-localized float-type name not recognized by
    the crossref Lua filter), fatal in `quarto-pre/figures.lua`. Blocks the entire
    i18n/localized-crossref theme, not just cosmetic.
11. **Typst-native citation handling looks architecturally inconsistent — needs research,
    not a quick fix.** `typst-citeproc.qmd`, `typst-no-citeproc.qmd`, and
    `typst-bibliography-leading-dot.qmd` (all plain, non-margin, non-book documents with
    `citeproc: true`) all fail: the generated Typst has malformed nested citation calls
    (`@Cronbach_1951[#cite(<Cronbach_1952>, form: "prose")]` for `[@Cronbach_1951,
    @Cronbach_1952]` — nonsensical Typst), is missing the native `<ref-KEY>` labels the
    Q1 fixture's assertions expect, and (forbidden by the fixture) still emits a plain
    `#bibliography(("refs.bib"))` call. This is confusing because the **already-merged,
    already-passing** `margin-layout/citation-margin-citeproc.qmd` fixture (also
    `citeproc: true`) explicitly asserts the *opposite* — pre-rendered prose citations
    with **no** native `#cite()` calls — and that assertion passes today. Whether Q1's
    real behavior differs between margin/book and plain-document contexts, or whether
    something more specific to these three fixtures is wrong, is unresolved. Do not
    attempt a fix without a dedicated research pass first.
12. **\[Corrected 2026-10-01, fixed by PR #772: pandoc already emits Skylighting output;
    only plumbing and generated definitions were missing.\]**
    **Skylighting-based syntax highlighting for Typst is not implemented at all — the
    single largest capability gap found this session.** Q2's Typst output always uses
    Typst's own native/idiomatic code highlighter (bare ` ```python ` fenced blocks,
    colored by Typst itself at compile time), **regardless of the `syntax-highlighting:`
    metadata setting**. Quarto\'s own Skylighting-based highlighting — Q1\'s *default*
    mode, which generates `#Skylighting(...)`/`#KeywordTok`/`#StringTok`/etc. calls, a
    theme-specific `#show raw.where(block: true): set text(...)` styling block, and
    integrates with brand.yml's `monospace-*` tokens — has no implementation for Typst
    at all. Confirmed via `syntax-highlighting: idiomatic` (Q2's only working mode,
    `idiomatic.qmd` passes) contrasted with every other syntax-highlighting fixture
    (custom themes, skylighting defaults, line numbers, and all 6 `brand-monospace-*`
    variants — 12 of 13 fixtures in the theme) failing on missing `#Skylighting`/
    `#KeywordTok`/theme-specific `#show raw.where` rules. This is feature-sized work,
    not a bug fix.
13. **`css-property-processing: none` is silently ignored** — `grep -rn
    "css-property-processing"` over `crates/` returns zero matches. CSS-to-Typst
    color/property translation for raw HTML always runs, regardless of this metadata
    setting. Same shape as bug #1 (font-paths) — a documented metadata key with no
    reader anywhere in the Rust code. Found via `css-property-processing/none.qmd`.
14. **brand.yml color: several palette mechanisms unimplemented for Typst beyond plain
    foreground/background.** 7 of 15 sampled `brand-yaml/color/*` fixtures failed:
    named/custom brand colors (`primary: rgb(...)`, `burgundy: rgb(...)`, etc.),
    `color.mix(...)`-derived tones, and the "unknown brand color" diagnostic message are
    all missing from generated Typst output (`exper`, `homedepot`, `posit`,
    `posit-duobrand/brand-color-light-dark`, `typst-css-duobrand-named-color-dark`,
    `typst-css-duobrand-wrong-named-color-{dark,light}`). Simple single-color
    foreground/background and one-level named colors (the cases the predecessor plan
    actually sampled) do work.
15. **brand.yml typography: font-filtering, multi-font lists, and per-element
    typography unimplemented beyond a single mainfont/basefont.** 7 of 12 sampled
    `brand-yaml/typography/*` fixtures failed, plus all 3 `font-filtering*` fixtures:
    font-filtering fallback chains (`font: ("Libertinus Serif", ...)` style lists),
    generic-family fallback (`"sans-serif"`/`"monospace"` tokens), and richer
    per-element styling (title/subtitle/heading-2/paragraph combinations — the
    `kitchen-sink-*` fixtures) never reach the generated `font:`/`codefont:` arguments
    or per-element PDF text styling. Only the single-mainfont/basefont case (already
    sampled by the predecessor plan) works.
16. **Smart-quote heuristic gap: possessive apostrophe in a heading hard-errors.**
    ``## `P(A|B)` = Bayes' Rule`` fails to parse: `[Q-2-10] Closed Quote Without Matching
    Open Quote`. Real Pandoc's smart-typography heuristic recognizes this extremely
    common English possessive-apostrophe pattern (no matching open quote nearby) and
    treats the `'` as an apostrophe, not a quote-close; Q2 hard-errors instead. Not
    Typst-specific — a general markdown/smart-quotes parser gap. Found via
    `theorem-inline-code-title.qmd`.
17. **`brand.yml` `source: google` font fetching is not implemented for Typst.**
    `pandoc-template-features.qmd` (`brand.typography.fonts: [\{family: Fira Code,
    source: google\}]`) no longer crashes after bug #6's fix, but now fails
    `noErrorsOrWarnings` on a `typst compile diagnostic: warning: unknown font family:
    fira code` — the font is never fetched/registered, so Typst falls back silently
    (a warning, not a hard error) instead of rendering with the brand-declared
    monospace font. Not yet scoped: unclear whether Q2 has *any* Google Fonts fetch
    path for brand.yml (any format), or whether this is Typst-specific (missing
    `--font-path` wiring for a fetched cache dir, distinct from bug #1's
    metadata-only `font-paths:`/file-fonts gap). Found via
    `pandoc-template-features.qmd`, left unported pending this bug.
18. **Test-harness gap: `ensurePdfMetadata` assertion (Q1 spelling) has no Q2
    implementation.** `crates/quarto-test/src/spec.rs::parse_format_spec` only
    recognizes `ensureHtmlElements`, `ensureFileRegexMatches`,
    `ensureTypstFileRegexMatches`, `ensurePdfRegexMatches`, `ensurePdfTextPositions`,
    `ensureCssRegexMatches`, and a handful of non-`ensure*` keys (`noErrors`,
    `shouldError`, `printsMessage`, `fileExists`, `pathDoesNotExist`, `folderExists`,
    `dom-parity`); any other key (including `ensurePdfMetadata`, which checks
    extracted PDF document metadata — title/author/keywords/creator — against
    expected values) hits the `other => anyhow::bail!("Unknown assertion type")` arm
    and fails the whole fixture at spec-parse time, before rendering even starts.
    Found via `pandoc-template-features.qmd`\'s original (Q1) `ensurePdfMetadata`
    block, which had to be dropped (not adapted — there is no equivalent) when
    porting; the `ensureTypstFileRegexMatches` checks kept in its place verify the
    same title/author/keywords signal at the Typst-source level (the
    `set document(...)` call) rather than the compiled-PDF-metadata level, so this
    is a test-infra gap, not a rendering-correctness gap — no document under test
    actually produces wrong PDF metadata as far as this session's sampling showed.
    Worth a scoped follow-up (parse the assertion, extract PDF metadata via
    whatever the existing `ensurePdfRegexMatches`/`ensurePdfTextPositions` PDF-text
    extraction path already uses) if more ported fixtures want this Q1 assertion
    shape rather than working around it per-fixture.
19. **`q2 preview` on a document with an explicit non-HTML `format:` (typst, docx,
    pptx, ...) silently drops TOC, crossref numbering, and apparently figure
    content — not caused by this session's work, found only as a side effect of
    verifying bug #6's fix didn't regress preview.** `map_format_for_preview`\'s doc
    comment says such formats are "honoured as-is" for `q2 preview` — no pseudo-
    format substitution happens, so the render falls through to the native HTML
    pipeline (`render_qmd_to_html`) with `ctx.format.identifier` still e.g. `Typst`.
    But `AstTransformsStage` derives which transforms to drop
    (`PANDOC_TRANSFORM_EXCLUDED`) from `PipelineProfile::from_format(ctx.format.
    target_format)` — a pure string→enum function with no way to know it's
    actually running inside the HTML-chrome pipeline rather than heading for a real
    `pandoc` subprocess — so it computes `Pandoc("typst")` and drops every
    HTML-only transform on that list (`toc-generate`, `toc-render`,
    `crossref-render`, `navbar-*`, `sidebar-*`, etc.), the same way it correctly
    would for a real Pandoc-hybrid render. Confirmed empirically with a minimal
    `toc: true` + `format: typst` + a numbered-figure-crossref fixture rendered
    directly through `render_qmd_to_html`: no TOC in the output, the `@fig-a`
    crossref resolved to an empty string ("See ." instead of "See Figure 1."), and
    the figure/image itself was entirely absent from the output. **Only
    `authors-normalize` turned out to be load-bearing enough to notice as a crash
    /missing-content bug** (bug #6) because it's the *sole* metadata source for
    something `ApplyTemplateStage`\'s built-in title-block partial reads with no
    fallback; the other dropped transforms in `PANDOC_TRANSFORM_EXCLUDED` (TOC,
    crossref, navbar, ...) silently degrade preview fidelity instead of crashing,
    which is presumably why this has gone unnoticed. Not scoped or fixed this
    session — flagging only. The real fix likely needs a genuine "am I inside the
    HTML-chrome pipeline or heading for a real Pandoc write" signal threaded onto
    `RenderContext`/`StageContext`, since `ctx.format.identifier`/`target_format`
    alone cannot distinguish the two call sites that both end up calling
    `AstTransformsStage` with the same format.

## Port session results (2026-09-29)

Ported the curated groups (plan groups 1-8) into
`crates/quarto/tests/smoke-all/typst/{basic-blocks,toc-tables,columns,theorem,
syntax-highlighting,css-property-processing,brand-yaml}/`, verifying each fixture
individually with `SMOKE_FILTER=<dir> cargo nextest run -p quarto --test integration --
smoke_all` before keeping it. Every fixture that render-and-passes as-is (or with a
Q2-reality-vs-Q1-assumption assertion fix, documented inline) was kept; everything that
hit one of the 16 bugs above was pulled back out and left unported (source files
preserved outside the tree, not lost, in case Gordon wants them for reference while
fixing a given bug — ask if needed, they weren't committed anywhere).

**Kept (passing):**
- `basic-blocks/`: 7 of 8 — `block-divs`, `callout`, `callout-no-icon`,
  `code-listing-alignment` (subject text narrowed to `"CODESTART_func"` — the original
  `"CODESTART_func():"` spans a syntax-highlighting color-boundary that our
  `ensurePdfTextPositions` item-matching can't see across; a real but separate,
  lower-priority position-assertion-infra limitation, not filed as a numbered bug
  above), `callout-paragraph-alignment` (attributes reordered id-first per bug #2's
  verdict — `{#tip-alignment .callout-tip}` — with an explanatory comment), `raw-set-page-no-extra-page`,
  `url-image-mediabag` (assertion regex updated for Q2's leading-`/` mediabag path
  convention, confirmed consistent with the already-merged `crossref-grand-finale.typ`).
  Excluded: `definition-item-no-break` (bug #7).
- `toc-tables/`: 1 of 3 — `suppress-bibliography` (+ `refs.bib`). Excluded:
  `toc-title-auto-fallback` (bug #9), `tbl-align-issue10086` (bug #8).
- `columns/`: 4 of 4 — `basic-two-column`, `two-column-toc`, `two-column-landscape`,
  `two-column-title-block` (the latter two unblocked 2026-09-29 by the bug #6 fix).
- `theorem/`: 4 of 5 — `theorem-clouds`, `theorem-fancy`, `theorem-rainbow`,
  `theorem-simple` (+ `_brand.yml`). Excluded: `theorem-inline-code-title` (bug #16).
- `syntax-highlighting/`: 1 of 13 — `idiomatic` only. Excluded: all 6
  `brand-monospace-*`, `custom-theme` (+ `.theme`), `highlight-style-alias`, `none`,
  `skylighting-default`, `skylighting-line-numbers`, `skylighting` (all bug #12).
- `css-property-processing/`: 2 of 3 — `default`, `translate`. Excluded: `none`
  (bug #13).
- `brand-yaml/`: 10 of ~35 sampled — `color/foreground-background`,
  `color/link-primary-brand-color`, `color/nobrand/brand-color`,
  `color/posit-duobrand/brand-color-light-light`, `color/typst-css-duobrand-named-color`,
  `color/typst-css-named-brand-color`, `typography/basefont-typst`,
  `typography/dashed-font-weights`, `typography/mainfont-typst`,
  `typography/nobrand/brand-typography`. Excluded: 7 color variants (bug #14), all 3
  `font-filtering*` (bug #15), 6 typography variants (bug #15), all 9 `logo/*`
  variants (bug #5).
- **Entirely excluded (0 ported):** `typography-citeproc` group (3 files, bug #11),
  `layout/` (3 files, bug #3 — `fraction-layout` already known-blocked, the other 2
  newly confirmed same root cause), fonts group (`custom-fonts`, `font-paths/*` ×4,
  `relative-font-path`; unchanged, still bug #1), `juice/*` (bug #4, not re-tested).

**Not attempted this session** (per plan's own scoping, unchanged): dated regressions
(§9), `great-tables-oceania.qmd`/`gt-islands.qmd`/`pandas-cell-css-rules.qmd` (jupyter
engine unavailable), `lof-lot` (no source).

**Net result**: real Typst rendering in this codebase is considerably less complete
than the predecessor epic's sampling suggested — of ~60 fixtures sampled/attempted this
session (beyond the 7 orange-book/margin-layout fixtures already on `main`), roughly
27 ported clean and ~33 hit one of 10 distinct capability gaps (bugs #1, #3-#16, minus
#2 which resolved to "not a bug"). Three of those gaps are large, feature-sized
(Skylighting highlighting entirely unimplemented, brand.yml logo background-image
generation broadly broken, definition lists entirely unimplemented) rather than
one-line fixes — plan accordingly.

## Open questions for Gordon

- Which of these 16 items to prioritize fixing, and in what order? A
  suggested easy-first ordering by risk/size: #9 (one-line template typo, already
  diagnosed) → #13 (metadata key ignored, same shape as #1) → #1 (font-paths, same
  shape, larger) → #6 (authors.lua) → #3 (placeholder empty path) → #4 (juice) → #8
  (table wrapper regression) → #16 (smart-quote heuristic) → #2's diagnostic-only fix →
  #10 (i18n lua crash) → the three feature-sized items (#12 Skylighting, #5 brand logo,
  #7 definition lists) → #11 (needs research before any fix, do that research first) →
  #14/#15 (brand.yml color/typography breadth, likely follow-on work after #5/#12
  establish the pattern).
- Which of the three dated-regression sub-groups (§9) to include, if any.
- Whether to spend an environment cycle getting `jupyter` available to unblock
  sampling `great-tables-oceania`/`gt-islands`/`pandas-cell-css-rules`, or treat those
  three as out of scope for this pass.
- `lof-lot` — worth a note upstream to Q1, or just drop it?
- New this session: bug #17 (`brand.yml` Google Fonts fetching not implemented for
  Typst) — surfaced only after fixing #6 unblocked `pandoc-template-features.qmd`
  far enough to reach it. Not yet scoped (see #17's entry for open questions on
  whether this is Typst-specific or a general brand.yml gap).
- Also flagged, not investigated: `font-paths/brand-font-paths-book` (a book
  project) renders clean via direct `q2 render` but fails `error: expected content,
  found array` on `author` when run through the smoke-all harness on the identical
  fixture — a harness-vs-CLI discrepancy noted in passing while fixing bug #1,
  unrelated to bug #6.

No braid strand opened for this — per the repo's "Beads vs. plans (STRICT)" rule,
this stays exploratory until Gordon scopes it into an actual plan/phase list.

## Status update (2026-10-01)

Re-verified every numbered finding against current `main` (not from commit
messages alone — re-ran the actual smoke-all fixtures via `SMOKE_FILTER=...
cargo test -p quarto --test integration smoke_all`, plus two direct render
repros for #3 and #10). `workspace-4` itself was rebased onto `main`
(`3833d06c8`) this session; see below for what's still only on the branch.

**Fixed and merged to `main`:**
- **#1** font-paths/brand file-fonts → `--font-path`. Both halves landed:
  brand `source: file` dirs (`9a4dd0539`) and the general `font-paths:`
  metadata key (PR #750, `bd-3ij4nokp`). Verified: all 3 `font-paths/*`
  smoke-all fixtures pass. **Strand closed.**
- **#2** attribute-order — resolved as "not a bug" (Q2's id-first grammar is
  intentional); real gap was diagnostic quality, now added (PR #762,
  `bd-6hf7nz7i`). *Strand still shows open — stale, should close.*
- **#5** brand.yml logo generation (was 9/9 failing) — fixed by `b70b6b36b`
  (brand-mode/dark-mode wiring) + `aa75f3199` (logo filter param wiring).
  Verified: 7/7 `brand-yaml/logo/*` fixtures pass. No strand was filed for
  this one originally.
- **#10** Spanish `Figura` crossref crash — fixed via PR #757
  (`bd-m2j4m3wg`, "Use canonical float types for Q1 category lookup").
  Verified: `cargo test -p quarto-core --test integration pandoc_shim`
  61/61 pass, plus a direct `lang: es` + `@fig-a` crossref render repro
  renders clean. **Strand closed.**
- **#13** `css-property-processing: none` ignored — fixed, PR behind
  `bd-5adgk9hr` (`7bd31564a`). Verified: 3/3 fixtures pass. *Strand still
  shows open — stale, should close.*
- **#14** brand.yml color gaps (was 7/15 failing) — turned out to be a
  fixture-authoring issue (missing explicit `brand:` key under Q2's
  no-implicit-`_brand.yml`-discovery rule), not a real capability gap
  (`9f0f6c66a`). Verified: 3/3 fixtures pass. *`bd-x1iurczn` ("verify
  color.mix/named-color gaps") still open — now verified, should close.*
- **#17** `source: google` font fetching for Typst — implemented, PR #752
  (`bd-mzrgikmu`, closed already).
- **#18** `ensurePdfMetadata` test-harness gap — merged (`5fdec2093`/
  `58ed02fe6`). *`bd-0syqvgl2` still open — stale, should close.*

**Partially fixed:**
- **#4** juice/data-URI mismediabag — the fallback no longer leaks a bare
  UUID (merged `761f0f114`, *`bd-qj6odpi4` still open — stale, should
  close*), but juice itself has never worked in q2 at all (`juice.ts`
  doesn't exist). Real fix is **PR #766** (draft, open, mergeable),
  `bd-sccaj7u4`.
- **#7** definition lists silently dropped — content-loss is fixed, now
  falls back to literal text with a new diagnostic (`Q-2-54`, confirmed
  present on main), via `c90ee9202` (`bd-definition-lists-no-fallback-irykzp3g`
  already closed). Full `DefinitionList` grammar support (the
  `Term\n: description` syntax itself) is still not implemented — feature
  gap remains.
- **#15** brand.yml typography gaps — `font-filtering`/
  `font-filtering-fallback` now pass (verified 2/2). The broader
  multi-font/per-element/kitchen-sink fixtures were never ported — no
  `typography/` fixture dir exists on main yet. `bd-post2btu` (P2, open)
  accurately reflects the remainder.

**Fixed in code, sitting on `workspace-4`, not yet merged:**
No separate PR needed for these — they land when `workspace-4` itself
merges, not before.
- **#6 (P1, `bd-ymkkrn64`)** authors.lua crash on bare-string affiliation.
  Fixed across two commits (`37bddab0e`, narrowed by `79f2df5d8` after
  catching a preview regression). Compiles clean post-rebase.
- **#9** TOC title auto-fallback one-line template fix (`6d96fe81b`). No
  strand was ever filed for this one — trivial, lowest-risk item in the
  whole list.

**In a draft PR, awaiting a merge decision:**
- **#8** extra `#block[` wrapper around knitr/pandoc tables — **PR #767**
  (draft, open, mergeable), `bd-gb6u8qsz`.
- **#4**\'s real juice fix — **PR #766** (draft, open, mergeable),
  `bd-sccaj7u4` (see above).

**Still fully open, untouched:**
- **#11** Typst-native citation handling — **resolved 2026-10-01**: PR #770
  (`bd-ysqekrm2`, `bd-wjn7jdzw`) made `citeproc: true` work on hybrid formats
  (native `<ref-KEY>`/`<refs>`, no duplicate `#bibliography`). The "malformed
  nested cite" was pandoc's own parse of the malformed `[@a, @b]` (intended
  `;`), so the fixture was corrected. The three fixtures are ported under
  `typst/citations/`. Residual, filed separately: `@key[locator]` bare
  locators parse as citation + empty span (`bd-bare-locator-citation-nx3gzh07`,
  not ported: removed from the fixtures), and citeproc output differs from
  pandoc on cluster collapse / punctuation-in-quote
  (`bd-citeproc-cluster-collapse-kns8uggw`). `./refs.bib` is normalized to
  `refs.bib` by Q2, so that fixture accepts both forms.
- **#12** Skylighting syntax highlighting for Typst — corrected: pandoc already
  emits Skylighting output; only plumbing + generated definitions were missing.
  Tracked as `bd-typst-skylighting-av61oid8` (dispatched to workspace-7).
- **#16** smart-quote gap (`Bayes' Rule` hard-errors) — verified directly,
  still hard-errors on main. Only the diagnostic message improved (now
  suggests a `\'` escape, via now-closed `bd-7vz18qht`); the underlying
  heuristic gap (auto-recognizing the possessive without a manual escape)
  is unfixed. The previously-flagged "Q-2-10 duplicate" (byte-identical fix
  across two worktree branches) is still unreconciled.
- **#19** `q2 preview` on an explicit non-HTML `format:` silently drops
  TOC/crossref numbering/figure content — not scoped, not fixed, no strand
  filed.

**Stale-strand housekeeping (confirmed resolved, braid still shows open):**
`bd-6hf7nz7i` (#2), `bd-qj6odpi4` (#4 fallback half), `bd-5adgk9hr` (#13),
`bd-x1iurczn` (#14), `bd-0syqvgl2` (#18).

## Status update (2026-10-02)

Supersedes the 2026-10-01 update where they differ. Every numbered finding is
now either merged, fixed on this branch, or tracked by an open strand.

**Merged since the last update:**
- **#4** PR #766 (`bd-sccaj7u4`) replaced the missing `juice.ts` with an
  in-process CSS inliner; PR #763 (`bd-qj6odpi4`) had already stopped the
  fallback leaking a UUID.
- **#12** PR #772 (`bd-typst-skylighting-av61oid8`) wires Skylighting for Typst.
- **#11** PR #770, as recorded above.

**Corrections to the 2026-10-01 update:**
- **#16** is won't-fix by design, not an open gap. `bd-7vz18qht` was closed
  (Carlos, 2026-08-25): the parser cannot tell a plural possessive from a stray
  closing quote, so the answer stays "escape it as `\'`". PR #748 rewrote the
  Q-2-10 note to neutral wording and is merged, so the "unreconciled duplicate"
  is moot.
- **#15** is partly done: PR #751 ported `font-filtering` and
  `font-filtering-fallback`, and this branch ports four more typography fixtures
  (`basefont-typst`, `dashed-font-weights`, `mainfont-typst`,
  `nobrand/brand-typography`). Nine Q1 fixture groups remain unported
  (`kitchen-sink-*`, `font-filtering-generics`, `google`, `complex`,
  `brand-extension`, `relative-path`, `simple`, `font-list`,
  `title-inherit-base-family`), probably unblocked by PR #752. Tracked by
  `bd-post2btu`.

**PR numbers for fixes recorded above as commits only:** #5 and #14 are #751,
#7 is #753, #13 is #764, #18 is #749.

**Strands closed this session** (all had merged PRs): `bd-6hf7nz7i`,
`bd-sccaj7u4`, `bd-qj6odpi4`, `bd-ysqekrm2`, `bd-wjn7jdzw`, `bd-5adgk9hr`,
`bd-x1iurczn`, `bd-0syqvgl2`, `bd-typst-skylighting-av61oid8`, and the parent
`bd-dsco4` (explicit `brand:` works; implicit `_brand.yml` discovery is
intentionally absent and warns).

**Still open:** #19 (no strand), `bd-gb6u8qsz` (#8, PR #767 draft),
`bd-post2btu` (#15 remainder), `bd-bare-locator-citation-nx3gzh07`,
`bd-citeproc-cluster-collapse-kns8uggw`, and `bd-ymkkrn64` (#6, closes when this
branch merges).
