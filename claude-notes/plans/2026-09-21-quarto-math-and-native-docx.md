# quarto-math and the native docx writer

**Epic:** bd-pq9k90z2 (native docx writer)
**Strand (phase 1, this branch):** bd-entbg6x3 (quarto-math)
**Branch:** `braid/bd-entbg6x3-quarto-math`
**Status:** executing Phase 0 (started 2026-09-21 afternoon; decisions 5–9 below settled with the user).
**Related, parallel experiment:** the pandoc-hybrid leg (bd-fzqykm0n,
bd-ymkkrn64; plan file lives on a colleague's branch as
`claude-notes/plans/2026-09-18-pandoc-hybrid-P5-implementation.md`) reaches
docx/pptx/typst through Lua filters plus Pandoc. Both approaches proceed in
parallel until we decide which is easier, cleaner, and faster to land.

## Overview

Goal: `Pandoc` (the pampa struct) to `.docx`, written directly by pampa with
no Pandoc binary, in a way that

1. survives Word (well-formed OOXML, not merely "LibreOffice opens it"),
2. carries the source `.qmd` and its source map inside the package
   (redoc-style, Noam Ross's R package) so a Word comment can be mapped back
   to a `.qmd` location, and
3. renders math, which in Word means OMML.

Item 3 forces a math conversion layer, and a Typst writer needs the same layer
for Typst math. So the first deliverable is a `quarto-math` crate, a
micro-pandoc for math expressions: one reader (mitex's LaTeX parser), a spec
we own, and one writer per target (OMML first, Typst second, MathML later).

### Licensing ground rules

- **Off limits:** pandoc and texmath (GPL). Do not read their source for this
  work.
- **Usable:** python-docx (MIT, `external-sources/python-docx`), mitex
  (Apache-2.0, `external-sources/mitex`), Typst's `codex` symbol crate
  (Apache-2.0), ECMA-376 (the OOXML standard, freely available), redoc (MIT,
  `external-sources/redoc`, read for its embedding mechanism only).
- Vendored artifacts keep their upstream license notice next to them.

## Research summary (2026-09-21)

Evidence for every claim below is in the session transcript; the probe tools
are preserved under `claude-notes/research/2026-09-21-quarto-math-probes/`.

### Validating docx without Word

Three layers, no single tool covers all:

| Layer | What breaks | Tool |
| --- | --- | --- |
| OPC package | parts missing from `[Content_Types].xml`, dangling rels, overrides naming a directory (pandoc #11378) | in-tree Rust check (cheap, always on) |
| WordprocessingML schema | child order in `w:pPr`/`w:rPr`, unknown children | `xmllint --schema` with vendored ECMA-376 / Microsoft XSDs (what pandoc's CI does via a modified `devoidfury/docx-validator`) |
| Semantic | duplicate bookmark ids, tables without `w:tblGrid`, numbering ids that don't resolve | Open XML SDK `OpenXmlValidator` via `OOXMLValidatorCLI` (.NET) or `@xarsh/ooxml-validator` (npm, standalone) — nightly, has known false negatives (dotnet/Open-XML-SDK #592) |

LibreOffice headless is a visual smoke test only (too permissive). python-docx
opens parts lazily and is not a validator. Word remains the only ground truth;
keep a fixture corpus for occasional manual passes.

### Sidecars survive Word (the redoc mechanism)

`[Content_Types].xml` maps every part to a MIME type via `Default` (by
extension) and `Override` (by exact part name). "No overrides for directories"
means only that a part name must be a file; extra parts are fully allowed.
redoc drops files into a top-level `redoc/` directory, adds a `Default` entry
per extension, and adds one relationship per file from
`word/_rels/document.xml.rels` with a custom `Type` URI. **Verified:** redoc's
shipped `example-edited.docx` was saved by Word 16 and still contains all eight
sidecar files and relationships. Parts not reachable through a relationship
are dropped on save. Google Docs drops everything unknown (redoc #33);
LibreOffice preservation is unverified against a current release.

### python-docx as the clean-room OOXML reference

- `src/docx/oxml/`: declarative element model. Statically extracted (stdlib
  `ast`, no lxml): 77 `CT_*` classes, 134 ordered child declarations, 80 typed
  attribute declarations, 22 literal XML templates (inline picture, table
  skeleton, numbering). This is where Word's child-order strictness is written
  down.
- `src/docx/opc/`: self-contained package layer (~600 lines: content types from
  parts, relationships, part naming, every reltype/content-type URI in
  `constants.py`).
- `src/docx/templates/default.docx`: Word-generated, 160 styles (Heading 1–9,
  List Bullet/Number 1–3, Quote, Caption, Table Grid, Strong, Emphasis,
  Title/Subtitle…), 9 list definitions. Usable reference doc on day one after
  stripping the duplicate `stylesWithEffects.xml` and the thumbnail.
- Not in python-docx (write from ECMA-376): hyperlink writing, footnotes,
  bookmarks, fields (TOC), math. Partially there: numbering (element classes
  exist, list-instance assignment is ours).

Port **data, not code**: python-docx's insert-before machinery exists because
lxml trees are mutable; typed Rust structs whose fields serialize in
`_tag_seq` order make ordering errors impossible by construction.

### mitex as the math front end

- `mitex-parser` produces a lossless rowan CST whose *shape* is spec-driven
  (argument binding by arity: `\frac` gets two `ClauseArgument`s,
  `\sqrt[3]{x}` a bracket and a curly), with `ItemAttachComponent` for `_`/`^`
  (nested, left-assoc), `ItemLR` for `\left…\right`, `ItemEnv` for
  environments (flat body; `&` and `\\` are tokens, cell splitting is the
  writer's job), infix `\over` bound to its operands, and `\newcommand` macros
  expanded in the lexer (the definition vanishes from the tree).
- Unknown commands parse leniently (zero-arg command followed by a plain
  group); only the writer errors. Good: that is where we attach a source span.
- The *meaning* of commands lives in Typst closures in
  `packages/mitex/specs/latex/standard.typ` (132 commands, 22 envs, 583
  symbols). The Rust-side spec carries only arg shape + a Typst alias. So an
  OMML writer needs its own semantic table. Symbol aliases are Typst symbol
  names (`lt.closed`), which `codex` maps to Unicode, so the symbol half can be
  derived.
- The spec artifact is a binary (`default.rkyv`) in a git submodule,
  regenerated by running the `typst` binary over Typst source. We will not
  inherit that build step; the spec has a serde JSON form and the parser needs
  only arg shapes.
- crates.io is stale (0.2.4, 2024-06) vs the repo (0.2.7, 2026-07): git
  dependency or vendored copy.
- mitex's own Typst writer emits calls into its Typst prelude package
  (`mitexsqrt`, `textmath`, `zws`, `pmatrix`), so reusing it means every Quarto
  Typst document imports the mitex package. We write our own Typst emitter
  against the same tree instead.
- Words are coarse: `i=1` and `x>0` are single `TokenWord`s. OMML tolerates
  this (Word classifies characters at render time); a MathML writer needs a
  splitting pass (identifier / operator / number), which is a pure function of
  the token text and keeps spans exact.

### Source maps through the mitex tree

- rowan gives every node/token a byte `text_range()`; for macro-free input the
  annotation is `SourceInfo::substring(math_text_info, start, end)` — the same
  parent mechanism quarto-yaml's `make_source_info` uses, minus the
  char-to-byte conversion.
- With macros, rowan ranges index the *expanded* text. But mitex tokens are
  `(Token, &'a str)` with the str always a slice of the original input (macro
  bodies borrow the definition site, macro arguments the use site; nothing is
  synthesized). **Verified with the probe:** a second lexer pass with the same
  `MacroEngine`, zipped against the tree's leaves, aligned 1:1 on every input
  and recovered original spans (`\newcommand{\pr}[1]{P(#1)} \pr{x>0}` → `P`
  at the definition, `x>0` at the use site). Since we own the parser copy, the
  cleaner implementation records this side table at the parser's three
  `builder.token` call sites rather than lexing twice.
- Diagnostics inside an expansion point at the macro definition (as TeX does).
  Use-site attribution would need a marker token from the macro engine; later.
- **pampa gap:** `Inline::Math.source_info` covers the whole `$…$` node but
  `text` is not a plain substring of it on multi-line math (soft-break
  collapse, gutter strip in `crates/pampa/src/pandoc/treesitter.rs`). The
  reader must record a `Substring` (single line) or `Concat` (multi-line)
  mapping for the text.

### Math converters surveyed (for the record)

| Crate | Converts | License | Last release |
| --- | --- | --- | --- |
| mitex | TeX → Typst | Apache-2.0 | 2024-06 (repo 2026-07) |
| pulldown-latex | TeX → MathML | MIT | 2026-07 |
| math-core | TeX → MathML | MIT | 2026-09 |
| tex2typst-rs | TeX → Typst | GPL-3.0 | off limits |
| codex | Typst symbol names → Unicode | Apache-2.0 | 2026-06 |

No permissive TeX→OMML or MathML→OMML exists. OMML is MathML-shaped
(fraction, radical, sub/sup, n-ary, delimiter, matrix), so writing the OMML
emitter from ECMA-376 Part 1 §22.1 is bounded work.

## Phases

### Phase 0 — prerequisites and fixtures (TDD spine)

- [ ] Math fixture corpus under `crates/quarto-math/tests/fixtures/`: one
      `.tex` snippet per construct we must support (fractions, roots with
      index, sub/sup incl. nested and `\limits`, `\left…\right` incl. `.`,
      `\text`, `\mathrm`/`\mathbf`/`\mathbb`, `\operatorname`, big operators
      with limits, accents, `\binom`, `aligned`/`cases`/`pmatrix`/`bmatrix`,
      `\newcommand` with and without args, spacing commands, Unicode input,
      unknown command, unbalanced braces) plus the math actually used in
      `docs/` and the Q1 test corpus (grep `$` in `.qmd` fixtures).
- [ ] pampa: `Inline::Math` text mapping — record `SourceInfo::substring` /
      `concat` for the math *text* (bd-q6ed / bd-qpa2 touch the same code;
      coordinate). **Decision 5:** done as its own strand and PR against
      `main` (worktree under `.worktrees/`), then merged into this branch
      locally so both the native and pandoc-hybrid legs get it. Test: for
      every fixture in `crates/pampa/tests/…` with math, mapping `text`
      offset 0 and `text.len()` back through `SourceContext` lands inside the
      node span and on the right line.
- [ ] Vendor mitex (decision 2/6): `crates/quarto-math/vendor/mitex-{lexer,parser,spec,glob}`
      as separate crates, edition pinned to 2021, upstream `LICENSE` copied
      next to each, one `VENDORED.md` naming the upstream commit (`985d8e7`,
      2026-07-07) and listing every local patch. Keep upstream test suites.
      Strip the rkyv feature from mitex-spec.
- [ ] Dump mitex's prebuilt spec (`default.rkyv`, artifacts submodule; a copy
      is in the room-5 checkout) to JSON with a throwaway tool built against
      `external-sources/mitex`, so the symbol rows can be generated without
      the Typst build step. (`typst` is now installed locally too, so
      `typst query` regeneration is a fallback.)
- [ ] Vendor the OMML schema under `crates/quarto-math/tests/schemas/`:
      `shared-math.xsd`, `shared-commonSimpleTypes.xsd` and W3C's `xml.xsd`,
      taken from `external-sources/python-docx/ref/xsd/` (MIT notice kept),
      with the one-line import patch (`schemaLocation="xml.xsd"` on the
      `xml:` namespace import) that libxml2 needs. Test helper skips (with a
      clear message) when `xmllint` is absent. Verified 2026-09-21: with the
      patch, `xmllint --schema` accepts a correct `m:oMath` (frac, sSup, nary)
      and rejects `m:den` before `m:num` with a precise message.

### Phase 1 — `quarto-math` crate (bd-entbg6x3)

Tests first for every item; each writer test is a snapshot of the emitted
markup for a fixture plus, for OMML, an `xmllint --schema` pass.

- [ ] Crate skeleton `crates/quarto-math` (workspace member, `wasm32`-clean:
      no std::fs at runtime; spec embedded via `include_str!`).
- [ ] **Spec** `spec/commands.json`, owned by q2: per command/env/symbol →
      arg shape (mitex's `ArgShape`/`ArgPattern` JSON form), semantic kind for
      OMML (`Frac`, `Rad`, `Nary{op}`, `Accent{char}`, `Func`, `Sym{codepoint}`,
      `Matrix{delims}`, `Text`, `Style{variant}`, …), Typst alias. Generate the
      symbol rows from mitex's spec + `codex`; hand-write the ~150 structural
      rows. Test: every mitex spec entry has a row; every row's codepoint is a
      valid scalar.
- [ ] **Reader**: `parse(text, &Spec) -> Cst` via mitex-parser with a
      leaf-index → original-span side table (record at the `builder.token`
      call sites; fallback: the two-pass zip from the probe). Test: probe
      cases as unit tests, including both macro cases; property test that
      spans are within bounds and non-decreasing per leaf for macro-free
      input.
- [ ] **Normalization** `Cst -> MathAst`: flatten nested attachments into
      one `SubSup`, bind `\limits`/`\nolimits`, split env bodies into rows and
      cells, resolve `ItemLR` delimiters, attach `SourceInfo` to every node
      (`substring` of the math text info; node span = union of leaves).
      Test: snapshot `MathAst` for every fixture.
- [ ] **Diagnostics**: unknown command, arity mismatch, unbalanced
      delimiters, unknown environment → `quarto-error-reporting` diagnostics
      with the node's `SourceInfo`. New `Q-` codes need catalog entries **and**
      pages under `docs/errors/<subsystem>/` **and** sidebar entries in the
      same commit (`cargo xtask lint` enforces both).
- [ ] **OMML writer** `MathAst -> String` (`m:oMath` / `m:oMathPara` for
      display). Test: snapshots + `xmllint` against the vendored schema.
- [ ] **Typst writer** `MathAst -> String` emitting plain Typst math (no
      mitex prelude). Test: snapshots; optional compile check when `typst` is
      on PATH (skip otherwise, say so).
- [ ] **Word splitting** pass (`TokenWord` → identifiers / numbers /
      operators) behind a flag; needed by MathML, harmless for OMML. Test: spans
      of split children partition the parent span.
- [ ] pampa integration point: `quarto_math::convert(&Math, Target) ->
      Result<String, Diagnostic>` with the verbatim-TeX-in-code-style fallback
      the writers use on error.
- [ ] `cargo xtask verify --skip-hub-build`, then full `cargo xtask verify`
      if anything under `quarto-core`/`pampa` changed (the pampa Math mapping
      does).

### Phase 2 — OPC package layer + validation harness (new strand under the epic)

- [ ] Port `python-docx/src/docx/opc/` by hand onto `zip` + `quick-xml`
      (both already in the workspace): parts, content types (`Default` vs
      `Override` rules), relationships, part naming, constants table.
- [ ] In-tree OPC validator (Rust test helper): every part covered by content
      types, every rel target exists, no directory part names, XML parts
      well-formed.
- [ ] Vendor the WordprocessingML transitional schemas; `xmllint` step in
      the test helper (skip with message when absent); wire the same step into
      CI in the same commit (`cargo xtask verify` ↔ CI drift rule).
- [ ] Nightly Open XML SDK validation job (`actions/setup-dotnet` +
      `OOXMLValidatorCLI`), advisory.

### Phase 3 — docx writer in pampa (new strand under the epic)

- [ ] Vendor stripped `default.docx` into `resources/docx/` with the MIT
      notice; document what was removed.
- [ ] Generate `crates/pampa/src/writers/docx/oxml_model.rs` from the
      python-docx extraction JSON (typed property-container structs with
      fields in `_tag_seq` order; constants). Move
      `extract_python_docx_oxml.py` into `crates/xtask` and commit the
      generated file with a header naming the python-docx commit (`e454546`,
      v1.2.0).
- [ ] AST walker `crates/pampa/src/writers/docx.rs`, shaped like the HTML
      writer's `write_block` / `write_inline`. First subset: Para, Plain,
      Header, CodeBlock (highlighter runs → colored runs), BlockQuote,
      Bullet/OrderedList (numbering instances), Table (incl. merges, widths,
      Caption style), Figure/Image (EMU sizing via `imagesize`), Link,
      Emph/Strong/Strikeout/Superscript/Subscript/SmallCaps/Underline, Code,
      Note (footnotes part), HorizontalRule, LineBreak, RawBlock/RawInline
      `openxml`, Div/Span `custom-style`, Math via `quarto-math`.
- [ ] `q2 render --to docx` wiring in `quarto-core` (`format.rs`,
      `render_to_file.rs`), reference-doc option, end-to-end verification
      recorded in this plan (invocation + inspected output).
- [ ] Deferred: DefinitionList, LineBlock, TOC field, tracked changes
      (`Insert`/`Delete`), bookmarks for crossref targets.

### Phase 4 — sidecars and comment round trip (new strand under the epic)

- [ ] Embed `.qmd` source and source map as parts (`quarto/` directory,
      `Default` content types, relationships from `document.xml.rels` with a
      `https://quarto.org/…` reltype). Test: Word round-trip fixture (manual,
      recorded), LibreOffice round-trip (headless, automated).
- [ ] Emit a paragraph/run → source-span index so a Word comment anchored on
      a run maps to a `.qmd` range (`EditComment` inline on the way in).
- [ ] `q2 docx comments <file.docx>` (or equivalent) that lists comments with
      `.qmd` locations.

### Phase 5 — Typst writer consumer

- [ ] Out of scope for this epic; the Typst format writer (whichever leg
      lands) consumes `quarto_math::convert(_, Target::Typst)`.

## Decisions (2026-09-21)

1. **Style vocabulary: keep pandoc's style names.** Decided. Q1 users'
   reference docs are keyed on pandoc's names (`Source Code`, `Body Text`,
   `First Paragraph`, `Compact`, `Block Text`, `Caption`, `Image Caption`,
   `Table Caption`, `Verbatim Char`, `Hyperlink`, …); matching them keeps
   `reference-doc` and `custom-style` customizations portable from Q1. Names
   are not code: our own `styles.xml` defines them (the definitions are ours,
   the vocabulary is the public Q1 contract). python-docx's template supplies
   the Word-native scaffolding (Normal, Heading 1–9, Table Grid, numbering,
   theme, fonts) that those styles inherit from. The writer resolves a style
   *name* to a `w:styleId` by looking it up in the reference doc's
   `styles.xml`, adding a definition when the name is missing, so the body's
   only reference-doc-dependent bytes are style ids.
2. **mitex dependency mode: vendor `mitex-lexer` + `mitex-parser`.**
   Recommended pending confirmation. Upstream velocity for those two crates is
   near zero (two commits since 2025-01, one of them CI-only; see the
   2026-09-21 velocity check in the session transcript), so a quarterly
   `git log <upstream> -- crates/mitex-lexer crates/mitex-parser` check is
   enough to track bugfixes. Vendoring lets us add the span side table at the
   parser's `builder.token` sites and drop `mitex-spec-gen`'s Typst/submodule
   build step entirely. Record the upstream commit in `VENDORED.md`.
3. **Use-site attribution inside macro expansions: definition-site for v1.**
   Decided. Revisit if users report confusing math diagnostics.
4. **MathML writer: follow-up strand bd-9z83tcv0.** Decided. Not in this
   epic; the word-splitting pass in Phase 1 is designed for it.
5. **pampa math-text mapping lands on `main` first.** Decided 2026-09-21. It
   is a prerequisite that touches the same `treesitter.rs` lines as the open
   bd-q6ed / bd-qpa2 fixes; a small separate strand + PR keeps this branch
   additive and gives the pandoc-hybrid leg the same mapping.
6. **Vendor all four mitex front-end crates as crates.** Decided 2026-09-21
   (supersedes the "lexer + parser" wording of decision 2). Measured: lexer
   1,987 / parser 1,482 / spec 705 / glob 2,170 source lines (~6,300 total).
   `mitex-glob` is itself a vendored `glob-match` (MIT) used at one call site
   in the parser's argument matcher; keeping it beats reimplementing it.
   New third-party deps: `rowan`, `logos`, `ena`, `ecow`; `rkyv` is dropped.
   Local patch set stays minimal (span side table, rkyv removal, clippy
   allows); everything else lives in quarto-math on top of the vendored
   tree.
7. **Error subsystem 20, `math`.** Decided 2026-09-21. Pages under
   `docs/errors/math/` plus a sidebar section, same commit as each code.
8. **quarto-math API boundary: `(&str, MathType-equivalent, SourceInfo)`.**
   Decided 2026-09-21. quarto-math does not depend on quarto-pandoc-types;
   pampa adapts `Inline::Math` at the call site. Keeps the crate small for
   WASM.
9. **Schemas come from python-docx's `ref/`, not an ECMA download.** Decided
   2026-09-21. `external-sources/python-docx/ref/` carries the transitional
   XSDs (the `schemas.openxmlformats.org` namespaces Word writes), RelaxNG
   compact forms, and the four ISO/IEC 29500 PDFs; Part 1 §22.1 (OMML) is
   pages 3591–3707 and `pdftotext` extracts it in seconds. Phase 2 note:
   Word-saved parts (including python-docx's `default.docx`) fail plain XSD
   validation on `mc:Ignorable`; validating a Word round-trip fixture needs
   a markup-compatibility strip first. Our own writer output does not emit
   MCE, so this does not affect writer tests.
10. **No Word available for ground truth.** Neither of us has MS Word at the
    moment. The OMML writer is written from ISO 29500 Part 1 §22.1 plus the
    schema; Word-generated samples remain wanted whenever someone with Word
    can produce them (fixture request recorded in Phase 1).

### Reference docs and the pullback (why the blast radius is small)

A reference doc contributes the *non-body* parts of the package: `styles.xml`
(style definitions), `numbering.xml` (list definitions we may inherit),
`theme`, `fontTable`, `settings`, headers/footers, and the final `w:sectPr`
(page size, margins) copied into `document.xml`. The body of `document.xml`
is generated from the AST alone and refers to styles by id; changing the
reference doc changes how those ids *render*, not the paragraph/run structure.
So `v1.docx` vs `v2.docx` (same `.qmd`, different reference doc) differ in
`styles.xml`, `sectPr`, and possibly style-id attribute values, and nowhere
else in the body. The pullback (Word comments / tracked changes back to
`.qmd`) is therefore insensitive to the reference doc. What it *is* sensitive
to is Word's re-save, which splits and merges runs and adds `w:rsid*`
attributes, so the pullback must anchor on paragraph identity plus in-paragraph
text diffing, never on run structure. Candidate anchor: `w14:paraId`, which
Word assigns per paragraph and preserves across edits for co-authoring
(unverified; Phase 4 fixture).

## References

- python-docx: `external-sources/python-docx` (MIT, v1.2.0 `e454546`); schemas + ISO PDFs under `ref/`
- mitex: `external-sources/mitex` (Apache-2.0, `985d8e7`; run
  `git submodule update --init` for the prebuilt spec)
- redoc: `external-sources/redoc` (MIT); mechanism in `R/officer-embed.R`
- quarto-yaml source-map technique: `external-sources/quarto-yaml/crates/quarto-yaml/src/parser.rs` (`make_source_info`)
- Probes: `claude-notes/research/2026-09-21-quarto-math-probes/`
- Validators: OOXML-Validator CLI (mikeebowen), `@xarsh/ooxml-validator`,
  `devoidfury/docx-validator`, pandoc issues #9265/#9266/#9269/#11378,
  quarto-cli #7978
- ECMA-376: https://ecma-international.org/publications-and-standards/standards/ecma-376/
