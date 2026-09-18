# P7 — Implementation tasks & Test Seam Spec

**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P7-format-tail.md`](2026-08-20-pandoc-hybrid-P7-format-tail.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§11 golden-capture strategy, §12 known limitations, §13 project-mode gate, §14 multi-format guardrail)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Research companion:** [`../research/2026-07-13-q1-format-typescript.md`](../research/2026-07-13-q1-format-typescript.md)
**Sibling companions:** [`P1`](2026-09-18-pandoc-hybrid-P1-implementation.md), [`P2`](2026-09-18-pandoc-hybrid-P2-implementation.md), [`P3`](2026-09-18-pandoc-hybrid-P3-implementation.md), [`P4`](2026-09-18-pandoc-hybrid-P4-implementation.md), [`P5`](2026-09-18-pandoc-hybrid-P5-implementation.md), [`P6`](2026-09-18-pandoc-hybrid-P6-implementation.md), [`P7-foundation`](2026-09-20-pandoc-hybrid-P7-foundation-implementation.md) — prerequisites below cite their task numbers
**Depends on:** **P7-foundation**, P1, P2, P4, P5 — **and P6**, for correct numbers in its golden (per the epic's graph). P7 is last.
**Status:** Ready for subagent-driven execution.

This document's tasks are numbered **4, 5, 6, 8, 9, 10, 11, 12**. Tasks 1, 2, 3, and 7 live in
[`2026-09-20-pandoc-hybrid-P7-foundation-implementation.md`](2026-09-20-pandoc-hybrid-P7-foundation-implementation.md)
(Task 7 renumbered Task 4 there) — that document owns the format-agnostic CLI plumbing (the
`render.rs` gate relaxation, the multi-format warning, the project-mode containment gate, and B3
shared-services wiring) that any Pandoc-tail format needs, not only docx/pptx. Cross-references to
those tasks below say "P7-foundation Task N."

This file converts P7's Coarse checklist into `## Task N` units `superpowers:subagent-driven-development`
can dispatch, and binds every test P7 needs to a named production seam and revert hunk before any
code is written (the `/prevalidating-test-seams` discipline). The Spec is P7 + the design doc;
where this file and the plan disagree, the plan wins.

**Reading the vendored Q1 source.** The local `quarto-cli` checkout tracks `main` and will diverge
from the pinned tag `v1.11.3`. Always read pinned content via `git show v1.11.3:<path>`, never from
the worktree directly — every quarto-cli fixture path below was verified present at `v1.11.3` this
way. Behaviour claims marked **(measured)** were reproduced by running pandoc 3.8.1 locally.

---

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **`U`** | Rust unit test | `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **`I`** | Rust integration test, in-process, no external binary | `crates/<crate>/tests/integration/<name>.rs`, registered `pub mod <name>;` in that crate's `tests/integration/main.rs` | `cargo nextest run -p <crate>` |
| **`E`** | **End-to-end CLI test** — spawns the real `q2` binary and inspects the produced file | `crates/quarto/tests/integration/<name>.rs`, using `const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");` (the established idiom — `render_cli_e2e.rs:28`, `attribution_cli_e2e.rs:43`, and 5 more) | `cargo nextest run -p quarto` |
| **`L`** | Lua/pandoc integration test — a **real** `pandoc` subprocess below the CLI level, against the materialized Q1 tree | same layout as `I`; uses P4 Task 2's `run_main_lua` | `cargo nextest run -p quarto-core` |
| **`G`** | Dev-only golden **capture** — needs a real pinned-release Q1 `quarto` binary; **not in CI** (CLAUDE.md External Sources Policy) | `crates/xtask/src/capture_pandoc_goldens.rs` | `cargo xtask capture-pandoc-goldens` (local/dev only) |
| **`X`** | `cargo xtask` lint / verify gate | `crates/xtask/src/lint/<rule>.rs`, or a repo-level `check(workspace_root)` | `cargo xtask lint` / `cargo xtask verify` |

**Per `.claude/rules/integration-tests.md`, never add a top-level `crates/<crate>/tests/<name>.rs`.**
One `integration` binary per crate. `crates/quarto-core/tests/integration/main.rs` has 96
`pub mod` entries today; `crates/quarto/tests/integration/` has 31 files. New files are appended
alphabetically.

**Row tally across this document's 8 tasks (4, 5, 6, 8-12):** `U=31`, `I=4`, `E=5`, `L=4`, `G=1`,
`X=2` — 47 bound rows, plus **3 deferred seams** (T8.3, T12.3, T12.4). (P7-foundation's companion
carries 26 bound rows across its 4 tasks — `U=6`, `I=4`, `E=10`, `L=4`, `X=2` — for a combined 73
bound rows across both documents.) The `I`/`L` split is by *environment*, not by file location: a
row that reaches `PandocWriteStage` shells out to a real `pandoc` and is therefore `L`, even though
it lives in an `integration` binary and is invoked in-process.

### `E`- and `L`-tier gate policy (explicit, and its skipping is visible)

P7 adopts P4's gate policy verbatim rather than inventing a second one: **a hard gate, never a
skip.**

1. A real `pandoc` is already a hard dependency of `cargo nextest run --workspace` today — pampa's
   four oracle tests call `assert_good_pandoc_version()`
   (`crates/pampa/tests/integration/test.rs:161`), which `.expect()`-panics when pandoc is absent
   (`test.rs:165-168`). P7's `E` and `L` tiers therefore introduce **no new environment
   requirement** and must panic, not skip, when pandoc is missing or below P4 Task 7's floor.
2. **No `QUARTO_TEST_PANDOC=1`-style opt-in.** An opt-in gate is a skip by default.
3. The `G` tier is the only genuinely-skipped surface, and it is skipped by *not being a test* —
   it is an xtask a human runs, whose output is committed. Its "skip" is visible as a stale `.snap`
   in `git status`, and the `I`-tier assertions in Task 11 are what fail when it drifts.
4. **`.snap` accounting.** Per CLAUDE.md's "Snapshot Test Changes" rule, every task below that
   adds or updates `.snap` files must, in its commit message and in the running summary, report the
   **count** added/modified/removed and **summarize what changed**. Task 11's acceptance criterion
   restates this because the snapshot set is P7's largest committed artifact.

---

## End-to-end coverage map (which user-visible behaviors are driven through the real binary)

CLAUDE.md's **"End-to-end verification before declaring success"** rule records two incidents where
every test passed and the feature did not work, because the tests called library functions directly
and the CLI took a different branch. **P7 is exactly that shape: a CLI-visible feature behind a
config branch.** So each user-visible behavior below names its `E`-tier row, or says plainly that
it has none and why.

| User-visible behavior | Driven through the real `q2` binary? | Where |
|---|---|---|
| `q2 render f.qmd --to docx` produces a real `.docx` | **Yes** | P7-foundation T3.1 (`E`) |
| `q2 render f.qmd --to pptx` produces a real `.pptx` | **Yes** — separately from docx | P7-foundation T3.2 (`E`) |
| Multi-format `format:` block warns, naming used + skipped | **Yes** | P7-foundation T3.4 (`E`); message construction at P7-foundation T1.1-T1.3 (`U`) |
| Website project + `--to docx` writes no sitemap / no alias redirects | **Yes** | P7-foundation T2.4 (`E`) |
| Website project + `--to html` **still** writes them | **Yes** (the "path was actually exercised" half) | P7-foundation T2.5 (`E`) |
| `--reference-doc` reaches pandoc and changes the output | **Yes** | T4.6 (`E`) |
| `--reference-doc` pointing at a missing file diagnoses with a span | **Yes** | T4.7 (`E`) |
| Document metadata reaches `docProps/core.xml` | **Yes** | T6.3 (`E`) |
| Staged resources / rewritten links survive into the docx | **Yes** | P7-foundation T4.3 (`E`) |
| `--to latex` still refuses cleanly (stub, no variant added) | **Yes** | T4.9 (`E`) |
| pptx hides echoed source + warnings on slides | **No** — `I` only (T5.2/T5.3); see the `accepted-untested` entry in the Missing-test pass (needs a Python/R toolchain) | — |
| docx/pptx semantic content matches Q1 | **No** — `I` against committed `G`-captured snapshots (T11.*); the `G` half needs a real Q1 binary and is out of CI by policy | — |

Everything not in the "Yes" column is stated in the Missing-test pass with a bound seam or an
explicit `accepted-untested: <rationale>`.

---

## What the semantic extractor must preserve vs. may normalize

This section is normative for Tasks 9-11 and is the single largest vacuity hazard in P7. The point
of extracting **semantic text rather than raw XML** is to suppress Pandoc-version byte-noise. But
**an extractor that normalizes too aggressively collapses the discriminator**: the epic's central
claim is that *numbers* are identical across HTML and Pandoc, and P6's number-parity golden reaches
its assertion *through this extractor*. If the extractor strips numbers, prefixes, or the space
between them, that golden survives its own revert and the epic's headline claim becomes
unassertable.

### MUST preserve, byte-exactly

1. **Crossref numbers** — the digits and any compound form (`1`, `1.2`, `2.3.1`).
2. **Crossref prefixes and kinds** — `Figure`, `Table`, `Theorem`, `Note`, `alg.`, `Alg.`, and any
   `crossref.*-prefix` override that reached Q1's Lua. Do **not** case-fold: Q1's own fixture
   asserts `Alg.~` and `alg.~` as *distinct* expected strings
   (`v1.11.3:tests/docs/smoke-all/crossrefs/theorem/algorithm.qmd:19-22`).
3. **The non-breaking space (U+00A0) between prefix and number.** This is the specific item P5's
   Route-N work turns on, and Q1's own fixture carries an inline comment saying so —
   `algorithm.qmd:30`: `"\\[alg. 1\\]..."  # note the non-breaking space in "alg. 1"`. Q2's own
   producer is `head_text.push('\u{a0}')` at `crossref_render.rs:900`, documented at
   `crossref_render.rs:21` and `:761`. **(measured)** pandoc 3.8.1 emits it verbatim inside both
   `<w:t>` (docx) and `<a:t>` (pptx): `Figure\xc2\xa01`, hexdumped to confirm.
   **Consequence: the extractor must NOT apply any `\s+ → " "` collapse, `trim()` on interior
   runs, or `char::is_whitespace`-based tokenization** — every one of those silently rewrites
   U+00A0 to U+0020 and the nbsp assertion goes vacuous while still reading as a text comparison.
4. **The caption delimiter** (`: ` after the number, Q1's `title-delim`) and the theorem trailing
   `.` — these are the surface §12's crossref-presentation asymmetry is *about*.
5. **Paragraph order** — the extraction is a sequence, not a set.
6. **`<w:pStyle w:val="…"/>` / pptx placeholder role**, per paragraph. **(measured)**: a
   title/author/date/body document yields exactly `Title`, `Author`, `Date`, `FirstParagraph`.
7. **`<m:oMath>` flattened text content + element count.** **(measured)**
   `$$x=1$$` produces one `m:oMath` in docx and one in pptx. Without this, the extraction is
   byte-identical whether an equation number is present, absent, or wrong.
8. **Image/media inventory** — the `word/media/` (resp. `ppt/media/`) file-name list, and the
   `word/_rels/document.xml.rels` relationships **whose `Type` ends in `/image`**, by `Target`.
   **A raw entry count is version-noise, not signal.** **(measured)** an image-free document
   already carries 7+ relationships (`numbering`, `styles`, `settings`, `theme`, `fontTable`,
   `webSettings`, `footnotes`). Filter to image relationships, or the count drifts with any
   pandoc change to the default reference doc.
9. **`<w:drawing>` count** and **`<w:br w:type="page"/>` count**.
10. **Non-empty `docProps/core.xml` fields** `dc:title`, `dc:creator`, `dc:subject`, `cp:keywords`.

### MAY (and MUST) normalize away

1. **`docProps/core.xml`'s `dcterms:created` / `dcterms:modified`.** **(measured)** these are
   wall-clock timestamps of the render — including them makes every snapshot fail on every run.
2. **Attribute order** within an element, and XML namespace-prefix declarations.
3. **Insignificant inter-element whitespace / indentation** in the XML source — i.e. whitespace
   *between* tags, never whitespace *inside* a `<w:t>`/`<a:t>`/`<m:t>` text node. The distinction
   is the whole ballgame; `quick-xml`'s `Event::Text` inside a run is text, the `Event::Text`
   between `</w:p>` and `<w:p>` is not.
4. **Run splitting.** Pandoc may split one logical string across several `<w:r>`/`<w:t>` runs for
   styling. Concatenate runs within a paragraph *without inserting a separator*, then compare the
   paragraph string. (Concatenating with `" "` would break requirement 3 above.)
5. **`w:rsid*`, `w14:paraId`, `w:id` on comments/footnotes**, and any generated numeric id.
6. **The zip entry order and per-entry timestamps.**

### The one thing the extractor may not decide for itself

An **accepted divergence** (Q2 legitimately differs from Q1 — Route N's Q1-normative ref text, the
dropped `filename` header, missing section numbers, the one mermaid case) is recorded as a
*labeled* snapshot, never as an extractor-level normalization. If the extractor ever grows a
"strip the thing we know differs" branch, every future change in that area becomes invisible. Task
11 owns the labeling mechanism.

---

## Which of P7's tasks creates the golden harness

The harness is **Tasks 9, 10, 11**, and the borrowable unit is **Task 9**:

- **Task 9 — `crates/quarto-ooxml-extract/`**, a new leaf crate holding the extraction function.
  This is the piece both the `G`-tier xtask and the `I`-tier assertion test call, and the piece a
  predecessor plan would want. It depends only on `zip` + `quick-xml` — deliberately **not** on
  `quarto-core`, because `xtask` has no `quarto-core` dependency today (verified:
  `crates/xtask/Cargo.toml` deps are `anyhow, clap, nodejs-semver, proc-macro2, serde, serde_json,
  serde_yaml, syn, tempfile, time, walkdir`) and adding one would put the whole engine closure in
  front of every `cargo xtask lint`.
- **Task 10 — the `G`-tier capture** (`cargo xtask capture-pandoc-goldens`) + the fixture copy-in.
- **Task 11 — the `I`-tier assertion** against the same committed snapshots, plus per-fixture
  accepted-divergence provenance.

**P7 runs last, so P5 and P6 cannot borrow Task 9 — each has resolved that on its own side, so P7
inherits nothing from them:**

- P5's Layer-2 goldens item (`P5-lua-shim.md:655-659`) → resolved as **P5 companion Task 8**,
  *"Layer-2 per-type goldens — a narrow post-filter-AST harness, committed as `insta` snapshots"*.
  It asserts on the **post-filter Pandoc AST**, not on rendered OOXML, so it needs no extractor.
- P6's number-parity item (`P6-numbering-wiring.md:272-278`) → resolved as **P6 companion Task 5**,
  *"Figure / theorem / callout number-parity goldens — **schedulable without P7**"*.

**Consequence for P7:** Task 11 is not carrying deferred predecessor items, and Task 10's fixture
set has no inherited obligation to mirror theirs. What P7 adds on top is the one thing neither
sibling harness can reach — **the rendered docx/pptx surface**, where a number can be correct in the
post-filter AST and still be lost by the writer (the `<m:oMath>` case is exactly that shape). T11.3
is therefore a genuine second, independent check of the epic's number-identity claim, not a
duplicate of P6 Task 5. (P6 Task 5 and P7 T11.3 assert number identity at different depths — P6 at
the post-filter AST, P7 at the rendered OOXML — and are not duplicates for that reason: the
`<m:oMath>` case is correct-in-AST-but-lost-in-writer, which only P7 can see.)

---

## Tasks 1-3 and 7 — see P7-foundation

**The multi-format render warning, the project-mode containment gate, the format-gate relaxation +
`render_qmd_to_pandoc` routing, and B3 shared services** (formerly this document's Tasks 1, 2, 3,
and 7) live in
[`2026-09-20-pandoc-hybrid-P7-foundation-implementation.md`](2026-09-20-pandoc-hybrid-P7-foundation-implementation.md)
as its Tasks 1, 2, 3, and 4 respectively — see that file for scope, acceptance criteria, test-seam
specs, revert hunks, and vacuity checks.

---

## Task 4: The per-format invocation builder — docx + pptx, the pandoc-defaults allow-list, `FORMAT_PATH_KEYS`, the callout-icon PNGs, and the latex stub

This is this document's Task 4, distinct from
`2026-09-20-pandoc-hybrid-P7-foundation-implementation.md`'s own Task 4 (B3 shared services) —
always disambiguate with the plan name when citing either from a third document.

**Scope.** One task for the whole same-shape forwarding family, per the plan's own grouped
checklist item: `--to`, the per-format `pandoc` defaults, the forwarding allow-list, the two new
path-shaped keys, the 5 docx callout-icon params + their PNGs, and latex documented as a stub.

**Files.**
- `crates/quarto-core/src/pandoc_invocation.rs` (new, or wherever P4 Task 9's `PandocWriteStage`
  assembles argv — extend, don't duplicate) — the per-format table:
  **docx/odt** `page-width: 6.5`, `default-image-extension: png`;
  **pptx** `output-divs: false` (overrides the HTML-family base), `default-image-extension: png`;
  **latex** stub only — `format: latex` emits `.tex` directly, the extension wins over the inner
  `pdf` recipe, so no latexmk/tectonic step is in scope.
- The **forwarding allow-list** (allow-list, *not* full `kPandocDefaultsKeys` pass-through):
  `reference-doc`, `template`, `highlight-style`, `toc`, `toc-depth`,
  `reference-location`, `shift-heading-level-by`, and `slide-level` for pptx. Each with its own
  test row.
- `crates/quarto-core/src/project/format_paths.rs:99-105` — `FORMAT_PATH_KEYS` currently holds
  exactly 5 entries (`css` `ExistenceDiagnose/Entries`, `theme` `ExistenceSilent/Theme`, and the
  three `include-*` `Always/Include`). Add `reference-doc` and `template`, resolving relative to
  the declaring file with a leading `/` meaning project root.
- `claude-notes/designs/path-resolution-model.md` — add both to the consumption-site inventory, per
  the repo rule in CLAUDE.md ("Path resolution is a bug *class*"). A deliberate scope-out needs a
  strand linked to `bd-oejuizi9`.
- `resources/formats/docx/{note,tip,warning,caution,important}.png` (new, in-tree) — vendored from
  `v1.11.3:src/resources/formats/docx/`, 5 files, 749-1257 bytes each. They are **outside P4's
  traced `src/resources/filters/` vendoring closure**, so P7 vendors them separately. Consumed via
  the 5 docx callout-icon filter params (`docxCalloutImage` returns `nil` when unset,
  `v1.11.3:src/resources/filters/modules/callouts.lua:84-96` — degradation is graceful but
  **silent**, which is why T4.8 asserts the params are set rather than only that the render
  succeeds).
- `crates/quarto-core/src/pandoc_formats/latex.rs` (new, doc-comment only) — the documented stub.
  **No `FormatIdentifier::Latex` variant is added**; `--to latex` continues to return
  `Err("Unknown format: latex")`.

**Acceptance criterion.**

```
$ cargo run --bin q2 -- render f.qmd --to pptx
$ unzip -l f.pptx | grep -c 'ppt/slides/slide'
2                                   # slide-level splitting happened

$ cargo run --bin q2 -- render ref.qmd --to docx     # front matter: reference-doc: custom.docx
$ unzip -p ref.docx word/styles.xml | grep -c 'MyCustomStyle'
1                                   # the reference doc's styles reached the output

$ cargo run --bin q2 -- render f.qmd --to docx       # front matter: reference-doc: missing.docx
Error: [Q-...] `reference-doc: missing.docx` does not exist
  --> f.qmd:3:16
```

and `--to latex` still prints `Unknown format: latex`.

**Prerequisite.** **P4 Task 9** (the argv assembly this extends) and **P4 Task 4** (the
`QUARTO_FILTER_PARAMS` builder the 5 icon params ride in — P4 explicitly defers the *PNG assets* to
this plan, and this task supplies them).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | U | the per-format defaults table | Look up `"docx"` → `page-width == 6.5`, `default-image-extension == "png"`; `"pptx"` → `output-divs == false`, `default-image-extension == "png"` | none | each value in the table |
| T4.2 | U | the forwarding allow-list | Given a `format:` scope declaring all 8 allow-listed keys **plus** three non-allow-listed ones (`citeproc`, `wrap`, `columns`) → assert exactly the 8 appear in the assembled defaults and the 3 do not | none | the allow-list constant |
| T4.3 | U | the argv assembler | For pptx, assert `slide-level` is forwarded; for docx, assert it is **not** | none | the pptx-only `slide-level` arm |
| T4.4 | U | `format_paths::FORMAT_PATH_KEYS` + its resolver | A `reference-doc: ../shared/ref.docx` in a subdirectory `_metadata.yml` resolves against the **declaring file's** directory; a `reference-doc: /ref.docx` resolves against the **project root** | filesystem via `tempfile` | the two new `FORMAT_PATH_KEYS` rows |
| T4.5 | U | same, for `template` | Same two cases for `template:` | as above | the `template` row |
| T4.6 | **E** | the real binary + real pandoc | `q2 render ref.qmd --to docx` with a `reference-doc:` whose `word/styles.xml` contains a uniquely-named style → assert that style name appears in the output's `word/styles.xml` | nothing mocked | the `--reference-doc` argument in the argv assembly |
| T4.7 | **E** | the real binary, missing-path diagnostic | `q2 render f.qmd --to docx` with `reference-doc: missing.docx` → non-zero exit; stderr names `missing.docx` **and** carries a source span pointing at the `reference-doc:` line | nothing mocked | `MarkPolicy::ExistenceDiagnose` on the new `reference-doc` row |
| T4.8 | U | the `QUARTO_FILTER_PARAMS` blob for docx | Assert all 5 callout-icon params are present and each points at an **existing** file under `resources/formats/docx/` | none | the icon-param block, and the vendored PNGs |
| T4.9 | **E** | the real binary | `q2 render f.qmd --to latex` → non-zero exit, stderr contains `Unknown format: latex` | nothing mocked | *(none — see below)* |
| T4.10 | X | `xtask::lint` | `cargo xtask lint` green — no `external-sources/` reference reaches a compile-time macro for the vendored PNGs (`external-sources-in-macro`) | none | any `include_bytes!("…external-sources…")` shortcut |

**Revert hunks, stated exactly:**
- T4.1 — Revert ⟨`page-width: 6.5` to the base default⟩ → ⟨`assert_eq!(docx.page_width, 6.5)`⟩ RED.
- T4.2 — Revert ⟨the allow-list constant to "forward every key in the `format:` scope"⟩ → ⟨`assert!(!defaults.contains_key("wrap"))` in `test_forwarding_is_allow_listed`⟩ RED. **Chosen because the failure mode is over-forwarding, which no "the 8 keys are present" assertion can catch.**
- T4.3 — Revert ⟨the pptx-only guard on `slide-level`⟩ → ⟨`assert!(!docx_argv.contains("--slide-level"))`⟩ RED.
- T4.4 — Revert ⟨the `reference-doc` row in `FORMAT_PATH_KEYS`⟩ → ⟨`assert_eq!(resolved, subdir.join("../shared/ref.docx").canonicalize()?)` reddens (the raw string is joined against the project root instead)⟩ RED.
- T4.5 — Revert ⟨the `template` row⟩ → ⟨the parallel assertion⟩ RED.
- T4.6 — Revert ⟨the `--reference-doc` argument⟩ → ⟨`assert!(styles_xml.contains("MyCustomStyle"))` in `test_e2e_reference_doc_forwarded`⟩ RED.
- T4.7 — Revert ⟨`MarkPolicy::ExistenceDiagnose` to `ExistenceSilent` on the `reference-doc` row⟩ → ⟨`assert!(stderr.contains("f.qmd:3"))` in `test_e2e_missing_reference_doc_diagnosed`⟩ RED. **(measured)** without the Q2-side diagnostic, pandoc 3.8.1 exits **99** with the bare line `File missing.docx not found in resource path` — no span, no code, no pointer to the YAML key. So the span clause is the discriminator, not the "it fails" clause.
- T4.8 — Revert ⟨delete one vendored PNG⟩ → ⟨`assert!(path.exists())` in `test_docx_callout_icons_present`⟩ RED. Note the failure this guards is *silent*: `docxCalloutImage` returns `nil`, the render succeeds, and the callouts simply have no icons.
- T4.9 — **no revert hunk exists, by construction.** See below.
- T4.10 — Revert ⟨point `include_bytes!` at `external-sources/quarto-cli/src/resources/formats/docx/note.png`⟩ → ⟨`cargo xtask lint` reddens on `external-sources-in-macro`⟩.

### Refactor-induced vacuity check

- **T4.9 (the latex stub) asserts almost nothing, and that is correct.** What it *does* assert:
  `--to latex` still refuses, with the pre-existing message, i.e. this task did **not**
  accidentally add a `Latex` variant while adding `Pptx`. What it deliberately does **not** assert:
  anything about `.tex` output, KOMA template context, `formatExtras`, or latexmk — none of which
  exists. There is no production hunk whose revert reddens T4.9 in the usual sense, because the
  behavior it pins is *the absence of an implementation*; the hunk it guards against is a **future
  addition** (`"latex" => Ok(FormatIdentifier::Latex)` in `format.rs:84-101`). Recorded here so
  nobody reads T4.9 as evidence the stub "works".
- **T4.2's discriminator is the *excluded* keys, not the included ones.** The allow-list exists
  specifically to avoid silently forwarding everything Q1's TS type declares. A test asserting only
  that the 8 named keys arrive passes identically under a full pass-through — it survives the exact
  refactor the decision exists to prevent. Hence the three non-allow-listed keys in the fixture.
- **T4.6's discriminator must be a style name unique to the reference doc.** Asserting "the render
  succeeded with `--reference-doc`" is non-discriminating: **(measured)** pandoc 3.8.1 succeeds
  with a valid reference doc whether or not its styles are used, and succeeds identically with
  none. A uniquely-named style in `word/styles.xml` is the only cheap surface that differs across
  the two states.
- **T4.4/T4.5's expected values must not be the project root.** If the fixture declares
  `reference-doc:` in the *project-root* `_quarto.yml`, then "resolve against the declaring file"
  and "resolve against the project root" produce the **same** path and the test survives its own
  revert. The fixture therefore declares it in a **subdirectory** `_metadata.yml`. This is the
  exact recurrence shape CLAUDE.md's path-resolution rule warns about.

---

## Task 5: Format-specific `execute` defaults — pptx's `echo: false` / `warning: false` and both formats' figure sizes

**Scope.** Apply the per-format `execute` defaults, at the one format-aware seam where the engine's
own defaults and the document's `execute:` scope meet.

**Files.**
- `crates/quarto-core/src/stage/stages/engine_execution.rs:463-481` — **this is the seam.**
  `ExecutionContext::new(temp_dir, ctx.project.dir.clone(), path.clone(),
  ctx.format.identifier.to_string())` at `:464-468` already receives the format, and
  `.with_execute_scope(ast.meta.get("execute").cloned())` at `:481` already passes the document's
  merged `execute:` scope. The format defaults go **under** the document scope here — i.e. the
  scope handed to `with_execute_scope` becomes `format_defaults ⊕ document_scope`, document
  winning. `with_execute_scope` itself is `crates/quarto-core/src/engine/context.rs:280-283`.
- `crates/quarto-core/src/engine/knitr/format.rs:274-296` — read-only context, and the reason the
  seam above is the right one: `ExecuteConfig::with_defaults()` sets `fig_width: Some(7.0)`
  (`:281`), `fig_height: Some(5.0)` (`:282`), `echo: Some(Value::Bool(true))` (`:287`),
  `warning: Some(true)` (`:288`) — the **base** defaults — and
  `overlay_document_scope` (`:323-350`, called from `crates/quarto-core/src/engine/knitr/mod.rs:298`)
  overlays whatever arrives in the execute scope on top. So an injection at the `engine_execution.rs`
  seam lands correctly between the two, **engine-agnostically** (jupyter and knitr both read the
  same `execute_scope`), rather than needing a per-engine edit.
- Values, from `v1.11.3:src/format/formats.ts:315-331` and `formats-shared.ts:170-186`:
  **docx/odt** `execute.fig-width: 5`, `execute.fig-height: 4`;
  **pptx** `execute.fig-width: 11`, `execute.fig-height: 5.5`, **`echo: false`, `warning: false`**;
  base for comparison `fig-width: 7`, `fig-height: 5`, `echo: true`.

**Acceptance criterion.** For a pptx render, the `execute` scope handed to `ExecutionContext`
resolves `echo == false` and `warning == false`; for docx, `fig-width == 5` and `fig-height == 4`;
for html, the base `7`/`5`/`echo: true` are unchanged; and in **all** formats an explicit document
`execute: {echo: true}` wins over the pptx default.

**Prerequisite.** **P1 Task 1** (`FormatIdentifier::Pptx`) — without it there is no pptx format to
key on.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | U | the format-defaults lookup | `"docx"` → `{fig-width: 5, fig-height: 4}`; `"pptx"` → `{fig-width: 11, fig-height: 5.5, echo: false, warning: false}`; `"html"` → empty | none | each value in the table |
| T5.2 | I | `EngineExecutionStage`'s scope assembly (real stage, real `RenderContext`) | Run the stage on a pptx render of a cell-bearing fixture → assert the `ExecutionContext.execute_scope` observed by the engine has `echo == false` and `warning == false` | the **engine** is a replay/markdown engine (no Python/R); the unit under test is the *scope assembly*, never the engine | the merge at `engine_execution.rs:481` |
| T5.3 | I | same | Same fixture with front matter `execute: {echo: true}` → assert the observed scope has `echo == true` | as above | the merge **order** (format under document) |
| T5.4 | I | same | An **html** render of the same fixture → assert the observed scope has no `echo` override and knitr's `ExecuteConfig::with_defaults()` still yields `fig_width == 7.0` | as above | the `is html → empty defaults` arm |
| T5.5 | U | `ExecuteConfig::overlay_document_scope` (existing) | Overlay `{fig-width: 5}` on `with_defaults()` → `fig_width == Some(5.0)`, and every other field unchanged from `with_defaults()` | none | `overlay_document_scope`'s `fig-width` arm at `format.rs:324-326` |

**Revert hunks, stated exactly:**
- T5.1 — Revert ⟨`echo: false` in the pptx row⟩ → ⟨`assert_eq!(pptx["echo"], false)`⟩ RED.
- T5.2 — Revert ⟨the format-defaults merge at `engine_execution.rs:481`, restoring the bare `ast.meta.get("execute").cloned()`⟩ → ⟨`assert_eq!(observed_echo, Some(false))` in `test_pptx_execute_defaults_reach_engine`⟩ RED.
- T5.3 — Revert ⟨the merge order, putting format defaults **over** the document scope⟩ → ⟨`assert_eq!(observed_echo, Some(true))` in `test_document_execute_wins_over_format_default`⟩ RED. **This is the row that fails if someone "fixes" the merge by making the format authoritative.**
- T5.4 — Revert ⟨apply the pptx defaults unconditionally rather than per-format⟩ → ⟨`assert!(observed_scope.get("echo").is_none())` in `test_html_execute_defaults_unchanged`⟩ RED.
- T5.5 — Revert ⟨`overlay_document_scope`'s `fig-width` arm⟩ → ⟨`assert_eq!(cfg.fig_width, Some(5.0))`⟩ RED.

### Refactor-induced vacuity check

- **T5.3 is the row whose expected value could collapse.** If the fixture's document scope declared
  `echo: false` (agreeing with the pptx default), the assertion would read identically whether the
  merge order is document-wins or format-wins. The fixture therefore declares `echo: **true**` —
  the value that *disagrees* with the format default — so the two states differ.
- **T5.2/T5.3/T5.4 assert the scope the engine *observes*, not the rendered slide.** That is a
  deliberate tier choice, not an oversight: observing the rendered effect requires a real
  Python/R toolchain. The rendered-outcome case is logged in the Missing-test pass as
  `accepted-untested`.
- **The unit under test is the scope assembly; the engine is the environment.** T5.2-T5.4 use a
  replay/markdown engine so the assertion is about the value handed across the seam. This is not
  "simulate the engine and assert success" — no row asserts a successful execution.
- **`default-image-extension: png` is deliberately NOT in this task.** It is a *pandoc* default,
  not an `execute` one; it lives in Task 4's table (T4.1) and is asserted there. Splitting it
  would leave two half-owners of one row.

---

## Task 6: The `Meta`-block mapping for docx/pptx (and the `ensureMetaInlines` coercion it needs)

**Scope.** Map Q2's normalized document metadata (title, date, authors) into the wire output's
Pandoc `Meta` in the shape Pandoc's docx/pptx writers read. Includes the `MetaBlocks`→`MetaInlines`
coercion half of the plan's nested-`<p>` triage item, because that is the part with a *docx*
consequence.

**Files.**
- `crates/quarto-core/src/…` — the per-format `Meta` mapping, sited wherever P4 Task 9's
  `PandocWriteStage` builds the `Pandoc` value it serializes. P2 Task 5 confirms the *carriage*;
  this task does the per-format mapping (design doc §8's Meta-block contract, §10's P2↔P7 seam).
- `crates/quarto-core/src/template.rs:1347` / `:1361` — `fn titleblock_field_to_html` at
  **`template.rs:1361`**, called from **`template.rs:1347`**, is the real site of the nested-`<p>`
  bug. **Note the split consequence:** the `<p>`-in-`<p>` symptom is HTML-only and does
  not affect docx output, but the *underlying* missing `ensureMetaInlines` coercion does: a
  `PandocBlocks`-valued `title`/`subtitle` reaching Pandoc `Meta` as `MetaBlocks` is not what
  Pandoc's docx writer reads for `title`. **(measured)** with a normal inline title, pandoc 3.8.1
  puts it in `docProps/core.xml` as `<dc:title>My Title</dc:title>` **and** in
  `word/document.xml` as a paragraph with `<w:pStyle w:val="Title"/>`.
- `crates/quarto-core/tests/integration/pandoc_meta_mapping.rs` — new.
- `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs` — extend (P7-foundation's Task 3 creates it).

**Acceptance criterion.**

```
$ cargo run --bin q2 -- render meta.qmd --to docx   # title: My Title / author: Alice / date: 2026-01-02
$ unzip -p meta.docx docProps/core.xml | grep -o '<dc:title>[^<]*</dc:title>'
<dc:title>My Title</dc:title>
$ unzip -p meta.docx word/document.xml | tr '>' '>\n' | grep -A2 'w:pStyle w:val="Title"'
$ cargo run --bin q2 -- render meta.qmd --to pptx
$ unzip -p meta.pptx docProps/core.xml | grep -o '<dc:title>[^<]*</dc:title>'
<dc:title>My Title</dc:title>
```

with the block-valued-title case producing `MetaInlines`, not `MetaBlocks`.

**Prerequisite.** **P2 Task 5** (the confirmed `Meta` carriage — "produces for P7"), **P4 Task 9**
(the serialization point), **P7-foundation's Task 3** (for the `E` row to run at all).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1 | U | the `Meta` mapper | Map a normalized profile with `title`/`author`/`date` → assert the `Meta` has keys `title`, `author`, `date`, each `MetaInlines` | none | each key's insertion |
| T6.2 | U | same, coercion path | Map a **block-valued** `title` (a `PandocBlocks` `ConfigValue`) → assert the `Meta` entry is `MetaInlines`, and its flattened text equals the source text | none | the `MetaBlocks`→`MetaInlines` coercion |
| T6.3 | **E** | the real binary + real pandoc | `q2 render meta.qmd --to docx` → `docProps/core.xml` contains `<dc:title>My Title</dc:title>` and `<dc:creator>Alice</dc:creator>`; `word/document.xml` has a paragraph with `<w:pStyle w:val="Title"/>` whose text is `My Title` | nothing mocked | the `title` insertion in the mapper |
| T6.4 | **E** | same, pptx | `q2 render meta.qmd --to pptx` → `docProps/core.xml` contains `<dc:title>My Title</dc:title>` | nothing mocked | the mapper's pptx arm (or its shared path) |
| T6.5 | I | the wire serializer + mapper together | Serialize a docx render's `Pandoc` value → assert the JSON's `meta.title.t == "MetaInlines"` | none | the coercion |

**Revert hunks, stated exactly:**
- T6.1 — Revert ⟨the `date` insertion⟩ → ⟨`assert!(meta.contains_key("date"))`⟩ RED.
- T6.2 — Revert ⟨the coercion, leaving `MetaBlocks`⟩ → ⟨`assert_eq!(entry.tag(), "MetaInlines")` in `test_block_valued_title_coerced`⟩ RED.
- T6.3 — Revert ⟨the `title` insertion in the mapper⟩ → ⟨`assert!(core_xml.contains("<dc:title>My Title</dc:title>"))` in `test_e2e_docx_meta_title`⟩ RED.
- T6.4 — Revert ⟨same⟩ → ⟨the pptx `dc:title` assertion⟩ RED.
- T6.5 — Revert ⟨the coercion⟩ → ⟨`assert_eq!(j["meta"]["title"]["t"], "MetaInlines")`⟩ RED.

### Refactor-induced vacuity check

- **`<dc:title>` is populated by pandoc from `Meta.title` — but so is the fallback.** **(measured)**
  pandoc with **no** `title` in `Meta` emits `<dc:title></dc:title>` (empty), not a missing element.
  So `core_xml.contains("<dc:title>")` is non-discriminating; the assertion must include the
  **value** (`<dc:title>My Title</dc:title>`), as written.
- **T6.2's flattened-text assertion must be paired with the tag assertion.** A coercion that
  produced `MetaInlines` from the *wrong* content (e.g. an empty inline list) satisfies the tag
  assertion alone.
- **T6.3 asserts both surfaces** (`docProps/core.xml` **and** the `Title`-styled body paragraph)
  because they come from different pandoc code paths: a `Meta.title` that reaches core props but
  not the body, or vice versa, is a real partial failure a single assertion would miss.
- **The `<p>`-in-`<p>` HTML symptom is explicitly out of this task's assertions.** It is an
  HTML-leg bug with no docx surface; fixing it is not required for P7's goldens to be diagnosable.
  Logged in the Missing-test pass.

---

## Task 8: Triage the two pre-existing Q2 bugs before any golden diff is trusted

**Scope.** Both bugs produce Q1/Q2 diffs unrelated to the hybrid work. The plan's requirement is
*fix or explicitly flag* — this task's deliverable is a **decision recorded in the tree**, plus a
bound test for whichever branch is taken.

**Files.**
- **Bug A — the nested-`<p>` / missing `ensureMetaInlines` coercion.** Real site
  `crates/quarto-core/src/template.rs:1361` (`titleblock_field_to_html`), caller `:1347`.
  **Decision: the docx-relevant half is fixed in Task 6** (T6.2/T6.5 bind the
  `MetaBlocks`→`MetaInlines` coercion). The HTML-leg `<p>`-in-`<p>` symptom has **no** docx or pptx
  surface and is explicitly flagged, not fixed, here — recorded as an `accepted-untested` line in
  the Missing-test pass with its own strand if one does not exist.
- **Bug B — the silent multi-id crossref drop.** The production site is
  `let first = cite.citations.first()?;` at `crates/quarto-core/src/transforms/crossref_resolve.rs:253`;
  the existing test `fn multi_crossref_cite_resolved_to_first` (whose own comment states "We
  currently resolve to the first; the second is dropped. (Phase 1 scope…)") is at
  `crossref_resolve.rs:540`. Note that the *mixed* case (crossrefs intermixed with bibliographic
  citations) already emits a diagnostic at `crossref_resolve.rs:246-254`; the **all-crossrefs**
  case `[@fig-a; @fig-b]` is the silent one.
- `crates/quarto-core/tests/integration/…` — the binding test for whichever branch is taken.

**Acceptance criterion.** A written record in the tree (plan checklist + a strand for anything not
fixed) stating, for each bug: fixed (with its bound test) or flagged (with the strand id), **and**
a note in Task 11's per-fixture provenance for any golden whose diff is attributable to it. Neither
bug may be discovered for the first time while reviewing a golden diff.

**Prerequisite.** None. Dispatch this **before** Tasks 10 and 11 — that is the whole point of the
plan's "before trusting any golden diff" phrasing.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T8.1 | U | `crossref_resolve::resolve` | `[@fig-a; @fig-b]`, both crossrefs, both indexed → assert **either** (fix branch) both ids resolve, **or** (flag branch) exactly one resolves **and** a diagnostic naming the dropped id is returned | none | fix branch: the multi-id loop replacing `citations.first()?` at `:253`. Flag branch: the new diagnostic push |
| T8.2 | U | same | The existing test `multi_crossref_cite_resolved_to_first` (`crossref_resolve.rs:540`) is **updated, not deleted**, to state the chosen behavior | none | whichever hunk T8.1 names |
| T8.3 | — | Bug A | `seam deferred until Task 6 of this plan` (T6.2/T6.5 are its binding tests, on the docx-relevant surface); the HTML `<p>`-in-`<p>` half is `accepted-untested` — see the Missing-test pass | — | — |

**Revert hunks, stated exactly:**
- T8.1 (flag branch, the likely one given the epic's "no new functionality" principle) — Revert ⟨the diagnostic push added next to `crossref_resolve.rs:253`⟩ → ⟨`assert_eq!(diags.len(), 1)` in `test_multi_crossref_drop_is_diagnosed`⟩ RED.
- T8.1 (fix branch) — Revert ⟨the multi-id loop, restoring `citations.first()?`⟩ → ⟨`assert_eq!(resolved_ids, ["fig-a", "fig-b"])`⟩ RED.
- T8.2 — Revert ⟨the updated expectation in the existing test⟩ → ⟨that test reddens⟩. Named to prevent the common shortcut of deleting the inconvenient existing test.

### Refactor-induced vacuity check

- **A "flag" branch whose only artifact is prose is vacuous.** If the decision is "don't fix,"
  the *bound* deliverable is the diagnostic (T8.1 flag branch) or, if even that is out of scope, a
  strand id plus a `#[test]`-asserted pin on the current behavior (T8.2) — never a sentence in a
  plan file alone. A future change to `citations.first()?` must move something.
- **Deleting `multi_crossref_cite_resolved_to_first` would silently erase the pin.** T8.2 exists to
  make that visible.
- **Bug A's binding lives in Task 6, and this row says so rather than inventing one here.** Two
  tasks each asserting half a coercion is how a seam ends up with no owner.

---

## Task 9: `crates/quarto-ooxml-extract/` — the shared semantic extractor (**the golden harness's core**)

**Scope.** A new leaf crate holding the single extraction function both the `G`-tier capture xtask
and the `I`-tier assertion test call. Implements the preserve/normalize contract in
**"What the semantic extractor must preserve vs. may normalize"** above, which is normative for
this task.

**Files.**
- `crates/quarto-ooxml-extract/Cargo.toml` (new) — dependencies **exactly** `zip` (new workspace
  dependency; add to `[workspace.dependencies]` in the root `Cargo.toml` next to
  `quick-xml = "0.39"` at `Cargo.toml:37`, which already exists) and `quick-xml` (workspace).
  **No `quarto-core` dependency, in either direction.** `crates/xtask/Cargo.toml` has no
  `quarto-core` dep today and must not gain one; the extractor is the leaf that both sides can
  share.
- `crates/quarto-ooxml-extract/src/lib.rs` — `pub fn extract_docx(bytes: &[u8]) -> Result<Extraction>`
  and `pub fn extract_pptx(bytes: &[u8]) -> Result<Extraction>`, plus `impl Display for Extraction`
  producing the stable text form that goes into a `.snap`.
- Consumers: `crates/xtask/Cargo.toml` (dependency, Task 10); `crates/quarto-core/Cargo.toml` and
  `crates/quarto/Cargo.toml` (`[dev-dependencies]`, Task 11).
- Entry paths, **all measured** with pandoc 3.8.1: docx → `word/document.xml`,
  `word/_rels/document.xml.rels`, `word/media/*`, `docProps/core.xml`; pptx → `ppt/slides/slideN.xml`
  (`slide1.xml`, `slide2.xml`, … in order), `ppt/slides/_rels/slideN.xml.rels`, `ppt/media/*`,
  `docProps/core.xml`. Text nodes: `<w:t>` (docx) / `<a:t>` (pptx) / `<m:t>` (both, inside
  `<m:oMath>`). Paragraph style: `<w:pPr><w:pStyle w:val="…"/></w:pPr>` — measured values from a
  title/author/date/body document: `Title`, `Author`, `Date`, `FirstParagraph`.

**Acceptance criterion.**
1. `extract_docx` on a docx produced from `Figure\u{a0}1 is here.` yields a paragraph string whose
   bytes are `Figure\xc2\xa01 is here.` — **verified by a byte-level assertion, not a `==` on a
   `&str` literal typed with a normal space.**
2. The extraction of two docx files produced from the same input by the same pandoc differs in
   **zero** bytes, and neither contains a timestamp (a `dcterms:created` substring assertion).
3. An input whose only difference is a **missing image** produces a *different* extraction
   (the media inventory differs), and an input whose only difference is an **absent equation
   number** produces a different extraction (the `m:oMath` flattened text differs).
4. `extract_pptx` reports one entry per slide, in slide order.
5. Not a zip / not an OOXML package → `Err`, never a panic.

**Prerequisite.** None. **This is the unit predecessor plans would have borrowed and could not —
see "Which of P7's tasks creates the golden harness" above.** Dispatch it as early as convenient;
nothing else in P7 blocks it.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T9.1 | U | `extract_docx`'s text-node handling | Extract a committed fixture docx built from `Figure\u{a0}1` → `assert_eq!(p.as_bytes(), b"Figure\xc2\xa01 is here.")` | none | any whitespace normalization applied to run text |
| T9.2 | U | same, run concatenation | A docx whose logical string is split across three `<w:r>`/`<w:t>` runs → assert the paragraph string has no inserted separator | none | the run-join (a `join(" ")` reddens this) |
| T9.3 | U | the core-props handling | Extract → assert the output contains `dc:title`'s value and does **not** contain `dcterms:created` or any `T..:..:..Z` timestamp | none | the core-props field allow-list |
| T9.4 | U | the media inventory | Two fixture docx files, identical but one with the image resolved and one without → assert the extractions differ, and that the one with the image lists exactly one `word/media/` entry and one `image`-typed relationship | none | the `Type.ends_with("/image")` filter on relationships |
| T9.5 | U | the `m:oMath` handling | Two fixture docx files: `Equation\u{a0}1` inside `<m:oMath>` vs. no number → assert the extractions differ | none | the `<m:oMath>` flattened-text collection |
| T9.6 | U | `<w:pStyle>` capture | Extract a title/author/date/body docx → assert the style sequence is exactly `["Title", "Author", "Date", "FirstParagraph"]` | none | the `w:pStyle` read |
| T9.7 | U | pagebreak + drawing counts | A docx with one pagebreak and one image → assert `br_page == 1`, `drawing == 1` | none | either counter |
| T9.8 | U | `extract_pptx` | Extract a 2-slide pptx → assert two slide entries in order, each with its `<a:t>` text; nbsp byte-preserved | none | the `ppt/slides/slideN.xml` enumeration (a `HashMap`/glob without a numeric sort reddens the order assertion) |
| T9.9 | U | error paths | A non-zip byte slice, and a zip with no `word/document.xml` → `Err` in both, no panic | none | the two error returns |
| T9.10 | U | determinism | Extract the same fixture twice → byte-identical | none | any iteration over a `HashMap` that reaches the output |

**Revert hunks, stated exactly:**
- T9.1 — Revert ⟨apply `text.split_whitespace().collect::<Vec<_>>().join(" ")` to run text — the single most likely "cleanup" a future contributor makes⟩ → ⟨`assert_eq!(p.as_bytes(), b"Figure\xc2\xa01 is here.")` in `test_nbsp_preserved_bytewise`⟩ RED.
- T9.2 — Revert ⟨join runs with `" "`⟩ → ⟨`assert_eq!(p, "onetwothree")`⟩ RED.
- T9.3 — Revert ⟨the core-props allow-list, emitting the whole element⟩ → ⟨`assert!(!s.contains("dcterms:created"))` in `test_no_timestamps_in_extraction`⟩ RED.
- T9.4 — Revert ⟨the `/image` relationship filter, counting all relationships⟩ → ⟨`assert_eq!(rels.len(), 1)` in `test_media_inventory_is_image_only`⟩ RED. **(measured)** an image-free docx already carries 7+ relationships, so an unfiltered count is version-noise.
- T9.5 — Revert ⟨drop `<m:oMath>` collection⟩ → ⟨`assert_ne!(with_number, without_number)` in `test_oMath_number_is_visible`⟩ RED.
- T9.6 — Revert ⟨the `w:pStyle` read⟩ → ⟨`assert_eq!(styles, ["Title","Author","Date","FirstParagraph"])`⟩ RED.
- T9.7 — Revert ⟨the `<w:br w:type="page"/>` counter⟩ → ⟨`assert_eq!(e.br_page, 1)`⟩ RED.
- T9.8 — Revert ⟨numeric slide ordering, e.g. iterate zip entries as encountered⟩ → ⟨the slide-order assertion (fixture built with 10+ slides so lexicographic ≠ numeric: `slide10` sorts before `slide2`)⟩ RED.
- T9.9 — Revert ⟨`?` on the zip open into `.unwrap()`⟩ → ⟨the `is_err()` assertions⟩ RED (the test panics rather than failing an assertion; a panicking test is still RED, and the row exists so the API contract is "Err, never panic").
- T9.10 — Revert ⟨iterate a `HashMap` into the output instead of a sorted/ordered structure⟩ → ⟨`assert_eq!(a, b)` across repeated extractions⟩ RED (flaky-red, which is the signal).

### Refactor-induced vacuity check

**This is the task where over-normalization silently voids the epic's central claim.** Named
checks, each with the specific expected value examined:

- **T9.1's expected value is a byte string, deliberately.** Written as
  `assert_eq!(p, "Figure 1 is here.")` with a keyboard space, the assertion **passes under the
  over-normalizing implementation** and the nbsp discriminator is gone — and nothing about the test
  source would look wrong. The byte-literal form is the only shape that distinguishes the two
  states. The downstream cost of getting this wrong is precise: P6's number-parity golden and P5's
  Route-N `alg.\u{a0}1` assertion both reach their expectation *through this function*, so both go
  vacuous together.
- **T9.4's discriminator only exists after the `/image` filter.** A count over *all* relationships
  does not differ between a document with a resolved image and one without (both carry the 7+
  boilerplate relationships, and a missing image is replaced by alt text with **no** relationship).
  The collapsed raw count is kept **only** as a shape check, never as the image-presence
  discriminator.
- **T9.8's fixture needs ≥10 slides.** With 2 slides, lexicographic and numeric ordering agree and
  the ordering assertion survives its own revert.
- **T9.3 is an anti-assertion and needs its positive twin.** "Contains no timestamp" is satisfied
  by an extractor that emits nothing at all. It is paired with the `dc:title` value assertion in
  the same test.
- **T9.10 guards a failure that is invisible in a single run.** A `HashMap` reaching the output
  makes every snapshot intermittently wrong, which reads as "insta is flaky" rather than as a bug.

---

## Task 10: `cargo xtask capture-pandoc-goldens` — the fixture set, the copy-in, and the `insta::Settings` capture

**Scope.** The dev-only `G`-tier capture: locate a real pinned-release `quarto`, render the named
fixtures to docx and pptx, run Task 9's extractor, write `.snap` files under an explicit
`insta::Settings` path. Plus the one-time copy of quarto-cli-sourced fixtures into our own tree.

**Files.**
- `crates/xtask/src/capture_pandoc_goldens.rs` (new) + its `main.rs` subcommand.
  `crates/xtask/Cargo.toml` gains `quarto-ooxml-extract` and `insta` (workspace, `Cargo.toml:44`,
  `insta = "1.46.3"`).
- `crates/quarto-core/tests/fixtures/pandoc-goldens/` (new) — the **copied** fixtures. Per the
  **External Sources Policy**, fixtures are copied in once (mirroring `resources/scss/`'s
  "copy in, track locally" pattern) and never read from `~/src/quarto-cli` or `external-sources/`
  at test time. Copy the fixture's **resources too** — `all-docx.qmd` references
  `img/thinker.jpg`, which exists at `v1.11.3:tests/docs/crossrefs/img/thinker.jpg`; copying the
  `.qmd` alone silently turns a figure into an unresolved image.
- `crates/quarto-core/tests/fixtures/pandoc-goldens/README.md` (new) — the provenance ledger:
  source path, source tag, copy date, and one line on why each fixture is in the set.

**The fixture set — 9 fixtures, every path verified present at tag `v1.11.3`** (via `git cat-file -e
v1.11.3:<path>`; the real path prefix is `tests/`, not `docs/`):

| # | Source (at `v1.11.3`) | Exercises | Engine-free? |
|---|---|---|---|
| 1 | `tests/docs/crossrefs/all-docx.qmd` (+ `tests/docs/crossrefs/img/thinker.jpg`) | numbered figure + captioned table + a theorem; Q1's own docx-targeted crossref doc | yes |
| 2 | `tests/docs/callouts.qmd` | all 5 callout types, captioned/uncaptioned, `collapse`, three `appearance` values, `icon="false"` — the docx callout-icon path | yes |
| 3 | `tests/docs/crossrefs/callouts.qmd` | **cross-referenceable** callouts — the Route-R Callout `order` reclassification (design §3) | yes |
| 4 | `tests/docs/crossrefs/theorems.qmd` | `::: {#thm-line}` + an unlabeled `$$` | yes |
| 5 | `tests/docs/crossrefs/theorem-types.qmd` | one div per theorem type (`lem/cor/prp/cnj/def/exm/exr`) — and **not** `alg`, which is Task 12's point | yes |
| 6 | `tests/docs/crossrefs/equations.qmd` | one labeled `$$ {#eq-black-scholes}` + an `@eq-` ref — the `<m:oMath>` path | yes |
| 7 | `tests/docs/smoke-all/crossrefs/theorem/proof-rendering.qmd` | `.proof`, `.proof name=…`, empty `.proof`, `.remark` — P5's Proof Route-R shape | yes |
| 8 | `tests/docs/smoke-all/2025/01/08/7260.qmd` | the smallest clean `.panel-tabset` (Tab A / Tab B `{.active}`) — the Tabset Route-R reclassification | yes |
| 9 | `tests/docs/smoke-all/mermaid/backticks.qmd` | **the one labeled accepted-divergence fixture** — see Task 11 | no engine cells, but a `mermaid` cell |

Plus **one fixture we author ourselves**, because no quarto-cli fixture covers it: a **Tabset
containing a FloatRefTarget subfloat** (P6 Finding 5) →
`crates/quarto-core/tests/fixtures/pandoc-goldens/tabset-subfloat.qmd`.

**The engine-free rule:** capture renders with a real `quarto`, so any fixture with an
`{r}`/`{python}`/`{julia}` cell needs that toolchain — excluded for v1. All 9 sourced fixtures were
checked and carry zero such cells. (Fixture 9's `mermaid` cell is a mermaid-engine cell, not
R/Python/Julia; see Task 11 for what its snapshot is expected to show.)

**Exclusions kept as-is:** the code-block `filename`-header fixtures and the
`crossref:`-presentation-option fixtures stay **out**, because those genuinely are structural
blind spots and accepted gaps (design §12). **mermaid is not excluded** — its exclusion would be
circular, since the point of fixture 9 is to record the mermaid divergence.

**Acceptance criterion.**
1. `cargo xtask capture-pandoc-goldens` with no real `quarto` on `PATH` **fails loudly**, naming
   the binary and the expected release tag — it must not skip, and must not write an empty snapshot.
2. With a real pinned-release `quarto`, it writes one `.snap` per fixture/format pair (20 files:
   10 fixtures × docx + pptx) at the `insta::Settings`-declared path, and re-running it produces
   **zero** `git diff`.
3. The written filenames match exactly what Task 11's test looks up — verified by running Task 11's
   test immediately after and observing **no** "snapshot not found, created new" line.
4. `cargo xtask capture-pandoc-goldens` is **not** invoked from `cargo xtask verify` or any CI
   workflow. CLAUDE.md's "keep `verify` and CI in sync" rule cuts the other way here: nothing is
   added to CI, so nothing is added to `verify`.

**Prerequisite.** **Task 9** (the extractor). **Task 8** (triage) — dispatch after it, so a
first-capture diff is diagnosable. A real Q1 `quarto` at the pinned release for the host platform.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T10.1 | U | the xtask's binary-location precondition | Invoke with a `PATH` containing no `quarto` → `Err` naming the binary and the expected tag; non-zero exit | `PATH` via env, in-process | the precondition `bail!` |
| T10.2 | U | the xtask's version precondition | Invoke against a stub `quarto` reporting a **different** version → `Err` naming both versions | the `quarto` **binary path only**, and solely to bind the precondition — T10.5 does the real run | the version comparison |
| T10.3 | U | the snapshot-naming function | Assert the derived `(snapshot_path, snapshot_name)` pair for `(fixture, format)` equals a hardcoded literal for three fixtures | none | the naming function |
| T10.4 | U | the fixture manifest | Assert every manifest entry's `.qmd` **and its declared resources** exist under `tests/fixtures/pandoc-goldens/`, and that the manifest has exactly 10 entries | none | the manifest, and the copied files |
| T10.5 | **G** | the real pinned `quarto` + Task 9's extractor | Run the full capture → 20 `.snap` files written; a second run produces an empty `git diff` | **nothing mocked** — this is the point | any change in the capture path |
| T10.6 | X | `xtask::lint::external_sources_in_macro` | `cargo xtask lint` green | none | any `include_str!`/`include_bytes!` pointed at `external-sources/` or `~/src/quarto-cli` |
| T10.7 | U | the xtask subcommand registration | Assert `capture-pandoc-goldens` is **absent** from `verify.rs`'s step list and from `build_all.rs` | none | any addition of it to `verify` |

**Revert hunks, stated exactly:**
- T10.1 — Revert ⟨the missing-binary `bail!` into `return Ok(())`⟩ → ⟨`assert!(result.is_err())` in `test_capture_fails_without_quarto`⟩ RED. **This is the "a skip must itself be visible" row.**
- T10.2 — Revert ⟨the version comparison⟩ → ⟨`assert!(err.to_string().contains("v1.11.3"))`⟩ RED.
- T10.3 — Revert ⟨the `insta::Settings` explicit path/name to insta's `module_path!()` default⟩ → ⟨`assert_eq!(derived, ("…/snapshots", "pandoc_golden_all_docx_docx"))`⟩ RED. Bound because the failure mode is a **silent pass** ("snapshot not found, created new"), not a build error.
- T10.4 — Revert ⟨drop `img/thinker.jpg` from the copy-in⟩ → ⟨`assert!(resource.exists())` in `test_fixture_manifest_complete`⟩ RED. The failure this guards is an unresolved image that *still renders*, per P7-foundation T4.3's measured note.
- T10.5 — Revert ⟨any behavioral change in the capture path⟩ → ⟨a non-empty second-run `git diff`⟩. Stated as a *deliberate* revert because T10.5's real job is to catch an accidental one.
- T10.6 — Revert ⟨read a fixture from `~/src/quarto-cli` at capture time instead of the copied tree⟩ → ⟨the lint reddens⟩ (for the macro form) **and** T10.4 reddens (for the runtime-path form).
- T10.7 — Revert ⟨add the capture step to `verify.rs`⟩ → ⟨`assert!(!verify_steps().contains("capture-pandoc-goldens"))`⟩ RED.

### Refactor-induced vacuity check

- **T10.3 exists because the failure is a green test.** `cargo insta` writes a brand-new snapshot
  and passes when it cannot find the named one. So the capture-side and assert-side naming must be
  **one function**, tested against hardcoded literals — not two independent derivations that happen
  to agree today. The literals must be written out, not computed, or the test follows a
  renaming and stops discriminating.
- **T10.2's mock is the `quarto` binary path only, and asserts a *failure*.** It never asserts a
  successful capture through a stub — that would be exactly the discipline's named anti-pattern
  ("simulate the engine in a mock and assert success"). T10.5 is the real run.
- **T10.5's "empty second-run diff" is non-discriminating if the first run wrote nothing.** Pair it
  with the file-count assertion (20), as written.
- **T10.4's count literal (10) is frozen, not derived.** `manifest.len() == manifest.len()` is the
  degenerate form; a count computed from a directory walk follows any accidental deletion.

---

## Task 11: The CI-runnable Q2-side golden assertion + per-fixture accepted-divergence provenance

**Scope.** The half of the harness that gates. Render each fixture through Q2's own hybrid path,
apply the **identical** extraction, and assert it against the same committed snapshot the capture
produced from real Q1 — so one assertion serves both Q1-parity and Q2 regression (design §11,
third bullet). Plus the provenance mechanism that keeps an accepted divergence labeled.

**Files.**
- `crates/quarto-core/tests/integration/pandoc_goldens.rs` (new) — registered in
  `crates/quarto-core/tests/integration/main.rs` alphabetically. Uses `insta::Settings::clone_current()`
  + `set_snapshot_path(...)` — the established idiom (`crates/pampa/tests/integration/test.rs:513-516`,
  `test_error_corpus.rs:257-258` and `:334-335`) — with **Task 10's shared naming function**, not a
  second derivation.
- `crates/quarto-core/tests/integration/snapshots/` — the 20 committed `.snap` files.
- `crates/quarto-core/tests/fixtures/pandoc-goldens/DIVERGENCES.md` (new) — one entry per accepted
  divergence: fixture, format, what differs from Q1, which design-§12 bullet or strand it belongs
  to, and the date accepted.

**Acceptance criterion.**
1. `cargo nextest run -p quarto-core -- pandoc_goldens` green with **no** `.snap` writes and no
   "created new" lines. Requires a real `pandoc`, no `quarto`, no `external-sources/` — CI-runnable.
2. Every `.snap` whose content differs from what real Q1 produced has a matching entry in
   `DIVERGENCES.md`, asserted by T11.5 — so `cargo insta review` cannot quietly convert a
   "Q1-parity" snapshot into an unlabeled "Q2 baseline".
3. **Snapshot accounting, per CLAUDE.md:** the commit message reports the count of `.snap` files
   added (20) and summarizes what each group asserts, and calls out every divergence entry
   explicitly for review before any push.
4. The mermaid fixture's snapshot shows the **diagram source text** as body content, and its
   `DIVERGENCES.md` entry names `bd-h1ub8f8z` and design §12.

**Prerequisite.** **Tasks 9 and 10** of this plan; **P4 Task 2**'s `run_main_lua` / **P4 Task 9**'s
`render_qmd_to_pandoc` for the Q2 render; **P5 companion Tasks 1-6** (shim + Route R/N + its error
handling) and **P6 companion Tasks 1, 2, 4** (external numbering for `Pandoc(fmt)`, the Callout
reclassification, the suppression matrix) for the content to match — this is the "P6 too, for
correct numbers in its golden" edge in the epic's graph. **Dispatch last.**

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T11.1 | L | `render_qmd_to_pandoc` + the real shim + Task 9's extractor | For each of the 10 fixtures × docx → `insta::assert_snapshot!` against the committed Q1-captured snapshot | nothing mocked (real pandoc, real vendored Lua) | P5's Route-R shim, P6's numbering wiring, Task 4's invocation builder — any of them |
| T11.2 | L | same, pptx | Same for × pptx | as above | as above |
| T11.3 | L | **number parity**, explicitly | For fixtures 1, 3, 4 render **both** `--to docx` and `--to html`; extract the docx numbers via Task 9 and the HTML numbers via the existing HTML-leg helper → assert the **number strings are equal** (`Figure\u{a0}1`, `Note\u{a0}1`, `Theorem\u{a0}1`), asserted byte-wise | nothing mocked | `crossref-numbering: external` (P3) + the Route-R `order` injection (P5) |
| T11.4 | U | the shared naming function | Assert the test's lookup name equals Task 10's write name for all 20 pairs | none | either side of the naming function |
| T11.5 | U | the `DIVERGENCES.md` ledger | Parse the ledger → assert every snapshot marked divergent in its own header comment has a ledger entry, and every ledger entry names an existing snapshot | none | the ledger, and the per-snapshot divergence marker |
| T11.6 | L | the mermaid fixture specifically | Assert its docx extraction **contains** the diagram's source text (e.g. a `graph TD` line) as body content | nothing mocked | *(see the vacuity check — this row guards labeling, not behavior)* |

**Revert hunks, stated exactly:**
- T11.1 — Revert ⟨the shared post-construction `order` assignment for FloatRefTarget, **P5 companion Task 2**⟩ → ⟨fixture 1's docx snapshot mismatch: `Figure\u{a0}1: …` becomes an unnumbered caption⟩ RED.
- T11.2 — Revert ⟨Task 4's pptx `output-divs: false`⟩ → ⟨the pptx snapshots gain the div-derived paragraphs⟩ RED.
- T11.3 — Revert ⟨setting `crossref-numbering: external` for `Pandoc(fmt)` profiles, **P6 companion Task 1**, letting Q1 renumber⟩ → ⟨`assert_eq!(docx_number.as_bytes(), html_number.as_bytes())` reddens where the two numbering passes disagree⟩ RED. **And** revert ⟨the nbsp preservation in Task 9⟩ → the same assertion reddens, because the HTML side carries U+00A0 from `crossref_render.rs:900` and a normalized docx side would not.
- T11.4 — Revert ⟨rename on one side only⟩ → ⟨`assert_eq!(read_name, write_name)`⟩ RED.
- T11.5 — Revert ⟨delete a `DIVERGENCES.md` entry while leaving its snapshot marked⟩ → ⟨`assert!(ledger.contains_key(&snap))` in `test_divergence_ledger_complete`⟩ RED.
- T11.6 — **no behavioral revert hunk exists.** See below.

### Refactor-induced vacuity check

- **T11.3 is where the whole harness earns its keep, and it is also where nbsp normalization would
  hide.** Note the double binding above: the assertion reddens under *either* a numbering
  regression *or* an extractor over-normalization. If Task 9 ever collapses U+00A0, T11.3 does not
  redden — it goes **vacuous**, because both sides would then read `Figure 1`. That is why T9.1 is a
  byte-literal assertion and why this row's comparison is `as_bytes()`. Stated here, in the task
  that consumes it, and not only in Task 9.
- **T11.6's expected value IS the divergence, so it is non-discriminating about any *change* in
  mermaid handling.** A snapshot recording "diagram source text appears" will keep passing if
  mermaid handling changes in any way that still leaves source text present, and will fail
  (correctly, loudly) if someone ever implements real mermaid rendering — at which point the right
  action is to re-capture and delete the ledger entry, not to weaken the test. **What it actually
  guards is narrow and should not be overclaimed: that the divergence stays *labeled and
  reviewable* rather than silently drifting.** It is not evidence mermaid works, and it is not a
  substitute for the runtime warning design §12 explicitly defers.
- **`insta::assert_snapshot!` is the one assertion form that can pass by writing a new file.**
  T11.4 is the structural guard; the acceptance criterion additionally requires observing **no**
  "created new" line. A CI run with `INSTA_UPDATE` unset fails on a missing snapshot, so the
  committed-snapshot path is the gated one — but T11.4 exists because a *renamed* snapshot is the
  case that silently creates rather than fails.
- **T11.1/T11.2's revert hunks are deliberately in P3/P5/P6/Task 4, not in this task.** That is the
  point of a golden: this task adds no production behavior, so every hunk whose revert reddens it
  belongs to something else. A reviewer should read the absence of a local hunk here as correct,
  not as an unbound test.
- **The accepted-divergence ledger must not become an extractor normalization.** If a future
  contributor "fixes" a persistent divergence by teaching the extractor to skip that surface, every
  future change there becomes invisible and T11.5 still passes (the ledger entry would just be
  deleted along with the marker). Nothing tests against this; it is a review rule, recorded here
  and in the extractor contract above.

---

## Task 12: Evaluate whether any in-scope docx/pptx fixture exercises the `algorithm` theorem type, and record the outcome

**Scope.** The epic's Definition-of-done conditional; P7 owns the *evaluation* because it owns the
fixture set. Evaluate the condition, then either land the
`THEOREM_CLASSES`/`RefTypeRegistry::BUILTINS` fix or file the follow-on strand — and leave an
artifact either way.

**Files.**
- `crates/quarto-core/src/transforms/theorem.rs:61-70` — `THEOREM_CLASSES`, 8 entries
  (`theorem/thm/Theorem`, `lemma`, `corollary`, `proposition`, `conjecture`, `definition`,
  `example`, `exercise`). **No `algorithm`/`alg`.**
- `crates/quarto-core/src/crossref/registry.rs:78-106` — `BUILTINS`, 21 entries
  (`fig, tbl, lst, eq, sec, thm, lem, cor, prp, cnj, def, exm, exr, sol, rem, nte, wrn, tip, imp,
  cau, demo`). **No `alg`.** Both halves of the gap are real.
- `crates/quarto-core/tests/fixtures/pandoc-goldens/README.md` — the recorded artifact (see below).

**The evaluation criterion.** There is no `.algorithm` class in Quarto. The algorithm theorem type
is selected by the **div id prefix `#alg-`**, exactly like `#thm-`; the string `algorithm` appears
only as the LaTeX/typst *environment* name in Q1's
`v1.11.3:src/resources/filters/customnodes/theorem.lua:45-49`
(`alg = { env = "algorithm", style = "plain", title = "Algorithm" }`). So the criterion is: grep the
copied fixture set (and the 9 sourced paths at `v1.11.3`) for `#alg-`, `@alg-`, `@Alg-` — **not**
`.algorithm`, which returns zero hits regardless of whether any fixture uses the type.

**Pre-computed result (verify, don't trust).** `git grep -l -E '#alg-|\.algorithm' v1.11.3 --
'*.qmd'` over the whole quarto-cli repo at the pinned tag returns **exactly one** file:
`tests/docs/smoke-all/crossrefs/theorem/algorithm.qmd`. It is `format: typst` with
html/latex/typst/markdown assertions and **no docx assertions**, and it is **not** in Task 10's
fixture set. `tests/docs/crossrefs/theorem-types.qmd` (fixture 5) covers seven theorem types and
deliberately **omits** `alg`. **Expected outcome: the condition does not fire → file the follow-on
strand.** The task must still run the grep against the actual copied set and record what it found.

**Acceptance criterion.**
1. A committed artifact — a section in
   `crates/quarto-core/tests/fixtures/pandoc-goldens/README.md` — stating the grep command run, the
   date, the fixture set it ran against, and the result. **"No fixture uses it" must be a recorded
   artifact, not a memory.**
2. A machine-checked version of that claim: T12.2 below, so the claim cannot silently expire when
   someone adds fixture 11.
3. If the condition **does** fire: `alg`/`Algorithm` added to both `THEOREM_CLASSES` and
   `BUILTINS`, with T12.3/T12.4 bound, before the golden set is captured.
4. If it does **not** fire: a strand filed (type `task`, linked `discovered-from` the epic) naming
   both gap sites, and its id recorded in the README section.

**Prerequisite.** **Task 10**'s fixture set must exist (the condition is evaluated *against* it).
Run before Task 10's capture, so a firing condition is fixed before snapshots are taken.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T12.1 | U | `THEOREM_CLASSES` + `BUILTINS` | Assert the current tables' exact contents (8 and 21 entries, listed) — a pin, so the gap's closure or widening is visible | none | any entry added to or removed from either table |
| T12.2 | U | the fixture set + the evaluation predicate | Walk every `.qmd` under `tests/fixtures/pandoc-goldens/` → assert **none** contains `#alg-`, `@alg-`, or `@Alg-`; the test's doc comment names the strand and says what to do if it reddens | none | *(the fixture set — this reddens when someone adds an `alg` fixture, which is the intent)* |
| T12.3 | U | `TheoremSugarTransform` | `seam deferred until the condition fires` — if `alg` is added, assert `::: {#alg-gcd}` becomes `CustomNode("Theorem")` with `kind == "Algorithm"` | none | the `("algorithm", "alg", "Algorithm")` row in `THEOREM_CLASSES` |
| T12.4 | U | `RefTypeRegistry::builtin` | `seam deferred until the condition fires` — assert `registry.kind_for("alg") == Some("Algorithm")` | none | the `("alg", "Algorithm")` row in `BUILTINS` |

**Revert hunks, stated exactly:**
- T12.1 — Revert ⟨add `("algorithm", "alg", "Algorithm")` to `THEOREM_CLASSES`⟩ → ⟨`assert_eq!(THEOREM_CLASSES.len(), 8)` and the listed-contents assertion⟩ RED. Stated as a *deliberate* revert: this row's job is to make the gap's eventual closure a visible, reviewed change rather than a silent one.
- T12.2 — Revert ⟨add `tests/docs/smoke-all/crossrefs/theorem/algorithm.qmd` to the fixture set⟩ → ⟨`assert!(hits.is_empty())` in `test_no_fixture_uses_alg`⟩ RED, at which point T12.3/T12.4 become required. **This is the mechanism that keeps the recorded outcome honest.**
- T12.3, T12.4 — `seam deferred until the condition fires`. No placeholder hunk is written.

### Refactor-induced vacuity check

- **A grep for `.algorithm` is the vacuous form of this whole task.** It returns zero hits whether
  or not any fixture exercises algorithms, because that is not Quarto's syntax. The predicate must
  be `#alg-`/`@alg-`/`@Alg-`. T12.2 encodes the correct predicate so the evaluation cannot be
  re-run wrongly later.
- **A README sentence alone is vacuous.** "No in-scope fixture uses `.algorithm`" is true on the day
  it is written and silently false the day someone adds fixture 11. T12.2 is what makes the claim
  expire loudly. This is exactly the discipline's "an absent test cannot be found by a revert" case.
- **T12.1's expected counts are frozen literals, not `len()`-derived.** A computed expectation
  follows any change to either table and stops discriminating.

---

## Missing-test pass

Reasoned across P7's whole change for behavior with **no** test — the load-bearing safety branches,
the documented limitations, and the structural contracts successor plans depend on. Every item gets
a bound seam or an explicit `accepted-untested: <rationale>`. Silent omission reads as "covered."

### The nine "minus" items in the epic's v1 envelope

Each is a user-visible documented limitation (`2026-08-20-pandoc-hybrid-epic.md:170-188`), so each
needs a labeled golden, a bound seam, or an `accepted-untested` verdict.

| # | Envelope item | Verdict |
|---|---|---|
| 1 | **No mermaid rendering** (source text appears instead) | **Labeled golden** — fixture 9, T11.6, `DIVERGENCES.md` naming `bd-h1ub8f8z`. The self-gate `mermaid.rs:175` (`if !ctx.format.identifier.is_html_based() { return Ok(()) }`) additionally gets a bound unit row: `accepted-untested` is **not** used here. Add **T11.7 (`U`)**: assert `MermaidTransform::transform` is a no-op for a docx `RenderContext` — Revert ⟨the `is_html_based()` self-gate at `mermaid.rs:175`⟩ → ⟨`assert_eq!(ast_before, ast_after)`⟩ RED. |
| 2 | **No section numbers** (`number-sections` / `@sec-`, `bd-5aklrxgi`) | **Bound negative seam.** Add **T4.11 (`U`)**: assert the pandoc-defaults allow-list does **not** contain `number-sections` or `number-offset` — this is design §11's frozen decision (forwarding them would produce a third, unaudited behavior; Q1 itself *deletes* them, `v1.11.3:src/command/render/pandoc.ts:1056-1057`, guarded at `:1048-1052`, comment at `:1044-1047`). Revert ⟨add `number-sections` to the allow-list⟩ → ⟨`assert!(!allow_list.contains("number-sections"))`⟩ RED. **Verified (measured):** docx is neither latex, typst, nor markdown output, so docx **does** take Q1's delete branch — Q1's Lua owns numbering for docx, which is precisely why forwarding is wrong. |
| 3 | **No code-block `filename` headers** (upstream, quarto-cli#14906) | `accepted-untested: the loss happens inside Q1's own vendored `decoratedcodeblock.lua`, which has renderers for html/markdown/latex only; there is no Q2-side hunk whose revert could redden a test, and design §11 resolved this as an accepted upstream gap explicitly out of epic scope. The `filename` fixtures are deliberately excluded from the golden set (Task 10) so no snapshot silently encodes the loss as a Q2 baseline.` |
| 4 | **Listing pages render prose-only** | `accepted-untested: `listing-generate`/`listing-render` are excluded for the Pandoc leg by **P1 Task 2**'s exclude list, which P1 tests directly; a P7 test would duplicate P1's binding and drift from it. P7-foundation's T4.1 "contains" assertion covers the inverse direction (what must *not* be excluded).` |
| 5 | **No project-mode rendering** | **Bound in P7-foundation** — its Task 2 in full (T2.1-T2.6), with T2.2/T2.5 as the "path was actually exercised" rows. |
| 6 | **One format per invocation, now with a warning** | **Bound in P7-foundation** — its Task 1 (T1.1-T1.3, `U`) + Task 3's T3.4 (`E`, the conjunction). |
| 7 | **`Post`-position user Lua filters can't see custom-node content** (`bd-o90yz5mg`) | **Bound in P7-foundation** — its Task 3's T3.8 (`I`): for a `Pandoc("docx")` render, assert `UserFiltersStage::post()` observes an AST that still contains `CustomNode` wrappers, and that the **`Pre`** position observes none (`pre()` runs before any sugar transform). Revert ⟨reorder `UserFiltersStage::post()` after the wire-format cut⟩ → ⟨the `Pre`-sees-no-CustomNode assertion⟩ RED. This pins the *documented* shape so a future reorder is a reviewed change, not a surprise. |
| 8 | **An explicit `crossref:` override is honored on Pandoc and silently ignored on HTML** (`bd-wqdi1pd2`) | `accepted-untested: verified zero Q2 readers of `crossref.title-delim`/`fig-prefix`/`ref-hyperlink` anywhere in `crates/` — only `crossref_render.rs:29`'s own comment admits the gap (`crossref_render.rs:26-31` hard-codes `"<Kind> <N>: "`). A test asserting the asymmetry would pin a *bug* the epic explicitly declines to fix, and would redden the day `bd-wqdi1pd2` lands. The `crossref:`-presentation fixtures are excluded from the golden set for the same reason.` |
| 9 | **A callout whose crossref category Q2 knows and Q1 doesn't renders unnumbered, with a dangling ref** | `accepted-untested in P7: the `fail()`-fallback, its `by_ref_type` guard and its warning are **P5 companion Task 6**'s hunk and tests (design §12's last bullet, P6 Finding 2). P7's fixture 3 (`tests/docs/crossrefs/callouts.qmd`) exercises only Q1-known categories, so no P7 golden encodes the fallback. If P5 Task 6's tests are ever dropped, nothing in P7 catches it — recorded so that is a known, not a discovered, gap.` |

### Other load-bearing branches

| Behavior | Verdict |
|---|---|
| **The `algorithm`/`THEOREM_CLASSES` condition** | **Bound** — Task 12 in full, with T12.2 as the expiry mechanism and a committed README artifact (not a memory). |
| **`--reference-doc` — file missing** | **Bound** — T4.7 (`E`), asserting the Q2-side span-bearing diagnostic, which fires *before* pandoc. **(measured)** without it pandoc 3.8.1 exits 99 with the bare line `File X not found in resource path`. |
| **`--reference-doc` — file present but malformed** | `accepted-untested: outside `MarkPolicy::ExistenceDiagnose`'s reach (existence, not content), and P7's plan does not name a content check. **(measured)** pandoc 3.8.1 exits **1** with a GHC `CallStack` backtrace (`Data.Binary.Get.runGet at position 4: Did not find end of central directory signature`), which P4 Task 10's verbatim nonzero-exit passthrough would print to the user as-is. A content-validity check (e.g. a `PK\x03\x04` pre-flight on `reference-doc`) would be new functionality nobody has scoped — see Open questions.` |
| **`pandoc` absent, at the CLI level** | **Bound, by reference.** P4 Task 7 owns the check and its `Q-18-*` code. **P7-foundation's Task 3 adds T3.9 (`E`)**: run `q2 render f.qmd --to docx` with a `PATH` containing no `pandoc` → non-zero exit, stderr contains the `Q-18-*` code and the word `pandoc`, and **no** partial `.docx` is left on disk. Revert ⟨P4's pre-flight check, letting the `Command::new("pandoc")` spawn fail⟩ → ⟨`assert!(stderr.contains("Q-18-"))` and `assert!(!out.join("f.docx").exists())`⟩ RED. This is the row that answers "does the CLI surface it *well*", which P4's own tier cannot: P4's `L`-tier tests *require* pandoc. |
| **`pandoc` present but below the floor** | `accepted-untested: P4 Task 7 owns the version comparison and its gate, with its own tests. An `E` row would need a stub `pandoc` on `PATH` reporting a low version, which is a second copy of P4's harness; the CLI-surfacing shape is already covered by P7-foundation's T3.9 code-and-message assertion, which shares the diagnostic path.` |
| **Binary output — the contract beyond "a file exists"** | **Bound, three ways, because "a file exists" is exactly the failure mode.** (a) P7-foundation's T3.1/T3.2 assert the zip magic `PK\x03\x04` and a required internal entry (`word/document.xml` / `ppt/slides/slide1.xml`) — this is what rules out HTML-in-a-`.docx`. (b) **P7-foundation's T3.10 (`E`)**: assert the output file's length is **> 0** and that `Content-Type`-shaped sniffing does not match text — concretely, `assert_ne!(&bytes[..2], b"<!")` and `assert!(bytes.len() > 1000)`, guarding the zero-byte and HTML-written-to-a-binary-path cases. Revert ⟨P4's `content: String::new()` decision into "write `RenderedOutput.content` to the output path"⟩ → ⟨T3.10's length assertion reddens with a 0-byte file⟩ RED. (c) **T9.9 (this document's Task 9)** makes the extractor reject a non-OOXML input with `Err`, so a corrupt output cannot pass Task 11 as an empty extraction. |
| **The HTML-leg nested-`<p>` symptom** | `accepted-untested: HTML-only, with no docx/pptx surface; the docx-relevant half (the `MetaBlocks`→`MetaInlines` coercion) **is** bound, at T6.2/T6.5. Flagged, not fixed, per Task 8; needs a strand if one does not already exist.` |
| **The `latex` stub** | **Bound negatively** — T4.9 asserts `--to latex` still refuses. See Task 4's vacuity check for what it deliberately does not assert. |
| **The `G` tier's own skip** | **Bound** — T10.1/T10.2 make a missing or wrong-version `quarto` a loud failure, never a skip; T10.7 keeps the capture out of `verify`/CI. |
| **`cargo nextest run --workspace` + `cargo xtask verify`** | **Bound as a task-exit gate, not a test.** Per the user-level instruction, each task gates on `cargo clippy -p <crate> --all-targets -- -D warnings` + `cargo nextest run -p <crate>`; the **workspace** run happens once per phase boundary and before any push, reported as a delta against the live baseline. `RenderContext` gains a field in **P1 Task 1**, so P7's phase-boundary verify must be **full** `cargo xtask verify` (not `--skip-hub-build`) — `quarto-core` is in `wasm-quarto-hub-client`'s closure. |

---

## Open questions

- **A malformed `--reference-doc` surfaces a GHC backtrace to the user.** **(measured, pandoc
  3.8.1)** a non-zip file passed as `--reference-doc` exits **1** with
  `pandoc: Data.Binary.Get.runGet at position 4: Did not find end of central directory signature`
  plus a `CallStack`/`HasCallStack` trace. Under P4 Task 10's verbatim nonzero-exit passthrough,
  that is what the user reads. The *missing*-file case is clean (Task 4 diagnoses it with a span
  before pandoc runs, via `MarkPolicy::ExistenceDiagnose`), but the malformed case is outside that
  policy's reach and no task above names it. Whether to add a content-validity check (e.g. a
  `PK\x03\x04` pre-flight on `reference-doc`) is Gordon's call — it would be new functionality
  nobody has scoped yet.
