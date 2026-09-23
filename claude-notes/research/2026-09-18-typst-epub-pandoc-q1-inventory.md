# Research: typst & epub as pandoc-hybrid-writer follow-ons

**Date:** 2026-09-18 (Rev 2 — corrected after a blank-slate implementation review; see
"Rev 2 corrections" note at the end of each Part)
**Context:** The pandoc-hybrid-writer epic (`claude-notes/plans/2026-08-20-pandoc-hybrid-epic.md`,
design doc `claude-notes/designs/pandoc-hybrid-architecture.md`, worktree `workspace-1`,
branch `feature/pandoc-writer-hybrid`) targets docx + pptx first, with latex stubbed. Its
own docs name **typst, epub, and pdf** as anticipated Tier-2 follow-ons; this note scopes
the first two (pdf rides on typst/latex, not a follow-on in its own right).

**Live caveat (2026-09-18):** `ListAgents` shows three other active sessions
(`pandoc-hybrid-writer-impl-1/2/3`) apparently implementing/revising the epic itself right
now. At least one file reference below (`…-P7-implementation.md`, cited by the blank-slate
reviewer) postdates this note's original research pass — the epic's own docs are moving
targets while this follow-on is being drafted. Re-verify file/section references against
the epic's actual landed shape immediately before starting implementation, not just once
at epic-completion time.

This note consolidates five research passes (pandoc's Typst writer, pandoc's EPUB writer,
Quarto 1's typst format, Quarto 1's epub format, Q2's existing external-process/binary-
discovery infrastructure) plus corrections from a full blank-slate implementation review
(7 blockers, 12 design-decision gaps, 10 mechanical fixes — all verified against the repo).
It is the shared background for
[`2026-09-18-pandoc-hybrid-typst.md`](../plans/2026-09-18-pandoc-hybrid-typst.md) and
[`2026-09-18-pandoc-hybrid-epub.md`](../plans/2026-09-18-pandoc-hybrid-epub.md).

**Why docusaurus isn't here:** Pandoc has no Docusaurus/MDX writer at all — Q1 implements
`docusaurus-md` as a bundled *extension* riding on Pandoc's plain `markdown`/`gfm` writer,
with heavy per-custom-node Lua that bypasses the Pandoc writer entirely. That's a
fundamentally different shape from "reuse Pandoc's own writer," the epic's core premise.
epub replaced it — the epic's own docs already anticipate epub as a Tier-2 follow-on
alongside typst.

---

## Part 1 — Pandoc's Typst writer (upstream Pandoc, `~/src/pandoc`, commit `cd77c632a`, `3.8.1-684-gcd77c632a`, 2026-08-16)

- **Location:** `src/Text/Pandoc/Writers/Typst.hs` (735 lines — corrected, was miscounted
  as 736), `writeTypst`. A **Typst reader** also exists (bidirectional maturity), not
  relevant to Q2's write-only need. No live dependency on an external `typst-hs` AST
  package for the writer itself — it builds Typst source text directly via pandoc's own
  `Text.DocLayout` pretty-printer. The cabal dependency on package `typst >= 0.11 && < 0.12`
  is only for math/highlighting-style helpers (`styleToTypst`), not the writer's AST.
- **Mapping:** headings, emphasis/strong/strike/underline/sup/sub/smallcaps, lists, code
  blocks (fenced or Skylighting-highlighted `#raw`), math (routed through `texmath`'s Typst
  backend), images (`#image()`/`#box()`), tables (`#figure(table(...))` with explicit
  `columns:`/`align:`).
- **Known gaps:**
  - **Citation prefix is dropped** — `-- Note: this loses prefix` (`Typst.hs:533`, `toCite`).
    `[see @foo]` keeps the suffix (`supplement:`) but drops "see ".
  - **Hand-rolled escaper** (`escapeTypst` at `:621`, `needsEscapeAtLineStart` at `:666`)
    covering `[`, `]`, `#`, `<`, `>`, `@`, `$`, `` ` ``, `_`, `*`, `~`, leading `/`, `+`, `-`,
    `=` — the classic source of edge-case bugs on unusual input.
  - **Crossrefs — corrected (was wrong in Rev 1):** Pandoc's own `reference-type` handling
    (`Typst.hs:537`) is fed **only by Pandoc's LaTeX reader**
    (`src/Text/Pandoc/Readers/LaTeX/Inline.hs:67`) — it does **not** apply to Q2's input,
    which never goes through Pandoc's LaTeX reader. `reference-type` appears nowhere in
    q2's `crates/` or in Q1's `src/resources/filters/`. Q1's actual typst crossref path is
    its own Lua: `crossref/refs.lua:89-91` emits `RawInline('typst', '#ref(<label>,
    supplement: [...')` directly. A Q2 typst crossref needs the shim to reconstruct **this**
    path, not rely on Pandoc's writer inferring anything from `reference-type` attrs.
  - **Extension point Q1 already leans on**: `typst:*` / `typst:text:*` key-value attrs
    (`pickTypstAttrs` at `:138`) let Lua filters inject raw Typst properties.
  - `RawBlock`/`RawInline "typst"` passes through verbatim; any other raw format is
    **silently dropped**, no warning.
  - No literal `TODO`s, but several `-- see #NNNN` comments reference upstream issues
    (`#9104`, `#9252`, `#9389`, `#10661`, `#10805`, `#11044`, `#11210`, `#11511`, `#11568`)
    — search precedent if Q2 hits similar symptoms.
- **Test coverage:** `test/writer.typst` (854 lines, general AST golden test) + **5** narrow
  `test/command/typst-*.md` regression tests: `typst-auto-identifiers`, `typst-hs-80`,
  `typst-image-alt`, `typst-images`, `typst-property-output` (Rev 1 named only 4 of these).
- **Post-processing:** Pandoc's writer only emits `.typ` text — compiling to PDF is not
  Pandoc's job, confirming a Q2 typst format needs its own compile step (see Part 5).

## Part 2 — Pandoc's EPUB writer (same checkout)

- **Location:** `src/Text/Pandoc/Writers/EPUB.hs` (1572 lines), `writeEPUB2`/`writeEPUB3`.
  Builds the **full zip container directly** (mimetype, `container.xml`, OPF package doc,
  NCX/nav, CSS, chapter XHTML) — no external tool. Architecturally: split the AST into
  chapters via **`splitIntoChunks "ch%n.xhtml" (writerNumberSections opts) Nothing
  (writerSplitLevel opts)`** (`EPUB.hs:564-569` — corrected; there is no `splitByHeaders`
  function), render each chapter with Pandoc's own HTML writer (`writeHtmlStringForEPUB`),
  then zip the results. TOC/nav auto-generated from header hierarchy. Metadata
  (title/author/date/cover-image/language/identifier) via `Meta`
  (`getEPUBMetadata` at `EPUB.hs:176`, `metadataFromMeta` at `:342`), with support for
  merging an external OPF metadata XML file. Media goes through Pandoc's mediabag like any
  other writer. **`writerSplitLevel` is a real, user-facing knob** (default `1`,
  `Options.hs:385`), exposed via `--split-level`/legacy `--epub-chapter-level`
  (`App/Opt.hs:222`, `:607-609`) — this must land in the per-format defaults allow-list, not
  be treated as automatic. Pandoc also **synthesizes a level-1 header from the document
  title** when the document has none (`EPUB.hs:555-562`) — worth a golden-test case.
- **Known gaps:** minimal — one performance TODO (inefficient manifest lookup, `:592`) and
  one closed-issue workaround (cover-image naming, `#4206`, `:762`). No open
  unsupported-construct markers. Mature, low-surprise writer.
- **Test coverage:** `test/epub/` (8 fixture `.epub` files) + `test/Tests/Readers/EPUB.hs` —
  reader-side round-trip fixtures (write→read→compare), not dedicated golden writer tests.
- **Post-processing:** **None.** EPUB is "Pandoc writes final bytes, done" — same shape as
  docx/pptx, unlike typst. `epub` maps to `writeEPUB3` (`Writers.hs:165-167`) — Pandoc's
  `--to epub` really does default to epub3.

## Part 3 — Quarto 1's typst format (`external-sources/quarto-cli`, pinned to release `v1.11.3` — the epic's own vendoring pin; checked HEAD is `v1.11.5-6-g0c497cd6d` but every file cited below is byte-identical between the two)

- **Registration:** `src/format/typst/format-typst.ts` (`typstFormat()`): `pandoc.standalone
  = true`, `default-image-extension: svg`, `wrap: none`, `citeproc: false` (Quarto's own
  citeproc preferred; auto-adds a `-citations` variant if the user opts into Pandoc's
  native citeproc). One postprocessor: `skylightingPostProcessor` (regex patch for a
  Skylighting/Typst `block()` styling bug, upstream issue #14126).

  **Three more `formatExtras` behaviors, not in Rev 1:**
  - Sets `section-numbering: "1.1.a"` when `number-sections` is on (`:82-90`) — this is the
    one place typst intersects Q2's general section-numbering gap (`bd-5aklrxgi`); the
    intersection is incidental, not a fix for that gap.
  - Sets **`shift-heading-level-by: -1`** when the document has no level-1 heading
    (`:92-100`) — user-visible, not mentioned anywhere else in this research.
  - Moves `columns` from pandoc options into metadata (`:118-123`), and resolves brand
    logos to project-absolute paths (`:102-117`).

- **Lua filters — corrected inventory (Rev 1 mis-attributed one file's contents entirely).**
  **7 files, 2416 lines total** (Rev 1 said 6 files / ~2086 lines):
  - **`filters/quarto-post/typst.lua` (330 lines)** — `render_typst()` (`:25`) and
    `render_typst_fixups()` (`:207`). **This is the file that does margin notes (via the
    `marginalia` package), `.block` div → `#block(...)`, citeproc-aware margin citations,
    unnumbered-heading handling, image px→in DPI conversion, alt-text injection (working
    around Pandoc bug #11394), `#align()` for `fig-align`, and `typst:no-figure` table
    marking.** Rev 1 wrongly attributed all of this to `filters/layout/typst.lua`.
  - **`filters/layout/typst.lua` (385 lines)** — its *actual* content: `getWideblockSide`,
    `make_typst_wideblock`, `make_typst_margin_figure`, `make_typst_margin_caption_figure`,
    `make_typst_figure`, `render_floatless_typst_layout` — figure/panel **layout**, not
    margin notes. (`layout/meta.lua` also references marginalia, separately.)
  - `filters/modules/typst.lua` (112 lines) — `quarto.format.typst` helper API
    (`function_call`, `as_typst_content`, `as_typst_dictionary`), requires
    `modules/typst_css`.
  - `filters/modules/typst_css.lua` (844 lines) — CSS→Typst property translation (colors,
    fonts, spacing), including brand.yaml fallback-font-list filtering (issue #12556,
    Typst-version-sensitive).
  - `filters/quarto-finalize/typst.lua` (29 lines) — crossref metadata → `doc.meta.crossref`
    plumbing, marked **`-- FIXME finish this`** in Q1 itself. Treat as incomplete reference,
    not a model to copy verbatim.
  - `filters/quarto-post/typst-brand-yaml.lua` (379 lines) +
    `typst-css-property-processing.lua` (337 lines) — brand.yaml → Typst typography/color
    mapping.
- **What's template-level vs. Lua-level:** callouts, subfloats, and numbering are handled
  at the **template level** — `definitions.typ`'s `callout()` function (`:162`),
  `quartosubfloatcounter`/`quarto_super`, a callout-figure show rule. `definitions.typ` also
  does an **unconditional** `#import "@preview/marginalia:0.3.1"` (`:188`) — the marginalia
  package must be staged **regardless of whether the document uses margin notes**, because
  the template imports it unconditionally, not the Lua.
- **Template scope — corrected count.** **8 files total** in
  `src/resources/formats/typst/pandoc/quarto/`: `template.typ` (the orchestrator, passed as
  `--template`) plus **7** partials it includes: `numbering.typ` (defines
  `equation-numbering`, `callout-numbering`, `subfloat-numbering`, `theorem-numbering`,
  `theorem-render` — consumed directly by `crossref/equations.lua:129`'s
  `numbering: equation-numbering` output), `definitions.typ` (largest — utility
  functions, callout/subfloat/code-block styling, the marginalia import), `typst-template.typ`
  (the `article()` function), `page.typ` (geometry/logo), `typst-show.typ`
  (metadata/brand.yaml → `article()` params), `notes.typ` (endnotes), `biblio.typ`.
  (Rev 1 said "7 modular partials" and listed 8 names — off by one; also, Rev 1's
  "`default.typst`" doesn't exist — the two **unmodified reference copies of Pandoc's own
  template**, for users who supply a fully custom template, are
  `src/resources/formats/typst/pandoc/template.typst` and `.../definitions.typst`, plus
  `.../typst.template`.)
- **Delivery mechanism to Pandoc (missing from Rev 1 entirely):** Pandoc has no `--partial`
  flag — partials resolve relative to the template file Pandoc is given. Q1 materializes
  `template.typ` plus all 7 partials into one directory and passes `--template` pointing at
  the orchestrator (`format-typst.ts:128-141` builds this `templateContext`). A Q2 port
  needs the same staging step: write all 8 files into one directory at render time (or
  reference a fixed resource location), then pass `--template` at that path.
- **Vendored assets:** 5 Typst packages (fontawesome, marginalia, octique, showybox,
  theorion) + 3 embedded Font Awesome fonts. Asset-vendoring tasks, not logic ports — copy,
  don't rewrite. **Note the unconditional-marginalia-import point above**: at minimum
  `marginalia` must always be staged, not treated as an opt-in "Phase 4" package.
- **Known Q1-side workarounds citing upstream Pandoc bugs**, all found in
  `filters/quarto-post/typst.lua` (not `layout/typst.lua` — corrected):
  image alt-text passthrough (`:228`, #11394), height/width unit handling (`:217`, #9945),
  table-wrapped-in-figure (`:305`, #10438); Skylighting block styling (#14126) is handled by
  the `format-typst.ts` postprocessor, not this file. Checking whether these are fixed
  requires checking against **the epic's pinned minimum pandoc version (3.10, per P4's
  `resources/pandoc-filters/README.md`)** — the epic deliberately does **not** bundle
  Pandoc, so "already fixed" means "fixed by 3.10," not "fixed in whatever pandoc happens
  to be on the developer's PATH."
- **Crossref hookup:** `crossref/equations.lua` has a live `isTypstOutput()` branch
  (`:115`) — Typst gets `#math.equation(numbering:...)`, other formats get
  `\label{}`/text-fallback. This branch existing is a head start, but **confirming it
  engages end-to-end requires `numbering.typ`'s `equation-numbering` symbol to already be
  staged** (see the phase-ordering note in Part 5/the typst plan) — it cannot be verified as
  an isolated Phase 1 item before the template partials land.
- **Brand bridge — corrected (Rev 1 pointed at the wrong Q2 file).** Q1's typst brand
  filters read a serialized `brand` filter param. The Q2-side bridge is **not**
  `crates/quarto-core/src/brand_fonts.rs`, which is narrowly about publishing `source: file`
  fonts beside theme CSS — the actual brand parser is the **`crates/quarto-brand` crate**
  (`ResolvedBrand`, `BrandFont`), plus `crates/quarto-sass`'s `brand_to_layers`. The real
  task is serializing `ResolvedBrand` into the shape `typst-brand-yaml.lua` expects as a
  filter param — a schema-matching job against an existing, working brand parser, not a Lua
  port and not "check if brand support exists" (it does).
- **Filter-param sources — corrected (Rev 1 called a table row a "seam"; it isn't code).**
  `extractTypstFilterParams` is a **row in a survey table** in the epic's P4 shape plan
  (`claude-notes/plans/2026-08-20-pandoc-hybrid-P4-run-machinery.md`, "typst-specific | No —
  typst only") — there is no code seam to "wire," an implementer searching for one will find
  nothing and must design the contributor from scratch. The same table also names
  `extractColumnParams`/`quartoColumnParams` ("No — HTML/typst margin-notes feature") as
  unplumbed — exactly what the margin-notes work needs — plus a top-level
  `quarto-environment.paths.Typst` literal (`command/render/filters.ts:208`,
  `"Typst": typstBinaryPath()`) and a `typst-available-fonts` param (fed by
  `getAvailableTypstFonts()`, see Part 5).
- **`keep-typ` already exists in Q1 — not an open decision for a "full parity" plan.**
  `kKeepTyp = "keep-typ"` (`config/constants.ts:88`), typed at `config/types.ts:473`, forced
  on in debug mode alongside `keep-tex` (`config/metadata.ts:131-134`), consumed at
  `command/render/output-typst.ts:296`. Port it directly: discard the intermediate `.typ`
  unless `keep-typ: true` (matching `keep-tex`'s default).
- **PATH-discovery vs. bundled-binary asymmetry.** Q1's `typstBinaryPath()`
  (`core/typst.ts:20-23`) is `QUARTO_TYPST || architectureToolsPath("typst")` — Q1 *bundles*
  typst. `validateRequiredTypstVersion()` (`:198-227`, min typst `>=0.8`) runs **only** when
  `QUARTO_TYPST` is set, precisely because the bundled binary is known-good. Q2's decision
  (PATH/`QUARTO_TYPST`-only discovery, no bundling) means Q2 must run version validation
  **unconditionally** — there's no "known-good bundled" case to skip it for.

## Part 4 — Quarto 1's epub format (same checkout)

- **Registration:** `src/format/epub/format-epub.ts` — 51 lines, by far the thinnest format
  module found. Built on the shared `createEbookFormat("ePub", "epub")` helper. Real
  Quarto-specific logic: (a) `html-math-method` = `webtex` for epub2 / `mathml` for epub3
  (epub2 readers can't render MathML — this is a **Pandoc** option, so the wire-format cut
  must hand Pandoc **unresolved `Math` inlines** for the epub leg, not KaTeX-rendered HTML
  from Q2's HTML path; **unverified which side of Q2's math handling this falls on — flag
  for the epub plan, don't assume**), and (b) a book-project-only hook
  (`onSingleFilePreRender`) that — **corrected, Rev 1 described this backwards** — *reads*
  `format.pandoc[kEPubCoverImage]` as a guard (`:38-46`) and *sets*
  `format.metadata[kBookCoverImage]` (i.e. `cover-image`), not the other way around. Doesn't
  change the scope-out decision (still irrelevant outside book-mode); the mechanism
  description was wrong.
- **No epub-specific Lua filter *file* exists** — `createEbookFormat` just injects two
  HTML-family includes: `styles-callout.html` (shared with the plain HTML format) and
  `formats/epub/styles.html` (one CSS file, mostly Quarto's generic `.quarto-layout-*`
  panel/figure CSS).
- **But dedicated epub-conditional logic *does* exist inside shared filters — corrected,
  Rev 1's "no `isEpubOutput()` branch" claim was false:**
  - `crossref/sections.lua:48` — `if not _quarto.format.isEpubOutput() and
    numberSectionsOptionEnabled() ...` — a real epub-specific crossref branch.
  - `customnodes/callout.lua:139-141` — a full `_quarto.ast.add_renderer("Callout", ...)`
    predicated on `isEpubOutput() or isRevealJsOutput()`, with its own ~40-line render body.
    This is a dedicated renderer Q2's epub leg needs to exercise, not "attribute cleanup."
  - `customnodes/panel-tabset.lua:264` — epub explicitly routes to
    `render_tabset_with_l4_headings`.
  - (`book-numbering.lua:110` genuinely *is* book-mode-only — that one Rev-1 claim holds.)

  Whether Q2's epub leg needs to port these three behaviors, and whether Q2's HTML callout
  CSS matches the class names the shared Callout renderer emits for epub, is a real open
  question — not something "no open forks remain" (Rev 1's framing) can assert away.
- **`merge-includes: false`** (`formats-shared.ts:157-159`) keeps the two `include-in-header`
  CSS files separate — omitted from Rev 1's defaults list.
- **Scope indicator:** exactly one epub-specific resource file
  (`src/resources/formats/epub/styles.html`). No bundled fonts, no bundled packages, no
  template partials — still, genuinely, the thinnest of the three formats researched. The
  correction above adds real (if modest) scope; it doesn't overturn "epub is the simple
  one."

## Part 5 — Q2's existing external-process / binary-discovery infrastructure, and the typst compile step's real shape

- **Binary discovery abstraction already exists and already covers typst — verified, not
  just plausible.** `crates/quarto-core/src/render.rs`'s `BinaryDependencies` struct has a
  `typst: Option<PathBuf>` field (`:133`), populated by `discover()` (`:150-158`) via
  `runtime.find_binary("typst", "QUARTO_TYPST")` (`:155`) — the same env-var-then-PATH
  pattern used for `pandoc`, `git`, `dart_sass`, `esbuild`. **Currently unconsumed** — the
  only other reference is a test assertion (`render.rs:606`). `find_binary`
  (`crates/quarto-system-runtime/src/native.rs:371`): env var first, then `which::which`.
- **The real render-path gate — corrected (Rev 1 conflated two different files).** The
  native-format check is at **`crates/quarto/src/commands/render.rs:680-684`**
  (`if !format.identifier.is_native() { anyhow::bail!(...) }`), not
  `crates/quarto-core/src/render.rs` (that file holds `BinaryDependencies`, a different
  concern). `is_native()` itself is defined at `crates/quarto-core/src/format.rs:58-60`.
  **`FormatIdentifier::Typst` and `::Epub` already exist** (`format.rs:31,33`, `as_str` arms
  at `:49-50`, `TryFrom<&str>` arms at `:89-90`, non-native test assertions at `:635-636`) —
  the epic's own P7 companion plan already knows this and plans to replace the blanket
  native-only gate with `matches!(…, Docx | Pptx)`, explicitly **keeping** the refusal for
  `Pdf | Epub | Typst | Gfm | CommonMark`. So a typst/epub follow-on's real Phase-1 item is
  "add a `Typst`/`Epub` arm to that allow-match," not "add an enum variant" (there is
  nothing to add there).
- **`quarto typst` CLI subcommand already exists as a stub**
  (`crates/quarto/src/commands/typst.rs`: `Err(QuartoError::NotImplemented("typst"))`),
  parallel to `quarto pandoc`'s identical stub — a placeholder for a future
  "Quarto bundles/exposes a copy of the tool" feature, not a blocker for a Rust-side compile
  stage that shells out (or links) directly.
- **No typst auto-install exists.** `quarto install <target>` currently only documents
  "TinyTex or Chromium." Per the 2026-09-18 design discussion, v1 does **not** add one;
  PATH/env-var discovery only, with a clear diagnostic if missing — but see the version-
  validation asymmetry noted in Part 3: since Q2 doesn't bundle a known-good typst, version
  validation must run unconditionally, not only on override.
- **Subprocess precedent:** `crates/quarto-core/src/engine/knitr/subprocess.rs` — binary
  discovery cached in a `OnceLock`, `Command::new` (`:363`) + `Stdio::piped()` (`:368-372`)
  + `.spawn()` (`:378`) + `.wait_with_output()` (`:395`), explicit "binary not found" error
  path. A typst-compile stage should follow this same shape for the `typst` binary itself.
- **No existing PDF-producing capability today** outside this epic — no
  tinytex/latexmk/wkhtmltopdf/weasyprint integration found anywhere in `crates/`. The typst
  compile step is the **first** PDF-producing code path in Q2.

### The compile step is substantially more than "shell out to `typst compile`"

Q1's `core/typst.ts` and `command/render/output-typst.ts` do real orchestration around the
bare `typst compile` call, none of which was in Rev 1:

| Q1 source | What it does |
|---|---|
| `core/typst.ts:140-172` (`typstCompile`) | passes `--root`, `--package-path`, `--package-cache-path`, `--pdf-standard`, `--font-path` |
| `core/typst.ts:25-38` (`fontPathsArgs`) | `--font-path` **ordering is load-bearing** — Quarto's font paths must come first for its template to resolve fonts correctly |
| `format-typst.ts:104-106` | "Typst resolves `/` paths via `--root`, which points to the project directory" — **this is exactly Q2's leading-`/`-means-project-root convention** (`claude-notes/designs/path-resolution-model.md`); omitting `--root` breaks every project-root-relative image/logo reference |
| `core/typst.ts:198-227` (`validateRequiredTypstVersion`) | enforces typst `>=0.8`; in Q1 only runs when `QUARTO_TYPST` overrides the bundled binary — in Q2 this must run unconditionally (no bundled known-good binary) |
| `core/typst.ts:49-119` (`getAvailableTypstFonts`) | a **second** `typst` subprocess invocation (`typst fonts`), two-level cached, feeding the `typst-available-fonts` filter param (`command/render/pandoc.ts:1686-1697`) — needed for the font-fallback-filtering workaround (issue #12556) |
| `command/render/output-typst.ts` (410 lines) + `core/typst-gather.ts` (72 lines) | a **second bundled binary in Q1**, `typst-gather`, analyzes the `.typ` output and stages needed Typst packages before compiling |

**Without package staging, `typst compile` attempts a network fetch** of `@preview/marginalia`
(and any other `@preview` package the template/Lua references) at render time — unacceptable
for a hermetic/offline build.

**Resolved 2026-09-18: `typst-gather` is not a black box to re-shell-out to.** It is already
a Rust crate, `quarto-dev/typst-gather` (checked out locally at `/Users/gordon/src/typst-gather`,
depends on `typst-kit`/`typst-syntax`), with a real library API in `src/lib.rs` —
`analyze()`, `gather_packages()`, `Config` — behind a thin `clap`-based `main.rs` CLI. Q1 has
to shell out to it because Q1 is TypeScript/Deno and cannot link Rust code; **Q2 has no such
constraint.** Decision: refactor/depend on `typst-gather` as a **linked Cargo dependency**
called in-process from the new compile stage (calling `analyze()`/`gather_packages()`
directly), not vendored as a second bundled binary and not re-implemented from scratch. This
removes an entire "discover a second external binary" problem the Q1 architecture has.
