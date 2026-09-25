# Pandoc-Hybrid Long-Tail Formats

**Status:** Plan (Phase 0 complete; all Gordon decisions resolved 2026-09-24 — ready for Phase 1)
**Branch:** `pandoc-hybrid-long-tail` (workspace-7, off `origin/main` @ `790eaf89f`)
**Date:** 2026-09-24

## Overview

Bulk-implement Pandoc's long-tail output formats in q2 on top of the
pandoc-hybrid architecture (`claude-notes/plans/2026-08-20-pandoc-hybrid-epic.md`,
`claude-notes/designs/pandoc-hybrid-architecture.md`). The hybrid path is
already proven end-to-end by docx/pptx/epub/typst: q2 serializes its AST to
wire JSON, shells out to a real `pandoc` binary with the vendored Q1 Lua
filters, and pandoc writes the output file directly.

The premise (verified against Q1 source): **the long tail has no custom
code in Q1.** `defaultWriterFormat()`
(`external-sources/quarto-cli/src/format/formats.ts`) enumerates every
format, but the tail branches are one-liners over generic helpers in
`formats-shared.ts` (`plaintextFormat`, `createWordprocessorFormat`,
`createEbookFormat`). The vendored Lua core is format-agnostic
(`crossrefFilterActive` gates only on `metadata.crossref !== false`), so it
is reusable unchanged. Each new format in q2 is therefore: one
`FormatIdentifier` variant + a writer-name mapping + an output extension +
a defaults row. **48 new variants across 4 tiers**, plus completion work
for `Gfm`/`CommonMark` (in the enum today but unreachable and broken —
see Tier B).

**Excluded from this plan** (each with a reason, below): bibtex/biblatex/
csljson, opml, pdf/latex/beamer/context (latex/beamer epic), jats/ipynb/
asciidoc/dashboard/email (real Q1 logic — per-format follow-ons),
html/revealjs (native), docx/pptx/epub/typst (already shipped).

## Phase 0 (DONE): Tier D live smoke test

The four JS slide formats were the only untested bucket — every other tier
sits in the same JS-incompatible class that docx/pptx/epub/typst already
exercise. Tested 2026-09-24 by hand-replicating `PandocWriteStage`'s
invocation (`pandoc -f json -t <fmt> --data-dir <share>/pandoc/datadir
-L <share>/filters/main.lua --standalone --wrap none`) against a fixture
exercising slides, code, math, a callout, a table, and a crossref figure:

- **s5 / dzslides / slidy / slideous: all PASS** — exit 0, standalone
  decks (7.7–23 KB), correct slide structure (`class="slide section
  level1"` × 3 + title slide), JS assets referenced (`slides.js`,
  `slidy.js`, `slideous.js`, `reveal.js` for dzslides).
- The vendored `_format.lua` already classifies all four as
  `isHtmlSlideOutput()` → full HTML float/callout treatment, exactly as
  Q1. **No Lua patching needed for Tier D.**
- The test params blob included `"crossref-numbering": "external"` —
  empirical confirmation that Tier D tolerates external numbering (the
  `main.lua:737-752` fail-fast guard only rejects external for
  LaTeX/Typst targets). See wrinkle 6.
- Two warnings observed, both test artifacts, not gaps: `field 'order'
  is missing from float` (the hand-fed AST bypassed q2's
  CrossrefIndexTransform, which pre-assigns `.order` in the real
  pipeline), and unresolved `@fig-test` citation spans (same cause).
- Three setup gotchas for anyone replicating: the AST meta needs the
  `quarto_pandoc_reader_opts` sentinel (q2's JSON writer injects it);
  the params blob needs the structural keys `quarto-filters`,
  `results-file`, `language`, `quarto-environment`,
  `format-identifier`, `active-filters`, `execution-engine` (all built
  by `FilterParamsBuilder` in the real path); the dependency file path
  must exist.
- **chunkedhtml side-finding:** the writer emits a **zip archive** and
  *embeds* images — a missing image is a hard exit 99, not a warning.
  And the vendored Lua does **not** classify chunkedhtml as HTML, so
  FloatRefTargets degrade to placeholders (Q1-parity: Q1 never mapped
  it either).

## Format inventory (definitive)

Q1's extension choices (`createFormat(displayName, ext)`) are the default
for output extensions; pandoc's `Format.hs` table settles the rest.
Families refer to Q1's generic helpers. Every pandoc 3.11 writer
(`pandoc --list-output-formats`) is accounted for exactly once across
the four tiers, the shipped/native set, and the exclusions.

### Tier A — bulk tail (25 variants)

Defaults: **wordprocessor** = page-width 6.5, fig 5×4,
default-image-extension png (+ `--standalone` for rtf only).
**Ebook** = fig 5×4, png. **Plaintext** = `--standalone`, png, base fig
7×5 (no explicit execute row).

| Variant | pandoc `-t` | ext | Family |
|---|---|---|---|
| `Odt` | `odt` | `odt` | wordprocessor |
| `Opendocument` | `opendocument` | `xml` | wordprocessor |
| `Rtf` | `rtf` | `rtf` | wordprocessor (+standalone) |
| `Fb2` | `fb2` | `fb2` | ebook — **note:** Q1's `createEbookFormat` is not a pure one-liner: it adds `formatExtras` include-in-header (`styles-callout.html` + epub's `styles.html`) and `mergeIncludes: false`. fb2 rides whatever q2's shipped `Epub` does with those extras today; state the outcome in tests |
| `Plain` | `plain` | `txt` | plaintext |
| `Rst` | `rst` | `rst` | plaintext |
| `Org` | `org` | `org` | plaintext |
| `Muse` | `muse` | `muse` | plaintext |
| `Ms` | `ms` | `ms` | plaintext |
| `Man` | `man` | `man` | plaintext |
| `Texinfo` | `texinfo` | `texinfo` | plaintext |
| `Tei` | `tei` | `tei` | plaintext |
| `Zimwiki` | `zimwiki` | `zim` | plaintext |
| `Dokuwiki` | `dokuwiki` | `dokuwiki` | plaintext |
| `Haddock` | `haddock` | `haddock` | plaintext |
| `Json` | `json` | `json` | plaintext (AST dump; debug aid) |
| `Native` | `native` | `native` | plaintext (AST dump; debug aid) |
| `Icml` | `icml` | `icml` | plaintext |
| `Jira` | `jira` | `jira` | plaintext |
| `Mediawiki` | `mediawiki` | `mediawiki` | plaintext |
| `Xwiki` | `xwiki` | `xwiki` | plaintext |
| `Textile` | `textile` | `textile` | plaintext — **fresh verification required** (Q1's branch is dead via the `"texttile"` typo, `formats.ts:232`; no upstream baseline exists) |
| `Docbook` | `docbook` | `xml` | plaintext |
| `Docbook4` | `docbook4` | `xml` | plaintext |
| `Docbook5` | `docbook5` | `xml` | plaintext |

### Tier B — markdown family (9 variants: 7 new + gfm/commonmark completion)

Defaults: Q1's `markdownFormat` = plaintext base + **`output-divs: false`**
(markdown must not emit cell Divs) — **except** Q1's bare `markdown`,
which dispatches to `pandocMarkdownFormat()` (`formats.ts:122`,
`format-markdown.ts:92-99`) with **no** output-divs override (Q1-parity
for `Markdown` is output-divs **true**). ext `md` throughout. Invocation
args: none, matching what q2's enum variants pass today (deliberate
deviation from Q1's `standalone: true` for markdown targets; verified
against shipped behavior in tests).

**`Gfm` and `CommonMark` are already in the enum but are NOT shipped** —
the CLI gate (`render.rs:938-950`) admits only Docx|Pptx|Epub|Typst, and
`pandoc_writer_name_for(Gfm)` falls through to `output_extension_for` =
`"md"`, which pandoc rejects (`Unknown output format 'md'`). Phase 1's
gate change would make `--to gfm` reachable *and broken* without their
writer-name arms; the full gfm/commonmark completion lives here in
Tier B.

| Variant | pandoc `-t` | output-divs | Status |
|---|---|---|---|
| `Markdown` | `markdown` | true (Q1 parity) | new |
| `MarkdownStrict` | `markdown_strict` | false | new |
| `MarkdownPhpExtra` | `markdown_phpextra` | false | new |
| `MarkdownGithub` | `markdown_github` | false | new |
| `MarkdownMmd` | `markdown_mmd` | false | new |
| `Markua` | `markua` | false | new |
| `CommonmarkX` | `commonmark_x` | false | new |
| `Gfm` | `gfm` (D7, resolved) | false | **completion** (in enum, unreachable) |
| `CommonMark` | `commonmark` | false | **completion** (in enum, unreachable) |

Skipped: Q1's `md` alias (maps to **markdown_strict**+extensions with
`lookupTo = "commonmark"`, `formats.ts:140-144` — not plain commonmark;
users write `commonmark` or `markdown_strict`).

Known Q1 deviations to note in docs: Q1 *forwards* `number-sections`/
`number-offset` for markdown targets (`pandoc.ts:1052-1057`); q2's
allow-list excludes them uniformly (`format_defaults.rs:79-88`, with a
docx-scoped justification). Tier B keeps q2's uniform exclusion —
deliberate deviation, recorded here.

### Tier C — stretch (12 variants)

Defaults: **bare invocation** — Q1 gives these formats *no* pandoc
defaults at all (`unknownFormat("txt")`, `formats.ts:341-343`), and
`--standalone` is **not** a no-op here: pandoc 3.11 ships default
templates for ansi, djot, t2t, bbcode, and vimdoc, so standalone would
wrap output in template chrome Q1 never produced. Q1-parity = no extra
args. Extensions are a deliberate improvement over Q1's blanket `txt`
where pandoc's own `Format.hs` table has a convention (docs must note
the filename change for Q1 migrants).

| Variant | pandoc `-t` | ext | Notes |
|---|---|---|---|
| `Djot` | `djot` | `dj` | pandoc table |
| `T2t` | `t2t` | `t2t` | pandoc table |
| `Xml` | `xml` | `xml` | pandoc table; XML serialization of native AST |
| `Ansi` | `ansi` | `txt` | **kept per Gordon 2026-09-24**; terminal escape output, no file convention |
| `Vimdoc` | `vimdoc` | `txt` | vim help files are `doc/*.txt` by hard convention |
| `Bbcode` | `bbcode` | `txt` | forum markup, no convention |
| `BbcodeSteam` | `bbcode_steam` | `txt` | flavor |
| `BbcodePhpbb` | `bbcode_phpbb` | `txt` | flavor |
| `BbcodeFluxbb` | `bbcode_fluxbb` | `txt` | flavor |
| `BbcodeHubzilla` | `bbcode_hubzilla` | `txt` | flavor |
| `BbcodeXenforo` | `bbcode_xenforo` | `txt` | flavor |
| `Chunkedhtml` | `chunkedhtml` | `zip` | **zip archive**; embeds images (missing image = exit 99); vendored Lua treats as non-HTML → FloatRefTarget placeholders (Q1-parity) |

### Tier D — JS slide formats (4 variants)

Defaults: Q1's `createHtmlPresentationFormat` = fig 9.5×6.5, **echo:
false, warning: false**, fig-format retina, fig-responsive false,
tbl-colwidths auto, `--standalone --wrap none
--default-image-extension png`, **`crossref-numbering: external`**
(wrinkle 6). ext `html` throughout.

| Variant | pandoc `-t` |
|---|---|
| `S5` | `s5` |
| `Dzslides` | `dzslides` |
| `Slidy` | `slidy` |
| `Slideous` | `slideous` |

Phase 0 proved the writers work through the vendored filters. Phase 5 is
wiring + verification through the real binary, not research.

## Exclusions (with reasons)

- **bibtex, biblatex, csljson** — bibliography-*export* writers; pandoc
  discards the document body (`BibTeX.hs:43`, `CslJson.hs` docstring).
  Rendering a qmd to them silently drops all prose. Different feature.
- **opml** — outline-only writer (`OPML.hs:84` `blockToOPML _ _ = return
  empty`); every paragraph/code/list/image vanishes.
- **pdf, latex, beamer, context** — latex/beamer epic (per Gordon).
- **jats (+3 variants), ipynb, asciidoc/asciidoctor/asciidoc_legacy,
  dashboard, email** — real Q1-side logic (formatExtras, handlers,
  postprocessors); per-format follow-on plans.
- **html/html4/html5, revealjs** — native q2.
- **docx, pptx, epub (epub2/epub3), typst** — already shipped on the
  hybrid path.
- **epub2/epub3 as separate variants** — q2's existing `Epub` covers the
  family.
- **md alias** — Q1 compatibility shim (markdown_strict+extensions);
  `commonmark`/`markdown_strict` suffice.

## Architectural wrinkles (Phase 1 prerequisites)

Found while verifying the seams (and by blank-slate review); each would
silently mis-fire if the tail were added naively:

1. **Defaults tables are keyed by `output_extension` — which collides for
   the tail.** `format_pandoc_defaults()` (`pandoc_filters/format_defaults.rs`),
   `format_execute_defaults()` (`stage/stages/engine_execution.rs:868`),
   and `insert_active_filters()` (`pandoc_filters/params.rs`) all match on
   `format.output_extension`. Extension collisions in this plan:
   `xml` ← opendocument (wordprocessor, page-width 6.5) **vs** docbook×3 +
   xml (plaintext); `txt` ← plain/ansi/vimdoc/bbcode×6 (uniform — safe);
   `md` ← markdown×9 (uniform — safe). Fix: re-key all three tables by
   `FormatIdentifier` (or pandoc writer name).
   **Sub-wrinkle:** `format_pandoc_defaults` is called from *two* places
   with *two different keys* today — `insert_active_filters` passes the
   extension (`params.rs:172`) while `build_forwarded_args` passes the
   writer name (`format_defaults.rs:204`, from
   `pandoc_write.rs:566,582`). They coincide for the shipped four
   (except typst); the tail makes them diverge. The re-key must unify
   both call sites, and `build_forwarded_args`' own format gates
   (slide-level pptx-only, template-typst skip) need per-variant review.
2. **`format-identifier.base-format` must be the format's canonical
   name, not the extension.** `insert_format_identifier()` currently
   sends `output_extension` — right for docx/pptx/epub by coincidence,
   and **already wrong for typst** (sends `"pdf"` today — the fix
   repairs a latent bug). The vendored `_format.lua` reads `base-format`
   for email/dashboard/gfm checks. Fix: add
   `FormatIdentifier::canonical_name()` (the user's format key —
   `"gfm"`, `"docbook"`, `"s5"`…) and send that. Note this is *not*
   `pandoc_writer_name()` in general: with D7 resolved to bare `-t gfm`,
   gfm's writer name and base-format coincide, but the two concepts stay
   distinct for the tail (docbook's writer name is `"docbook"`, its
   extension is `xml`) — and `base-format` must be `"gfm"`, never a
   suffixed writer string, or the FloatRefTarget gfm renderer
   (`floatreftarget.lua:1191`, gated on `_format.lua:231`) stops firing.
3. **The CLI gate is a hardcoded allow-list.** `render.rs` bails unless
   `is_native() || matches!(Docx|Pptx|Epub|Typst)`. Replace with
   `FormatIdentifier::is_pandoc_hybrid()` so the gate widens itself as
   variants land — but only include a variant once its writer-name arm
   exists (see Tier B for why gfm/commonmark must not pass the gate
   before Phase 1 fixes their arms).
4. **`pandoc_writer_name_for()` already exists** and defaults to
   `output_extension_for` — every tail variant needs an explicit arm
   (docbook→`"docbook"` etc.), which is the natural place the whole
   writer-name table lives. **Gfm/CommonMark need arms in Phase 1**
   (`"gfm"`/`"commonmark"` interim), because the Phase 1 gate change
   would otherwise admit them with `-t md`.
5. **`KNOWN_BASE_FORMATS` is a second base-format registry**
   (`crates/quarto-core/src/extension/discover.rs:245-254`) feeding
   `parse_format_descriptor` — it lacks even **pptx** today. Without
   updating it, extension-style formats (`acm-odt`, `acm-rst`) silently
   won't resolve as extension formats. Add the new bases tier by tier
   (and pptx while there).
6. **`crossref-numbering: external` is Docx|Pptx-only today**
   (`insert_crossref_numbering_mode`, `params.rs:274-281`). Formats with
   a real FloatRefTarget renderer re-open the bd-fzqykm0n
   double-numbering hazard without it (Q1's auto-indexer renumbers
   floats q2 already numbered and whose refs q2 already resolved to
   literal text). Decision: set `external` for **Odt** (renderer at
   `floatreftarget.lua:670`; the `main.lua:737-752` fail-fast names odt
   as supported) and all **Tier D** variants (html renderer covers
   slides; Phase 0 ran external empirically), and for **Gfm** when Tier
   B completes it. Formats that hit the placeholder renderer (rst, org,
   …) leave the key unset — the auto-indexer runs harmlessly against
   scaffolding with no visible numbers to disagree. Test: the params
   blob for odt/s5/gfm asserts `external`.

## Known Q1-parity degradations (state in docs, do not fix here)

- **FloatRefTarget placeholders**: the vendored `floatreftarget.lua` has
  renderers for latex/html(+slides+epub)/docx/odt/asciidoc/jats/ipynb/
  typst/gfm/pptx only. Every other tail format emits a Q-11-1-classified
  warning per crossref float and scaffolds the content (caption/crossref
  chrome lost). Identical behavior in Q1 — not a regression.
- **Cell output arrives as Div-wrapped blocks** (`kOutputDivs: true`,
  verified safe: every relevant pandoc writer has a real Div handler).
  Callout/theorem chrome is lost, same as docx (pptx sets
  `output-divs: false`, so it doesn't get Div-wrapped cells).
- **Mermaid/OJS**: JS-incompatible formats get no diagram fallback
  (bd-h1ub8f8z — q2 has no mermaid→PNG pipeline; inherited unchanged
  from docx/pptx/epub/typst).
- **No template vendoring**: the tail rides pandoc's bundled default
  templates (unlike typst's 8 vendored partials).

## Work items

Testing conventions: read `claude-notes/instructions/testing.md` before
writing tests; integration tests go in
`crates/quarto-core/tests/integration/<name>.rs` + register in `main.rs`
(alphabetized). CLI-gate / `--to` behavior can't be tested from
quarto-core — use the existing e2e home
`crates/quarto/tests/integration/render_pandoc_formats_e2e.rs` (drives
`CARGO_BIN_EXE_q2` against real pandoc). Gate each task on
`cargo clippy -p quarto-core --all-targets -- -D warnings` +
`cargo nextest run -p quarto-core`; run `cargo nextest run --workspace`
once per phase boundary.

### Phase 1 — plumbing prerequisites (wrinkles 1–6)

- [ ] **Test spec first:** unit tests pinning the re-keyed tables
      keyed by the *existing* variants (Docx/Pptx rows + the
      no-op-default loop over Html/Pdf/Epub/Typst/Revealjs/Gfm/CommonMark
      — the mechanism Phase 2's rows plug into); gate predicate test
      (`is_pandoc_hybrid()` true for variants with writer-name arms, false
      for Html/Pdf/Revealjs); `format-identifier` param test
      asserting canonical-name semantics (typst sends `"typst"`, not
      `"pdf"` — regression test for the latent bug).
      **Deferred to Phase 2's test spec** (the variants don't exist
      until then, so a Phase 1 red state is impossible to construct):
      the docbook-vs-opendocument discrimination test (different
      defaults despite sharing ext `xml`) and the params-blob test
      asserting `crossref-numbering: external` for Odt. Phase 2 must
      write both tests *first* (red on the missing variants/default
      rows), then add the variants.
- [x] Add `FormatIdentifier::is_pandoc_hybrid()`; replace the `render.rs`
      gate's `matches!` clause; update the not-yet-supported error text.
- [x] Re-key `format_pandoc_defaults`, `format_execute_defaults`,
      `insert_active_filters` to `FormatIdentifier`; unify the two
      `format_pandoc_defaults` call sites (extension at `params.rs:172`,
      writer name at `format_defaults.rs:204`); review
      `build_forwarded_args`' format gates per variant.
- [x] Add `FormatIdentifier::canonical_name()`; send it as
      `format-identifier.base-format`.
- [x] Add `pandoc_writer_name_for` arms for `Gfm` → `"gfm"` and
      `CommonMark` → `"commonmark"` (final per D7, 2026-09-24 — bare
      `gfm`, no suffix mechanism needed).
- [ ] Add `crossref-numbering: external` for `Odt` in
      `insert_crossref_numbering_mode`. **Moved to Phase 2** with its
      test (see the deferral note above): `insert_crossref_numbering_mode`
      currently matches `Docx | Pptx`, and the `Odt` variant it needs
      doesn't exist until Phase 2 — TDD there: the red test names Odt,
      then the variant + arm land together.
- [x] Add pptx (missing today) to `KNOWN_BASE_FORMATS`.
- [x] **Gate-reachability e2e (added during execution):** since Phase 1's
      gate change admits Gfm/CommonMark (wrinkle 4's warning), Phase 1
      carries the minimal proof they reach the *right* writers:
      `e2e_render_gfm`/`e2e_render_commonmark` in
      `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs`
      (`--to gfm`/`--to commonmark` exit 0, output is markdown, no
      `<h1`). Phase 3 remains responsible for their *content*
      completion (external numbering, output-divs).
- [x] Workspace nextest; commit. (14,740 run / 14,740 passed / 200
      skipped / 0 failed, measured 2026-09-24 on this branch; delta vs
      parent `c24355329` = the 8 new tests enumerated above plus the
      in-place gfm→pdf conversion of
      `unsupported_format_aborts_before_any_render`.)

### Phase 2 — Tier A bulk tail (25 variants)

- [x] **Test spec first:** one golden/smoke fixture per *family*
      (wordprocessor/ebook/plaintext) asserting defaults reach the right
      sinks (execute scope, params blob, CLI args); a parametrized
      per-variant render smoke test through `render_document_to_file`
      (output file exists, non-empty, extension correct; **zip magic for
      odt only — fb2 is plain XML** (`<?xml …><FictionBook`), assert
      that instead). Textile gets a *fresh-baseline* snapshot (no Q1
      parity claim). e2e test in `render_pandoc_formats_e2e.rs` driving
      `--to odt` through the real binary.
- [x] Enum variants + `TryFrom<&str>`/display names +
      `output_extension_for` + `pandoc_writer_name_for` arms.
- [x] Defaults rows: `format_pandoc_defaults` (page-width/png — the
      single sink for `--default-image-extension`; do **not** also put
      it in `pandoc_invocation_args_for`), `format_execute_defaults`
      (fig 5×4 wordprocessor/ebook), `pandoc_invocation_args_for`
      (`--standalone` for the plaintext family and rtf only).
- [x] `KNOWN_BASE_FORMATS`: add the Tier A bases.
- [x] E2E per CLAUDE.md: `cargo run --bin q2 -- render <fixture> --to
      <fmt>` for at least one variant per family; inspect output bytes;
      record invocation + observed snippet in this file.

      Recorded 2026-09-24, fixture `target/e2e-longtail/f.qmd`
      (title `F`, `# Head`, `HelloLongTail _emph_ body.`, numbered
      list), binary `./target/debug/q2` at branch tip:

      - **odt** (wordprocessor): `./target/debug/q2 render
        target/e2e-longtail/f.qmd --to odt` → `f.odt`. `unzip -p f.odt
        mimetype` → `application/vnd.oasis.opendocument.text`;
        `unzip -p f.odt content.xml | grep -o HelloLongTail` →
        `HelloLongTail`.
      - **fb2** (ebook): `… --to fb2` → `f.fb2`. `head -c 300` →
        `<?xml version="1.0" encoding="UTF-8"?>` then
        `<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" …><description><title-info><genre>unrecognised</genre><book-title>F</book-title>…`
      - **plain** (plaintext): `… --to plain` → `f.txt`, body:
        `Head` / `HelloLongTail emph body.` / `1.  one` / `2.  two`.

      All three outputs inspected by hand (not inferred from exit
      codes). The initial fixture's `{python}` cell failed with the
      expected jupyter-unavailable error — re-run without it.
- [x] Workspace nextest; commit. (14,762 run / 14,762 passed / 200
      skipped / 0 failed, measured 2026-09-24 on this branch; delta vs
      parent `ccba0fb4c` = +22 tests: 20 `#[test]` in tracked diffs
      (`format_defaults.rs`, `params.rs`, `pandoc_execute_defaults.rs`,
      `discover.rs`, `engine_execution.rs`, e2e file) + 2 in the new
      `pandoc_long_tail_formats.rs`.)

### Phase 3 — Tier B markdown family (7 new variants + gfm/commonmark completion)

- [x] **Test spec first:** `output-divs: false` asserted in the params
      blob for every Tier B variant **except `Markdown`** (Q1 parity:
      true) — `test_tier_b_output_divs_per_format` (params.rs) +
      `test_format_defaults_table` rows (format_defaults.rs);
      shortcode round-trip test — P7 check done: q2 shipped **no**
      shortcode-unescape postprocessor at all (zero hits in Rust; the
      hybrid path had no output postprocessing), so it was filed as its
      own fix and applied uniformly: `FormatIdentifier::is_markdown_output()`
      + `unescape_shortcodes_in_output` in `pandoc_write.rs`, called
      after a successful pandoc invocation for markdown-family formats,
      gated on the written file containing `{{\<`/`\>}}`
      (commit `cb9a00733`). Red-first: both the integration test and
      the real-binary e2e failed with output `{{\< meta title \>}}`.
- [x] New variants + mappings + `output_divs: Some(false)` rows
      (`Markdown` excepted): 7 variants (Markdown, MarkdownStrict,
      MarkdownPhpExtra, MarkdownGithub, MarkdownMmd, Markua,
      CommonmarkX) with all seams; writer names are explicit arms —
      the extension fall-through would send `-t md` (pandoc's plain
      markdown writer) for every flavor.
- [x] **gfm/commonmark completion:** e2e *reachability* tests for
      `--to gfm` / `--to commonmark` already landed in Phase 1 (see the
      Phase 1 checklist); content completion: D7 resolved 2026-09-24 —
      kept the Phase 1 bare `-t gfm` arm (no variant-string mechanism;
      see the D7 decision entry for the archaeology);
      `crossref-numbering: external` set for Gfm
      (`insert_crossref_numbering_mode`; renderer at
      `floatreftarget.lua:1191`; bare markdown/commonmark stay unset —
      placeholder float renderer — and the vendored `main.lua:737-752`
      fail-fast guard only rejects LaTeX/Typst targets, so no guard
      accommodation needed).
- [x] `KNOWN_BASE_FORMATS`: added the 7 Tier B bases (gfm/commonmark
      were already present).
- [x] E2E at least `markdown_strict` and `commonmark_x`; inspected.
      Invocation: `cargo run --bin q2 -- render target/tmp-tierb/f.qmd
      --to <fmt> --output-dir target/tmp-tierb/out-<fmt>` for **all
      nine** flavors (fixture: heading + emphasis + code + escaped
      shortcode; no code cells). Snippets (Escaped line): markdown,
      markdown_github, markdown_mmd, markua, gfm, commonmark,
      commonmark_x → `{{< meta title >}}` (postprocessor active);
      markdown_strict, markdown_phpextra → `{{&lt; meta title &gt;}}`
      (those two writers HTML-entity-escape; measured Q1 parity — its
      postprocessor does not rewrite them either). All nine produced
      non-empty `f.md`. Also committed as `e2e_render_markdown_strict` /
      `e2e_render_commonmark_x` (commit `4859efa60`).
- [x] Workspace nextest; commit. (commits `cb9a00733` Phase 3a +
      `4859efa60` Phase 3b.) Measured 2026-09-24 on this branch after
      both commits: 14,776 run / 14,776 passed / 200 skipped / 0
      failed. Delta vs the Phase 2 baseline (14,762 run) = **+14**, all
      accounted for by this phase's new `#[test]` fns: 7 in format.rs
      (`test_tier_b_*` × 6 + `test_is_markdown_output_negatives`), 3 in
      params.rs, 1 in pandoc_long_tail_formats.rs, 3 in the e2e file.
      Skipped unchanged.

### Phase 4 — Tier C stretch (12 variants)

- [x] **Test spec first:** parametrized render smoke per variant
      (`tier_c_smoke_all_variants`, `pandoc_long_tail_formats.rs`);
      chunkedhtml asserts a valid zip whose `index.html` shell is backed
      by a chapter entry carrying the body text (**correction during
      execution:** the chunked writer splits content into numbered
      `1-<slug>.html` files — `index.html` is only the shell, so the
      first draft's "body inside index.html" assertion was wrong and
      was fixed against measured output); ansi asserts `\x1b[` escape
      bytes; **bare invocation asserted at three sinks** —
      `test_tier_c_invocation_args_empty` (no CLI flags),
      `test_tier_c_pandoc_defaults_noop` (`format_pandoc_defaults`
      returns the no-op default for all 12, pinning "no defaults rows"
      at the sink), and an *output-level* title-marker check (fixture
      title `TierCMarkerTitle` must not appear in the 10 text writers'
      output — measured pandoc 3.11: `pandoc -t djot --standalone`
      prepends `# <title>`; the AST-dump `xml` writer legitimately
      echoes metadata so it's exempt). Red-first: all 11 new tests
      failed with `Unknown format: djot` before the variants landed
      (/tmp/tc-red.log).
- [x] Variants + mappings: 12 variants (Djot, T2t, Xml, Ansi, Vimdoc,
      Bbcode, BbcodeSteam, BbcodePhpbb, BbcodeFluxbb, BbcodeHubzilla,
      BbcodeXenforo, Chunkedhtml) with as_str/TryFrom/is_pandoc_hybrid/
      output_extension_for/pandoc_writer_name_for arms — all explicit,
      the extension fall-through would send `-t txt`/`-t dj`/`-t zip`.
      **No defaults rows anywhere** (bare invocation): none in
      `format_pandoc_defaults`, `format_execute_defaults`,
      `pandoc_invocation_args_for`, or `insert_crossref_numbering_mode`
      (placeholder float renderer — same polarity as the plaintext
      tail).
- [x] `KNOWN_BASE_FORMATS`: added the 12 Tier C bases (+1 unit test:
      `test_parse_format_descriptor_tier_c_bases`, incl. an
      underscore flavor through the last-hyphen split,
      `acm-bbcode_steam` → base `bbcode_steam`).
- [x] E2E djot + chunkedhtml through the real binary; inspected.

      Recorded 2026-09-24, fixture `target/e2e-tierc/f.qmd` (title
      `Tier C Fixture`, `# Head`, `HelloTierCBody with *emphasis*.`,
      2-item list), binary `./target/debug/q2` at branch tip:

      - **djot**: `cargo run --bin q2 -- render
        target/e2e-tierc/f.qmd --to djot --output-dir
        target/e2e-tierc/out-djot` → `f.dj`, body:
        `{#head}` / `# Head` / `HelloTierCBody with _emphasis_.` /
        `- one` / `- two`. No title chrome (bare invocation) — the
        standalone template would have prepended `# Tier C Fixture`.
      - **chunkedhtml**: `… --to chunkedhtml --output-dir
        target/e2e-tierc/out-chunked` → `f.zip` (3,629 bytes).
        `unzip -l`: `sitemap.json` (217 B), `index.html` (4,384 B),
        `1-head.html` (4,638 B). `unzip -p … 1-head.html` carries
        `<h1 data-number="1" id="head">` and `HelloTierCBody with
        <em>emphasis</em>` — the body lives in the chapter file, the
        shell in `index.html`.

      Both outputs inspected by hand. Also committed as
      `e2e_render_djot` / `e2e_render_chunkedhtml`.
- [x] Workspace nextest; commit. (14,786 run / 14,786 passed / 200
      skipped / 0 failed, measured 2026-09-24 on this branch; delta vs
      the Phase 3 baseline (14,776 run) = **+10**, all accounted for by
      this phase's new `#[test]` fns: 6 in format.rs (`test_tier_c_*`),
      1 in discover.rs, 1 in pandoc_long_tail_formats.rs, 2 in the e2e
      file. Skipped unchanged.)

### Phase 5 — Tier D JS slide formats (4 variants)

- [ ] **Test spec first:** per-variant test asserting the standalone
      deck references the format's JS asset (`slides.js`/`slidy.js`/
      `slideous.js`/dzslides' `reveal.js` shim) and has
      `class="slide…"` structure; execute-scope test pinning
      echo/warning false + fig 9.5×6.5; params-blob test asserting
      `crossref-numbering: external`.
- [ ] Variants + mappings; `format_execute_defaults` presentation rows;
      `pandoc_invocation_args_for` `--standalone --wrap none`
      (`--default-image-extension png` comes from
      `format_pandoc_defaults`, single sink); `crossref-numbering:
      external` for all four.
- [ ] `KNOWN_BASE_FORMATS`: add the Tier D bases.
- [ ] **Live verification through the real binary** (CLAUDE.md e2e
      rule): `cargo run --bin q2 -- render <deck.qmd> --to slidy` (and
      one other), open the output, confirm slide structure + that the
      deck is self-contained modulo pandoc's CDN/asset refs. Record
      invocation + snippet here.
- [ ] Workspace nextest; commit.

### Phase 6 — docs + wrap-up

- [ ] `docs/` format reference: list the new formats, the Q1-parity
      degradations section above (FloatRefTarget placeholders, no
      mermaid fallback, output-divs), per-tier notes (chunkedhtml zip,
      ansi nature), and the **Tier C filename change for Q1 migrants**
      (djot/t2t/xml now get pandoc-conventional extensions instead of
      Q1's blanket `.txt`; gfm/commonmark are newly reachable, with gfm
      output a documented *superset* of Q1's — bare `-t gfm` per D7
      adds GH alerts, `tex_math_gfm`, `yaml_metadata_block`,
      `auto_identifiers` vs Q1's frozen 2022 extension list).
- [ ] Optionally file Q1's `"texttile"` typo upstream at quarto-cli.
- [ ] Reconcile this checklist with reality (per global rule), commit
      the plan file.
- [ ] **Full `cargo xtask verify`** green before asking to push — repo
      policy requires the WASM leg for any change under `quarto-core`,
      and this plan adds 48 enum variants there.

## Decisions (resolved)

- **ansi kept** as a renderable format (Tier C, ext `txt`) — Gordon,
  2026-09-24.
- **Tier D gets an early live test** — done in Phase 0 (passed), per
  Gordon 2026-09-24.
- **bibtex/biblatex/csljson/opml dropped** — body-discarding writers
  (verified in pandoc source), not "long-tail document formats".
- **s5/dzslides/slidy/slideous are Tier D, not Tier A** — JS-capable
  (`isHtmlSlideOutput` in both Q1 TS and the vendored Lua), different
  verification story.
- **Tier C uses bare invocation** (Q1 `unknownFormat` parity), not the
  plaintext family defaults — pandoc ships default templates for
  ansi/djot/t2t/bbcode/vimdoc, so `--standalone` would change behavior
  vs Q1. (Corrects the first draft's "harmless no-op" claim.)
- **`Markdown` (bare pandoc markdown) keeps Q1's output-divs: true**;
  the other six markdown-family variants get `output-divs: false`.
- **gfm/commonmark pulled out of "already shipped"** — they're in the
  enum but unreachable and mis-wired (`-t md`); completed in Tier B.
- **`--default-image-extension` has exactly one sink**
  (`format_pandoc_defaults` → `build_forwarded_args`), never duplicated
  in `pandoc_invocation_args_for`.
- **D2 (Gordon, 2026-09-24): Tier A plaintext family runs Q1-parity
  args** — `--standalone` + `--default-image-extension png`, mirroring
  Q1's `plaintextFormat` (formats-shared.ts:188). Bare invocation stays
  Tier-C-only, where Q1 has *no* format (`unknownFormat("txt")`), so
  bare *is* parity there. Bare-everywhere was rejected: titles vanish
  and SVG figures break on ~19 formats where Q1 deliberately handles
  both.
- **D3 (Gordon, 2026-09-24): all 6 bbcode flavors ship** (base +
  `_steam`/`_phpbb`/`_fluxbb`/`_hubzilla`/`_xenforo`). Q1 supports
  none of them (all fall to `unknownFormat`), so zero parity risk
  either way; per-variant cost is uniform.
- **D6 (Gordon, 2026-09-24): `native`/`json` ship as debug aids**
  (pandoc AST dumps through the full pipeline — the same dump Phase 0's
  smoke test had to hand-build).
- **D7 (Gordon, 2026-09-24): gfm uses bare `-t gfm`, not Q1's**
  `commonmark+<8 extensions>`. Archaeology
  (`claude-notes/research/2026-09-24-q1-gfm-commonmark-motivation.md`):
  Q1's `+footnotes` (2021-10-04) and `+tex_math_dollars` (2022-05-20)
  were workarounds for bundled pandocs predating pandoc 2.15/2.19
  putting those in gfm's defaults; the `commonmark` base + frozen list
  dates to Sept-2022 refactor churn with no stated rationale. Bare
  `-t gfm` on pandoc 3.11 is a strict *superset* of Q1's string (adds
  `yaml_metadata_block` — which Q1 deliberately subtracted —
  `auto_identifiers`, `tex_math_gfm`, `alerts`). Tradeoff accepted:
  output tracks pandoc's evolving GFM defaults (snapshot churn on
  pandoc upgrades) instead of freezing 2022 GitHub. Deviation documented
  in Phase 6. `canonical_name()` still reports `"gfm"` (wrinkle 2).
