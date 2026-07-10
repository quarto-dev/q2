# P7 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P7-format-tail.md`](2026-08-20-pandoc-hybrid-P7-format-tail.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§11 golden-capture strategy, §12 known limitations, §13 project-mode gate, §14 multi-format guardrail)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Research companion:** [`../research/2026-07-13-q1-format-typescript.md`](../research/2026-07-13-q1-format-typescript.md)
**Sibling companions:** [`P1`](2026-09-18-pandoc-hybrid-P1-implementation.md), [`P2`](2026-09-18-pandoc-hybrid-P2-implementation.md), [`P3`](2026-09-18-pandoc-hybrid-P3-implementation.md), [`P4`](2026-09-18-pandoc-hybrid-P4-implementation.md), [`P5`](2026-09-18-pandoc-hybrid-P5-implementation.md), [`P6`](2026-09-18-pandoc-hybrid-P6-implementation.md) — prerequisites below cite their task numbers
**Depends on:** P1, P2, P4, P5 — **and P6**, for correct numbers in its golden (per the epic's graph). P7 is last.
**Status:** Ready for subagent-driven execution, with the caveats in **Findings for Gordon**.

This file adds nothing to P7's scope — it converts P7's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P7 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P7 + the design doc; where this file and the plan disagree, the plan wins
— except where **Findings for Gordon** records a measured contradiction, which needs a decision
before the affected task is dispatched.

**Provenance of the anchors below.** Every Rust citation was re-read against this worktree
(`feature/pandoc-writer-hybrid`) on 2026-09-18. Every quarto-cli fixture path was verified to
**exist at tag `v1.11.3`** with `git cat-file -e v1.11.3:<path>` — the local checkout is now
`v1.11.5-1-g83d48d8e8`, so the pinned tag must be read via `git show v1.11.3:<path>`, never from
the worktree (same discipline P4's companion adopted). Behaviour claims marked **(measured)** were
reproduced by running **pandoc 3.8.1** locally while writing this file; the OOXML element/style
names in the extractor spec are all measured, not recalled.

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

**Row tally across all 12 tasks (including the rows added in the Missing-test pass):**
`U=39`, `I=7`, `E=15`, `L=9`, `G=1`, `X=4` — 75 bound rows, plus **3 deferred seams**
(T8.3, T12.3, T12.4). The `I`/`L` split is by *environment*, not by file location: a row that
reaches `PandocWriteStage` shells out to a real `pandoc` and is therefore `L`, even though it lives
in an `integration` binary and is invoked in-process. Nine such rows are labelled `L` rather than
`I` so the gate policy below actually covers them.

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
| `q2 render f.qmd --to docx` produces a real `.docx` | **Yes** | T3.1 (`E`) |
| `q2 render f.qmd --to pptx` produces a real `.pptx` | **Yes** — separately from docx, per the round-4 pptx finding | T3.2 (`E`) |
| Multi-format `format:` block warns, naming used + skipped | **Yes** | T3.4 (`E`); message construction at T1.1-T1.3 (`U`) |
| Website project + `--to docx` writes no sitemap / no alias redirects | **Yes** | T2.4 (`E`) |
| Website project + `--to html` **still** writes them | **Yes** (the "path was actually exercised" half) | T2.5 (`E`) |
| `--reference-doc` reaches pandoc and changes the output | **Yes** | T4.6 (`E`) |
| `--reference-doc` pointing at a missing file diagnoses with a span | **Yes** | T4.7 (`E`) |
| Document metadata reaches `docProps/core.xml` | **Yes** | T6.3 (`E`) |
| Staged resources / rewritten links survive into the docx | **Yes** | T7.3 (`E`) |
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
7. **`<m:oMath>` flattened text content + element count** (round-4, Reviewer B). **(measured)**
   `$$x=1$$` produces one `m:oMath` in docx and one in pptx. Without this, the extraction is
   byte-identical whether an equation number is present, absent, or wrong.
8. **Image/media inventory** — the `word/media/` (resp. `ppt/media/`) file-name list, and the
   `word/_rels/document.xml.rels` relationships **whose `Type` ends in `/image`**, by `Target`.
   **Precision on P7's own wording (which says "entry count + targets"): a raw entry count is
   version-noise, not signal.** **(measured)** an image-free document already carries 7+
   relationships (`numbering`, `styles`, `settings`, `theme`, `fontTable`, `webSettings`,
   `footnotes`). Filter to image relationships, or the count drifts with any pandoc change to the
   default reference doc.
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

## Which of P7's tasks creates the golden harness (predecessor plans defer to it)

The harness is **Tasks 9, 10, 11**, and the borrowable unit is **Task 9**:

- **Task 9 — `crates/quarto-ooxml-extract/`**, a new leaf crate holding the extraction function.
  This is the piece both the `G`-tier xtask and the `I`-tier assertion test call, and the piece a
  predecessor plan would want. It depends only on `zip` + `quick-xml` — deliberately **not** on
  `quarto-core`, because `xtask` has no `quarto-core` dependency today (verified:
  `crates/xtask/Cargo.toml` deps are `anyhow, clap, nodejs-semver, proc-macro2, serde, serde_json,
  serde_yaml, syn, tempfile, time, walkdir`) and adding one would put the whole engine closure in
  front of every `cargo xtask lint`. This is the concrete answer to P7 Finding 4's "the shared
  extraction function needs a home."
- **Task 10 — the `G`-tier capture** (`cargo xtask capture-pandoc-goldens`) + the fixture copy-in.
- **Task 11 — the `I`-tier assertion** against the same committed snapshots, plus per-fixture
  accepted-divergence provenance.

**P7 runs last, so P5 and P6 cannot borrow Task 9 — and both have now resolved that on their own
side, so P7 inherits nothing from them.** Both plans recorded the ordering problem and offered the
same two options; both sibling companions then picked option **(a)**, a narrower harness of their
own, rather than deferring behind P7:

- P5's Layer-2 goldens item (`P5-lua-shim.md:655-659`) → resolved as **P5 companion Task 8**,
  *"Layer-2 per-type goldens — a narrow post-filter-AST harness, committed as `insta` snapshots"*.
  It asserts on the **post-filter Pandoc AST**, not on rendered OOXML, so it needs no extractor.
- P6's number-parity item (`P6-*.md:272-278`) → resolved as **P6 companion Task 5**,
  *"Figure / theorem / callout number-parity goldens — **schedulable without P7**"*.

**Consequence for P7:** Task 11 is not carrying deferred predecessor items, and Task 10's fixture
set has no inherited obligation to mirror theirs. What P7 adds on top is the one thing neither
sibling harness can reach — **the rendered docx/pptx surface**, where a number can be correct in the
post-filter AST and still be lost by the writer (the `<m:oMath>` case is exactly that shape). T11.3
is therefore a genuine second, independent check of the epic's number-identity claim, not a
duplicate of P6 Task 5. See **Findings for Gordon** F7 for the one residual coupling.

---

## Task 1: The multi-format render warning — pure diagnostic + its `Q-18-*` code, page and sidebar entry

**Scope.** A pure function that, given a document's `format:` declaration and the single key that
was actually used, returns a `Q-18-*` warning naming the used key and the skipped keys. Lands
**before** Task 3's relaxation, so the warning exists the moment the guardrail it replaces is
removed (design doc §14). No rendering capability is added — this is one diagnostic.

**Files.**
- `crates/quarto-core/src/format.rs` — new `pub fn multi_format_diagnostics(...) -> Vec<DiagnosticMessage>`,
  next to the two places that silently reduce a multi-key `format:` to one:
  `format_key_from_frontmatter` at `format.rs:182-190` (`serde_yaml::Value::Mapping(m) =>
  m.keys().find_map(...)` — the CLI path) and `format_key_from_config_value` at `format.rs:208-217`
  (`entries.first()` — the project/pipeline path). Model it on `project_kind_diagnostics`
  (`crates/quarto-core/src/project/mod.rs:352-379`), which is the house shape: a pure function
  returning `Vec<DiagnosticMessage>`, built with `DiagnosticMessageBuilder::warning(...)
  .with_code("Q-5-18").problem(...)` (`mod.rs:365-378`).
- `crates/quarto-error-catalog/error_catalog.json` — one new `Q-18-*` entry in the `pandoc`
  subsystem **P4 Task 6 reserves** (subsystem number 18, frozen there). P7 *extends* that code
  set; it does not reserve a subsystem.
- `docs/errors/pandoc/Q-18-<n>.qmd` — the page, per `docs/errors/README.md`;
  `docs_url` exactly `https://quarto.org/docs/errors/pandoc/Q-18-<n>`.
- `docs/_quarto.yml` — the sidebar entry in P4's `- section: "pandoc"` block, **ascending by code
  number** (the `error-docs-sidebar-unlisted` rule enforces intra-section numeric order).

**Acceptance criterion.**
1. `multi_format_diagnostics` given `format: {docx: default, html: default}` and used key `"docx"`
   returns exactly one warning whose code is the new `Q-18-<n>`, whose text names **`docx`** as
   used and **`html`** as skipped, and which lists skipped keys in the document's declaration
   order.
2. Given a single-key `format:` (map or scalar), and given an absent `format:`, it returns an
   empty vec.
3. `cargo xtask lint` green — specifically `error-docs-page-missing`
   (`crates/xtask/src/lint/error_docs.rs`) and `error-docs-sidebar-unlisted`
   (`crates/xtask/src/lint/error_docs_sidebar.rs`).
4. Nothing is wired into `render.rs` yet — that is Task 3, deliberately.

**Prerequisite.** **P4 Task 6** (the `pandoc` subsystem, number 18, with its `- section: "pandoc"`
sidebar block). Without it this task has nowhere to put the code and both lint rules fail.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | U | `format::multi_format_diagnostics` | Call with a 2-key `format:` map + used `"docx"` → assert `len()==1`, `.code()=="Q-18-<n>"`, message contains `docx` **and** `html`, and `html` appears in the "skipped" clause not the "used" clause | none | the `.with_code("Q-18-<n>")` call, and the skipped-keys `format!` argument |
| T1.2 | U | same | Call with a 3-key map (`docx, html, pptx`), used `"docx"` → assert the skipped list is `["html", "pptx"]` **in declaration order** | none | the iteration that builds the skipped list |
| T1.3 | U | same | Call with a 1-key map, with a scalar `format: html`, and with `format:` absent → assert empty vec in all three | none | the `keys().count() > 1` guard |
| T1.4 | X | `xtask::lint::error_docs::check(workspace_root)` | Run against the real `workspace_root` → zero violations | none | `docs/errors/pandoc/Q-18-<n>.qmd` deleted |
| T1.5 | X | `xtask::lint::error_docs_sidebar::check(workspace_root)` | Run against the real `workspace_root` → zero violations | none | the new sidebar entry in `docs/_quarto.yml` |
| T1.6 | U | `quarto-error-catalog`'s catalog data | Load → assert the new code's `subsystem == "pandoc"` and `docs_url == "https://quarto.org/docs/errors/pandoc/Q-18-<n>"` | none | the entry's `docs_url` field |

**Revert hunks, stated exactly:**
- T1.1 — Revert ⟨the skipped-keys `format!` argument to a constant string such as `"other formats"`⟩ → ⟨`assert!(msg.contains("html"))` in `test_multi_format_names_skipped_key`⟩ RED.
- T1.2 — Revert ⟨the declaration-order iteration to a `BTreeSet`/sorted collect⟩ → ⟨`assert_eq!(skipped, ["html", "pptx"])` in `test_multi_format_skipped_order`⟩ RED. (Chosen because `docx, html, pptx` is *already* alphabetical; the fixture therefore uses a declaration order that is **not** alphabetical — `format: {docx:, pptx:, html:}` → expected `["pptx", "html"]` — so a sort collapses the discriminator and the test reddens. See the vacuity check.)
- T1.3 — Revert ⟨the `> 1` guard to `>= 1`⟩ → ⟨`assert!(diags.is_empty())` in `test_single_format_no_warning`⟩ RED.
- T1.4 — Revert ⟨delete `docs/errors/pandoc/Q-18-<n>.qmd`⟩ → ⟨`assert!(violations.is_empty())`⟩ RED.
- T1.5 — Revert ⟨delete the new sidebar entry⟩ → ⟨`assert!(violations.is_empty())`⟩ RED.
- T1.6 — Revert ⟨the entry's `docs_url` to the `lua` subsystem's URL shape⟩ → ⟨`assert_eq!(entry.docs_url, expected)`⟩ RED.

### Refactor-induced vacuity check

- **T1.2's expected value is the one that can collapse.** If the fixture's declaration order is
  alphabetical, the assertion reads identical whether the implementation preserves declaration
  order or sorts — it then survives its own revert. The fixture is therefore pinned to
  `{docx:, pptx:, html:}` with expected `["pptx", "html"]`, which differs across the two states.
- **T1.1 must not assert on the whole message string.** A whole-string equality assertion is
  brittle against harmless wording edits *and* non-discriminating about which key landed in which
  clause. Assert the two clauses separately (`used` clause contains `docx` and not `html`;
  `skipped` clause contains `html`).
- **This task's tests deliberately do not assert that the warning ever fires in a real render.**
  Before Task 3, the multi-format case still hard-errors at `render.rs:680-684`, so an `E`-tier
  test written here would pass **for the wrong reason** — the old refusal, not the new warning.
  That ordering trap is the whole point of §14, and its resolution is T3.4, not a test here. See
  Task 3's vacuity check.

---

## Task 2: The project-mode containment gate (design doc §13, Gordon's decision)

**Scope.** Gate `WebsiteProjectType::post_render`'s hook sequence on
`format.identifier.is_html_based()`, so a website/book/manuscript project rendered to a Pandoc
target does not write a sitemap of `.html` URLs that do not exist, and does not hard-fail in
`write_alias_redirects` with an HTML-specific diagnostic. Matches Q1's own
`websiteProjectType.postRender`, which already filters `outputFiles` to HTML-only. Lands before
Task 3, which is what makes the risk reachable.

**Files.**
- `crates/quarto-core/src/project/orchestrator.rs` — **the gate goes at the call site**,
  `orchestrator.rs:1220-1231` (`self.project_type.post_render(...)`), because `self.format` is
  already in scope there and used eleven lines earlier at `:1122`
  (`self.format.identifier.as_str()`). **`async fn post_render` at `orchestrator.rs:517-580` takes
  no `format` argument** (params: `project, index, output_paths, project_artifacts, resolver,
  runtime, diagnostics`) — so the alternative shape, threading a `format: &Format` into the trait
  method, also touches the trait definition at `orchestrator.rs:397-406` and the test double at
  `crates/quarto-core/tests/integration/project_pipeline.rs:227`. The call-site gate is chosen
  here; it is the "one-line fix" §13 and `bd-bgeet2mw` both describe. See Findings F4.
- `crates/quarto-core/src/format.rs:63-65` — `is_html_based()`, read-only. **Do not widen it.**
- `crates/quarto-core/tests/integration/website_post_render_format_gate.rs` — new; registered in
  `crates/quarto-core/tests/integration/main.rs` alphabetically.
- `crates/quarto/tests/integration/project_pandoc_gate_e2e.rs` — new; registered in
  `crates/quarto/tests/integration/main.rs`.

**Acceptance criterion.** With a minimal website project (`_quarto.yml` with
`project: type: website`, `website: site-url: https://example.com`, one `index.qmd` carrying an
`aliases:` entry):

```
$ cargo run --bin q2 -- render . --to docx
$ ls _site
index.docx
$ ls _site/sitemap.xml _site/robots.txt 2>&1
ls: _site/sitemap.xml: No such file or directory
ls: _site/robots.txt: No such file or directory
```

and, for the same project with `--to html`, `_site/sitemap.xml`, `_site/robots.txt` and the alias
redirect stub **are all present**. Both halves inspected; the second half is what makes the first
half mean something.

**Prerequisite.** Task 3's relaxation is **not** required to *test* this: `Format::from_format_string("docx")`
already returns `Ok` today (`FormatIdentifier::Docx` exists at `format.rs:29`), so the `I`-tier
rows can construct a docx `Format` and drive the orchestrator in-process right now. The `E`-tier
rows (T2.4/T2.5) **are** gated on Task 3 — before it, `render.rs:680` refuses. Dispatch this task
before Task 3 but land T2.4/T2.5 with, or immediately after, Task 3.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | L | the real orchestrator + `WebsiteProjectType::post_render` | Drive a website project to completion with a **docx** `Format` → assert no `sitemap.xml`, no `robots.txt`, no alias stub under `output_dir` | filesystem via `tempfile`; no mocked project type; **real pandoc** (the docx render reaches `PandocWriteStage`) | the `if self.format.identifier.is_html_based()` guard at `orchestrator.rs:1220` |
| T2.2 | I | same | Same project with an **html** `Format` → assert `sitemap.xml`, `robots.txt` and the alias stub **do** exist | as above, no pandoc needed (native HTML leg) | the same guard, inverted |
| T2.3 | L | same, alias-collision path | A docx render of a project whose `aliases:` collide (the case `write_alias_redirects` turns into an *error*, `orchestrator.rs:548-552`) → assert the render returns `Ok` | as T2.1 | the guard (without it, `write_alias_redirects` hard-fails the render) |
| T2.4 | **E** | the real `q2` binary, project render | `q2 render . --to docx` → exit 0, `_site/index.docx` exists and is non-empty, `_site/sitemap.xml` does **not** exist | nothing mocked | the guard |
| T2.5 | **E** | same | `q2 render . --to html` → exit 0, `_site/index.html` **and** `_site/sitemap.xml` both exist | nothing mocked | the guard |
| T2.6 | U | `FormatIdentifier::is_html_based` | Assert `Docx`/`Pptx`/`Pdf`/`Gfm` → `false`; `Html`/`Revealjs` → `true` | none | the `matches!` arm at `format.rs:64` |

**Revert hunks, stated exactly:**
- T2.1 — Revert ⟨remove the `is_html_based()` guard at `orchestrator.rs:1220`, restoring the unconditional `post_render` call⟩ → ⟨`assert!(!out.join("sitemap.xml").exists())` in `test_docx_project_writes_no_sitemap`⟩ RED.
- T2.2 — Revert ⟨widen the guard to `if false`, i.e. gate *everything* off⟩ → ⟨`assert!(out.join("sitemap.xml").exists())` in `test_html_project_still_writes_sitemap`⟩ RED. **This is the "the path was actually exercised" row.**
- T2.3 — Revert ⟨the guard⟩ → ⟨`assert!(result.is_ok())` in `test_docx_project_survives_alias_collision`⟩ RED (`write_alias_redirects` returns `Err`).
- T2.4 — Revert ⟨the guard⟩ → ⟨the `!sitemap.exists()` assertion in `test_e2e_docx_project_no_sitemap`⟩ RED.
- T2.5 — Revert ⟨gate the hooks on `is_native()` **and** additionally on `identifier == Html`, dropping revealjs⟩ → ⟨`test_e2e_html_project_sitemap_present`, extended to a `revealjs` project, reddens⟩. Stated for the revealjs sub-case; the `Html` sub-case is covered by T2.2.
- T2.6 — Revert ⟨add `Docx` to the `is_html_based` `matches!` arm⟩ → ⟨`assert!(!FormatIdentifier::Docx.is_html_based())`⟩ RED.

### Refactor-induced vacuity check

- **The gate produces the *absence* of artifacts, and an absence assertion is satisfied by any
  render that fails early.** A docx render that panicked in the pipeline also writes no
  `sitemap.xml`. T2.1/T2.4 therefore **must** pair the absence assertion with a positive one:
  the render returned `Ok` / exited 0, **and** `_site/index.docx` exists and is non-empty. Without
  that pairing the test passes before the gate exists (today's `render.rs:680` refusal produces
  exactly the same absence) and keeps passing after any future regression that breaks docx
  rendering entirely.
- **T2.2/T2.5 are the "the path was actually exercised" rows** and are not optional garnish: they
  are the only thing distinguishing "the gate is correct" from "the hook sequence is broken for
  everyone."
- **`is_html_based()` and `is_native()` return the same value for every `FormatIdentifier` variant
  that exists today** (`format.rs:58-60` vs `:63-65`, both `matches!(Html | Revealjs)`), and P1
  Task 1 adds `Pptx` to neither. So a refactor that routes this gate through `is_native()` by
  mistake is **behaviorally invisible** to any test that only checks outcomes. T2.6 pins the
  predicate's own table, and the gate must be written against `is_html_based()` because that is the
  one whose *meaning* is "HTML family" — `is_native()`'s meaning ("renders in-process") is exactly
  what Task 3 stops being true of a supported format. Flagged in Findings F5; no code change is
  requested, only awareness that these two tests cannot distinguish the predicates.
- **T2.3's expected value is `Ok`, which is also what a no-op render returns.** Pair it with the
  same non-empty-output assertion as T2.1.

---

## Task 3: Relax the format gate — admit **docx and pptx**, route through `render_qmd_to_pandoc`, wire the warning in

**Scope.** Replace `render.rs`'s blanket non-native refusal with one that admits docx and pptx and
routes them to P4's Pandoc entry point, and emit Task 1's warning at the same site. Verified
end-to-end for **both** formats.

**Files.**
- `crates/quarto/src/commands/render.rs:680-684` — **citation verified accurate today**: line 680
  is `if !format.identifier.is_native() {`, 681-684 the `anyhow::bail!("Format '{}' is not yet
  supported. Only HTML and revealjs are available in this version.", …)`. Relax to admit
  `Docx | Pptx` (and keep refusing `Pdf | Epub | Typst | Gfm | CommonMark`).
- `crates/quarto/src/commands/render.rs:676` — `let format = resolve_format(&format_str)?;`.
  **This is the round-4 Critical finding, verified:** `resolve_format` (`render.rs:1490-1492`)
  delegates to `Format::from_format_string`, whose only `pptx` outcome today is
  `Err(format!("Unknown format: {}", format_str))` at **`format.rs:449`** (P7's text cites
  `:447`; the `Err` is at `:449`, the comment at `:448`). `FormatIdentifier` has no `Pptx` variant
  (`format.rs:23-41`), and the resolution path that actually matters is the **non-exhaustive**
  `TryFrom<&str> for FormatIdentifier` string match at `format.rs:82-96` — the compiler will not
  force it. **P1 Task 1 adds the variant plus all four edits.** Relaxing line 680 alone leaves
  pptx failing four lines earlier.
- `crates/quarto/src/commands/render.rs` — call `multi_format_diagnostics` next to
  `detect_single_input_format` (`render.rs:673-676`), which is the site that reduces a multi-key
  `format:` to one via `format_key_from_frontmatter` (`format.rs:186`).
- `crates/quarto-core/src/render_to_file.rs:357-364` — the real call chain today is
  `render.rs` → `render_document_to_file` → `render_qmd_to_html(...)` at `:357`. Branch to P4 Task
  9's `render_qmd_to_pandoc` for `Pandoc(fmt)` profiles, and confirm that P4's decision — an
  **empty** `RenderedOutput.content` with a populated `output_path` — does not break the
  `OutputSink` / artifact path immediately below (`render_to_file.rs:366-380`, `OutputSink::new(resolver.allowed_output_roots())`).
- `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs` — new; registered in
  `crates/quarto/tests/integration/main.rs`.

**Acceptance criterion.** Exact invocations and observed output, both inspected:

```
$ cargo run --bin q2 -- render fixture.qmd --to docx
$ file fixture.docx
fixture.docx: Microsoft Word 2007+
$ unzip -p fixture.docx word/document.xml | grep -o '<w:t[^>]*>[^<]*</w:t>' | head -1
<w:t xml:space="preserve">Hello</w:t>

$ cargo run --bin q2 -- render fixture.qmd --to pptx
$ unzip -l fixture.pptx | grep ppt/slides/slide1.xml
     1730  ...  ppt/slides/slide1.xml

$ cargo run --bin q2 -- render multi.qmd     # front matter: format: {docx: default, html: default}
Warning: [Q-18-<n>] `format:` declares more than one format; only one is rendered
  ...rendered `docx`; skipped `html`.
$ ls multi.docx && test ! -e multi.html && echo "html correctly not produced"
```

`--to pdf` / `--to epub` / `--to typst` must still refuse with the existing message.

**Prerequisite.** **P1 Task 1** (`FormatIdentifier::Pptx` — without it `--to pptx` is unreachable
and the pptx half of this task is untestable), **P1 Task 2** (the `Pandoc`-kind transform
exclude-list) and **P1 Task 6** (the stage-level one), **P2 Task 5** (the `Meta` carriage),
**P4 Task 9** (`render_qmd_to_pandoc` + `PandocWriteStage`), **P4 Task 10** (the pandoc-subprocess
diagnostic — this is what surfaces a pandoc failure through the CLI), **Task 1 of this plan** (the
warning), and — for the *content* to be right, though not for this task's assertions, which are
structural — **P5 companion Tasks 1-5** (the shim's recognizer + Route R/N) and
**P6 companion Tasks 1, 2 and 4** (`crossref-numbering: external` for `Pandoc(fmt)` profiles, the
Callout reclassification, and the suppression matrix).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | **E** | the real `q2` binary, full `execute()` chain through clap | `q2 render f.qmd --to docx` → exit 0; `f.docx` first 4 bytes `PK\x03\x04`; `word/document.xml` present; its concatenated `<w:t>` text contains the fixture's body string | nothing mocked | the `Docx` arm of the relaxed gate at `render.rs:680` |
| T3.2 | **E** | same | `q2 render f.qmd --to pptx` → exit 0; `f.pptx` is a zip containing `ppt/slides/slide1.xml`; its `<a:t>` text contains the body string | nothing mocked | `FormatIdentifier::Pptx`'s arm in `TryFrom<&str>` (`format.rs:82-96`) **and** the `Pptx` arm of the relaxed gate — two separate hunks, T3.3 separates them |
| T3.3 | U | `Format::from_format_string` | `from_format_string("pptx")` → `Ok`, `identifier == Pptx`, `output_extension == "pptx"`, `native_pipeline == false`; and `from_format_string("docx")` → `Ok` | none | the `"pptx" => Ok(FormatIdentifier::Pptx)` arm at `format.rs:82-96` |
| T3.4 | **E** | the real binary + the warning wiring | `q2 render multi.qmd` (front matter `format: {docx: default, html: default}`) → **exit 0**, stderr contains `Q-18-<n>` and both `docx` and `html`, `multi.docx` exists and is non-empty, `multi.html` does **not** exist | nothing mocked | the `multi_format_diagnostics` call in `render.rs` |
| T3.5 | **E** | same | `q2 render f.qmd --to pdf` → non-zero exit, stderr contains `is not yet supported` | nothing mocked | the `Pdf` arm still refused by the relaxed gate |
| T3.6 | L | `render_document_to_file`'s profile branch | Call with a docx `Format` → assert the returned `RenderedOutput.content.is_empty()` and `output_path` ends `.docx`, and the file on disk is non-empty | filesystem via `tempfile`; **real pandoc** | the `Pandoc(fmt)` branch to `render_qmd_to_pandoc` at `render_to_file.rs:357` |
| T3.7 | L | `OutputSink` + the artifact path | Same docx render → assert no `OutputSink` refusal, and that no zero-byte artifact was enqueued for the empty `content` | filesystem via `tempfile`; **real pandoc** | the empty-`content` handling below `render_to_file.rs:366` |

**Revert hunks, stated exactly:**
- T3.1 — Revert ⟨restore the unconditional `anyhow::bail!` at `render.rs:681-684`⟩ → ⟨`assert_eq!(out.status.code(), Some(0))` and the `PK\x03\x04` assertion in `test_e2e_render_docx`⟩ RED.
- T3.2 — Revert ⟨drop the `"pptx"` arm from `TryFrom<&str> for FormatIdentifier`, `format.rs:82-96`, **leaving the relaxed `render.rs:680` gate in place**⟩ → ⟨`test_e2e_render_pptx` reddens with `Unknown format: pptx`⟩. This is the exact failure the naive "relax the gate" change would have shipped.
- T3.3 — Revert ⟨same `TryFrom` arm⟩ → ⟨`assert!(Format::from_format_string("pptx").is_ok())`⟩ RED.
- T3.4 — Revert ⟨the `multi_format_diagnostics` call in `render.rs`⟩ → ⟨`assert!(stderr.contains("Q-18-<n>"))` in `test_e2e_multi_format_warns`⟩ RED. **And** the paired assertion `assert_eq!(status.code(), Some(0))` reddens if the relaxation itself is reverted — see the vacuity check.
- T3.5 — Revert ⟨relax the gate to admit everything, i.e. delete the `bail!` outright⟩ → ⟨`assert_ne!(status.code(), Some(0))` in `test_e2e_pdf_still_refused`⟩ RED.
- T3.6 — Revert ⟨the `Pandoc(fmt)` branch, sending docx back through `render_qmd_to_html`⟩ → ⟨`assert!(out.output_path.extension() == Some("docx")) && assert!(fs::metadata(path)?.len() > 0)` reddens (the HTML writer writes HTML bytes to a `.docx` path, or writes nothing)⟩ RED.
- T3.7 — Revert ⟨treat the empty `content` as an artifact to enqueue⟩ → ⟨`assert!(!out.join("f.docx").metadata()?.len() == 0)`, i.e. the zero-byte-overwrite assertion⟩ RED.

### Refactor-induced vacuity check

- **"`--to docx` is no longer rejected" says nothing about pptx.** This is the round-4 Critical
  finding made concrete: the two formats fail at *different lines* (`render.rs:680` for docx;
  `render.rs:676` → `format.rs:449` for pptx, four lines earlier), so they need **two separately
  bound tests with two different revert hunks** — T3.1/T3.2, and T3.3 isolating the `TryFrom` arm.
  A single parameterized "both formats render" test with one revert hunk would let the pptx
  regression hide behind the docx pass. Verified against the real tree on 2026-09-18:
  `FormatIdentifier` at `format.rs:23-41` has `Html, Pdf, Docx, Epub, Typst, Revealjs, Gfm,
  CommonMark` and **no `Pptx`**.
- **The multi-format warning restores a signal that this task's own change removes, so "a warning
  appears" is the wrong discriminator.** Today `format: {docx:, html:}` fails loudly at
  `render.rs:681` — an accident of the refusal, but a real guardrail (§14). A test written *before*
  the relaxation that asserted "the multi-format case does not silently succeed" would **pass for
  the wrong reason**. T3.4 is therefore a **conjunction**: exit code 0 **and** the `Q-18-<n>`
  warning **and** `multi.docx` non-empty **and** `multi.html` absent. Reverting the relaxation
  reddens the first clause; reverting the warning wiring reddens the second. Neither revert alone
  leaves the test green. **Ordering is load-bearing: Task 1 (warning machinery, `U`-bound only)
  must land before Task 3, and T3.4 must be written as part of Task 3, never Task 1.**
- **T3.1's `PK\x03\x04` check is non-discriminating about content.** A pandoc run with no Lua
  filters at all produces a valid zip. The text assertion (the fixture's body string inside
  `<w:t>`) is what makes it more than a magic-number check; the *semantic* assertions are Task 11's,
  not this task's. Do not strengthen T3.1 toward numbering — it would duplicate Task 11 and drift.
- **T3.6's `content.is_empty()` is also what an unimplemented stub returns.** Pair it with the
  on-disk non-empty assertion (as written), or the row passes against a stage that does nothing.

---

## Task 4: The per-format invocation builder — docx + pptx, the pandoc-defaults allow-list, `FORMAT_PATH_KEYS`, the callout-icon PNGs, and the latex stub

**Scope.** One task for the whole same-shape forwarding family, per the plan's own grouped
checklist item: `--to`, the per-format `pandoc` defaults, the forwarding allow-list, the two new
path-shaped keys, the 5 docx callout-icon params + their PNGs, and latex documented as a stub.

**Files.**
- `crates/quarto-core/src/pandoc_invocation.rs` (new, or wherever P4 Task 9's `PandocWriteStage`
  assembles argv — extend, don't duplicate) — the per-format table. Values from the research doc
  (`2026-07-13-q1-format-typescript.md:81-84`) and P7 Finding 1:
  **docx/odt** `page-width: 6.5`, `default-image-extension: png`;
  **pptx** `output-divs: false` (overrides the HTML-family base), `default-image-extension: png`;
  **latex** stub only — `format: latex` emits `.tex` directly, the extension wins over the inner
  `pdf` recipe, so no latexmk/tectonic step is in scope.
- The **forwarding allow-list** (P7 Finding 2 — allow-list, *not* full `kPandocDefaultsKeys`
  pass-through): `reference-doc`, `template`, `highlight-style`, `toc`, `toc-depth`,
  `reference-location`, `shift-heading-level-by`, and `slide-level` for pptx. Each with its own
  test row.
- `crates/quarto-core/src/project/format_paths.rs:99-105` — **citation verified accurate**:
  `FORMAT_PATH_KEYS` currently holds exactly 5 entries (`css` `ExistenceDiagnose/Entries`, `theme`
  `ExistenceSilent/Theme`, and the three `include-*` `Always/Include`). Add `reference-doc` and
  `template`, resolving relative to the declaring file with a leading `/` meaning project root.
- `claude-notes/designs/path-resolution-model.md` — add both to the consumption-site inventory, per
  the repo rule in CLAUDE.md ("Path resolution is a bug *class*"). A deliberate scope-out needs a
  strand linked to `bd-oejuizi9`.
- `resources/formats/docx/{note,tip,warning,caution,important}.png` (new, in-tree) — vendored from
  `v1.11.3:src/resources/formats/docx/`. **Verified present at the tag**, 5 files, 749-1257 bytes
  each. They are **outside P4's traced `src/resources/filters/` vendoring closure**, so P7 vendors
  them separately. Consumed via the 5 docx callout-icon filter params (`docxCalloutImage` returns
  `nil` when unset, `v1.11.3:src/resources/filters/modules/callouts.lua:84-96` — degradation is
  graceful but **silent**, which is why T4.8 asserts the params are set rather than only that the
  render succeeds).
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
  addition** (`"latex" => Ok(FormatIdentifier::Latex)` in `format.rs:82-96`). Recorded here so
  nobody reads T4.9 as evidence the stub "works".
- **T4.2's discriminator is the *excluded* keys, not the included ones.** P7 Finding 2 chose an
  allow-list specifically to avoid silently forwarding everything Q1's TS type declares. A test
  asserting only that the 8 named keys arrive passes identically under a full pass-through — it
  survives the exact refactor the decision exists to prevent. Hence the three non-allow-listed
  keys in the fixture.
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

**Scope.** Apply the per-format `execute` defaults P7 Finding 1 enumerates, at the one
format-aware seam where the engine's own defaults and the document's `execute:` scope meet. P7
owns this explicitly (round-4 correction: previously stated but disowned, leaving it ownerless).

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
- Values (P7 Finding 1, from `v1.11.3:src/format/formats.ts:315-331` and
  `formats-shared.ts:170-186`): **docx/odt** `execute.fig-width: 5`, `execute.fig-height: 4`;
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
- `crates/quarto-core/src/template.rs:1347` / `:1361` — **citation drift corrected**: P7's text
  cites `template.rs:221` for the nested-`<p>` bug; line 221 today is an unrelated doc comment
  about `$rendered.navigation.toc-relocated$`. The real site is `fn titleblock_field_to_html` at
  **`template.rs:1361`**, called from **`template.rs:1347`**. (The research doc's own anchors,
  `2026-07-09-q1-filter-catalog.md:117` and `:342`, cite `template.rs:220,677` / `:221` + `:679` —
  also drifted.) **Note the split consequence:** the `<p>`-in-`<p>` symptom is HTML-only and does
  not affect docx output, but the *underlying* missing `ensureMetaInlines` coercion does: a
  `PandocBlocks`-valued `title`/`subtitle` reaching Pandoc `Meta` as `MetaBlocks` is not what
  Pandoc's docx writer reads for `title`. **(measured)** with a normal inline title, pandoc 3.8.1
  puts it in `docProps/core.xml` as `<dc:title>My Title</dc:title>` **and** in
  `word/document.xml` as a paragraph with `<w:pStyle w:val="Title"/>`.
- `crates/quarto-core/tests/integration/pandoc_meta_mapping.rs` — new.
- `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs` — extend (Task 3 creates it).

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
(the serialization point), **Task 3 of this plan** (for the `E` row to run at all).

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

## Task 7: B3 shared services wired into the Pandoc tail (staged resources, rewritten links)

**Scope.** Confirm — with tests, not by reading — that `ResourceCollector`'s mediabag/resource
staging and `LinkRewriteTransform` run **before** the wire-format handoff for a `Pandoc(fmt)`
render, so images and relative links resolve in the produced docx/pptx. Both are classified **B3**
(design doc §6): shared post-core services that cross the cut.

**Files.**
- `crates/quarto-core/src/pipeline.rs` — the `Pandoc(fmt)` transform list assembled by P1 Task 2:
  assert `ResourceCollector` and `link-rewrite` are **present**, i.e. *not* on the Pandoc-kind
  exclude list.
- `crates/quarto-core/src/transforms/link_rewrite.rs` — read-only. **P7's checklist item here is
  already resolved and correctly marked `[x]`**: re-verified 2026-09-18, the doc comment at
  `link_rewrite.rs:19-30` documents that `Image::target.0` is rewritten via
  `resolve_static_resource_href`, explicitly "matching Q1" (landed via commit `1d17a9ce7`). The
  transform is reused verbatim; there is no fix in this task, only a binding test.
- `crates/quarto-core/tests/integration/pandoc_b3_services.rs` — new.
- `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs` — extend.

**Acceptance criterion.**

```
$ cargo run --bin q2 -- render img.qmd --to docx     # body: ![cap](sub/pic.png)
$ unzip -l img.docx | grep word/media
     1234  ...  word/media/image1.png
$ unzip -p img.docx word/_rels/document.xml.rels | grep -o 'Target="media/image1.png"'
Target="media/image1.png"
```

and **no** `[WARNING] Could not fetch resource` line on stderr.

**Prerequisite.** **P1 Task 2** (the `Pandoc`-kind transform exclude-list — this task asserts what
that list does *not* contain), **P4 Task 9**, **Task 3 of this plan**.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T7.1 | I | `build_transform_pipeline` for `Pandoc("docx")` | Build → assert the ordered transform-name list **contains** `resource-collector` and `link-rewrite` | none | those two names' presence in the `Pandoc` arm / absence from the exclude list |
| T7.2 | I | `LinkRewriteTransform` on a Pandoc-profile render | Run the pipeline on a fixture with `![cap](sub/pic.png)` and a relative `[text](other.qmd)` → assert the `Image.target` is the resolved staged href and the link target is rewritten | filesystem via `tempfile` | the `Image::target.0` rewrite in `link_rewrite.rs` |
| T7.3 | **E** | the real binary + real pandoc | `q2 render img.qmd --to docx` → `word/media/` contains one entry; `word/_rels/document.xml.rels` has an `image`-typed relationship targeting it; **stderr contains no `Could not fetch resource`** | nothing mocked | the `resource-collector` entry in the `Pandoc` transform list |
| T7.4 | **E** | same | The same fixture where the image path is authored project-root-absolute (`/sub/pic.png`) → same assertions | nothing mocked | the leading-`/` handling in `resolve_static_resource_href` |

**Revert hunks, stated exactly:**
- T7.1 — Revert ⟨add `resource-collector` to the `Pandoc`-kind exclude list⟩ → ⟨`assert!(names.contains(&"resource-collector"))`⟩ RED.
- T7.2 — Revert ⟨the `Image::target.0` rewrite⟩ → ⟨`assert_eq!(img.target.0, expected_staged_href)`⟩ RED.
- T7.3 — Revert ⟨exclude `resource-collector` for the Pandoc profile⟩ → ⟨`assert!(media_entries.len() == 1)` **and** `assert!(!stderr.contains("Could not fetch resource"))` in `test_e2e_docx_image_staged`⟩ RED.
- T7.4 — Revert ⟨the leading-`/`-means-project-root branch⟩ → ⟨the same assertions with the absolute-authored fixture⟩ RED.

### Refactor-induced vacuity check

- **"An image is missing" does not fail a docx render.** **(measured)** pandoc 3.8.1 emits
  `[WARNING] Could not fetch resource nope.png: replacing image with description` and substitutes
  the alt text, **exiting 0**. So an `E` row asserting only "the render succeeded" or "the docx
  exists" is fully non-discriminating about resource staging. The two discriminators are the
  `word/media/` entry count and the absence of that stderr line — both asserted in T7.3. This is
  the same blind spot the extractor's media inventory closes for the goldens (Task 9,
  preserve-item 8).
- **T7.1's presence assertion is not a substitute for T7.3.** A transform can be *in the list* and
  still have no effect for a profile that never reaches its self-gate — the exact failure mode
  behind CLAUDE.md's 2026-04-20 `CodeHighlightStage` incident. T7.3 is the "the path was actually
  exercised" row.
- **T7.4 is not redundant with T7.2.** CLAUDE.md's path-resolution rule is explicit that a fix for
  one key or one form routinely leaves the sibling form broken; the two authored forms are
  separate states.

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
- **Bug B — the silent multi-id crossref drop.** **Citation drift corrected**: P7's text cites
  `crossref_resolve.rs:487`, which is the *test* `fn multi_crossref_cite_resolved_to_first` (whose
  own comment states "We currently resolve to the first; the second is dropped. (Phase 1 scope…)").
  The **production** site is `let first = cite.citations.first()?;` at
  **`crates/quarto-core/src/transforms/crossref_resolve.rs:238`**. Note that the *mixed* case
  (crossrefs intermixed with bibliographic citations) already emits a diagnostic at
  `crossref_resolve.rs:246-254`; the **all-crossrefs** case `[@fig-a; @fig-b]` is the silent one.
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
| T8.1 | U | `crossref_resolve::resolve` | `[@fig-a; @fig-b]`, both crossrefs, both indexed → assert **either** (fix branch) both ids resolve, **or** (flag branch) exactly one resolves **and** a diagnostic naming the dropped id is returned | none | fix branch: the multi-id loop replacing `citations.first()?` at `:238`. Flag branch: the new diagnostic push |
| T8.2 | U | same | The existing test `multi_crossref_cite_resolved_to_first` (`crossref_resolve.rs:486`) is **updated, not deleted**, to state the chosen behavior | none | whichever hunk T8.1 names |
| T8.3 | — | Bug A | `seam deferred until Task 6 of this plan` (T6.2/T6.5 are its binding tests, on the docx-relevant surface); the HTML `<p>`-in-`<p>` half is `accepted-untested` — see the Missing-test pass | — | — |

**Revert hunks, stated exactly:**
- T8.1 (flag branch, the likely one given the epic's "no new functionality" principle) — Revert ⟨the diagnostic push added next to `crossref_resolve.rs:238`⟩ → ⟨`assert_eq!(diags.len(), 1)` in `test_multi_crossref_drop_is_diagnosed`⟩ RED.
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
- **T9.4's discriminator moved once already, in this document.** P7's own extraction spec says
  "relationship entry count + targets"; a count over *all* relationships does not differ between a
  document with a resolved image and one without (both carry the 7+ boilerplate relationships, and
  a missing image is replaced by alt text with **no** relationship). The count only discriminates
  after the `/image` filter. The collapsed raw count is kept **only** as a shape check, never as
  the image-presence discriminator.
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

**The fixture set — 9 fixtures, every path verified present at tag `v1.11.3`** (`git cat-file -e
v1.11.3:<path>`; note P7's own text writes these as `docs/…`, dropping the `tests/` prefix — the
real prefix is `tests/`):

| # | Source (at `v1.11.3`) | Exercises | Engine-free? |
|---|---|---|---|
| 1 | `tests/docs/crossrefs/all-docx.qmd` (+ `tests/docs/crossrefs/img/thinker.jpg`) | numbered figure + captioned table + a theorem; Q1's own docx-targeted crossref doc | yes |
| 2 | `tests/docs/callouts.qmd` | all 5 callout types, captioned/uncaptioned, `collapse`, three `appearance` values, `icon="false"` — the docx callout-icon path | yes |
| 3 | `tests/docs/crossrefs/callouts.qmd` | **cross-referenceable** callouts — the Route-R Callout `order` reclassification (design §3) | yes |
| 4 | `tests/docs/crossrefs/theorems.qmd` | `::: {#thm-line}` + an unlabeled `$$` | yes |
| 5 | `tests/docs/crossrefs/theorem-types.qmd` | one div per theorem type (`lem/cor/prp/cnj/def/exm/exr`) — and **not** `alg`, which is Task 12's point | yes |
| 6 | `tests/docs/crossrefs/equations.qmd` | one labeled `$$ {#eq-black-scholes}` + an `@eq-` ref — the `<m:oMath>` path Reviewer B added | yes |
| 7 | `tests/docs/smoke-all/crossrefs/theorem/proof-rendering.qmd` | `.proof`, `.proof name=…`, empty `.proof`, `.remark` — P5's Proof Route-R shape | yes |
| 8 | `tests/docs/smoke-all/2025/01/08/7260.qmd` | the smallest clean `.panel-tabset` (Tab A / Tab B `{.active}`) — the Tabset Route-R reclassification | yes |
| 9 | `tests/docs/smoke-all/mermaid/backticks.qmd` | **the one labeled accepted-divergence fixture** (round 4) — see Task 11 | no engine cells, but a `mermaid` cell |

Plus **one fixture we author ourselves**, because no quarto-cli fixture covers it: a **Tabset
containing a FloatRefTarget subfloat** (P6 Finding 5) →
`crates/quarto-core/tests/fixtures/pandoc-goldens/tabset-subfloat.qmd`.

**The engine-free rule:** capture renders with a real `quarto`, so any fixture with an
`{r}`/`{python}`/`{julia}` cell needs that toolchain — excluded for v1. All 9 sourced fixtures were
checked and carry zero such cells. (Fixture 9's `mermaid` cell is a mermaid-engine cell, not
R/Python/Julia; see Task 11 for what its snapshot is expected to show.)

**Exclusions kept as-is** (P7 Finding 4, as corrected): the code-block `filename`-header fixtures
and the `crossref:`-presentation-option fixtures stay **out**, because those genuinely are
structural blind spots and accepted gaps (design §12). **mermaid is not excluded** — the round-4
correction found its exclusion circular.

**Acceptance criterion.**
1. `cargo xtask capture-pandoc-goldens` with no real `quarto` on `PATH` **fails loudly**, naming
   the binary and the expected release tag — it must not skip, and must not write an empty snapshot.
2. With a real pinned-release `quarto`, it writes one `.snap` per fixture/format pair (20 files:
   10 fixtures × docx + pptx) at the `insta::Settings`-declared path, and re-running it produces
   **zero** `git diff`.
3. The written filenames match exactly what Task 11's test looks up — verified by running Task 11's
   test immediately after and observing **no** "snapshot not found, created new" line. (P7 Finding
   4 names this as the silent-pass hazard.)
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
- T10.4 — Revert ⟨drop `img/thinker.jpg` from the copy-in⟩ → ⟨`assert!(resource.exists())` in `test_fixture_manifest_complete`⟩ RED. The failure this guards is an unresolved image that *still renders*, per T7.3's measured note.
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

**Scope.** The epic's Definition-of-done conditional, whose ownership was fixed to P7 in review
I10 (P5 disowns the implementation twice; P7 owns the *evaluation* because it owns the fixture set).
Evaluate the condition, then either land the `THEOREM_CLASSES`/`RefTypeRegistry::BUILTINS` fix or
file the follow-on strand — and leave an artifact either way.

**Files.**
- `crates/quarto-core/src/transforms/theorem.rs:61-70` — `THEOREM_CLASSES`, **verified**: 8 entries
  (`theorem/thm/Theorem`, `lemma`, `corollary`, `proposition`, `conjecture`, `definition`,
  `example`, `exercise`). **No `algorithm`/`alg`.**
- `crates/quarto-core/src/crossref/registry.rs:78-106` — `BUILTINS`, **verified**: 21 entries
  (`fig, tbl, lst, eq, sec, thm, lem, cor, prp, cnj, def, exm, exr, sol, rem, nte, wrn, tip, imp,
  cau, demo`). **No `alg`.** Both halves of the gap are real.
- `crates/quarto-core/tests/fixtures/pandoc-goldens/README.md` — the recorded artifact (see below).

**The evaluation criterion — and a terminology correction that changes its result.** The epic, P5
and P7 all phrase this as "the `algorithm` **theorem class**". Verified: **there is no
`.algorithm` class in Quarto.** The algorithm theorem type is selected by the **div id prefix
`#alg-`**, exactly like `#thm-`; the string `algorithm` appears only as the LaTeX/typst
*environment* name in Q1's `v1.11.3:src/resources/filters/customnodes/theorem.lua:45-49`
(`alg = { env = "algorithm", style = "plain", title = "Algorithm" }`). A grep for `.algorithm`
alone returns **zero** hits and would wrongly conclude "no fixture uses it, for the wrong reason."

**So the criterion is:** grep the copied fixture set (and the 9 sourced paths at `v1.11.3`) for
`#alg-`, `@alg-`, `@Alg-` — **not** `.algorithm`.

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
| 1 | **No mermaid rendering** (source text appears instead) | **Labeled golden** — fixture 9, T11.6, `DIVERGENCES.md` naming `bd-h1ub8f8z`. The self-gate `mermaid.rs:175` (`if !ctx.format.identifier.is_html_based() { return Ok(()) }` — **verified**) additionally gets a bound unit row: `accepted-untested` is **not** used here. Add **T11.7 (`U`)**: assert `MermaidTransform::transform` is a no-op for a docx `RenderContext` — Revert ⟨the `is_html_based()` self-gate at `mermaid.rs:175`⟩ → ⟨`assert_eq!(ast_before, ast_after)`⟩ RED. |
| 2 | **No section numbers** (`number-sections` / `@sec-`, `bd-5aklrxgi`) | **Bound negative seam.** Add **T4.11 (`U`)**: assert the pandoc-defaults allow-list does **not** contain `number-sections` or `number-offset` — this is design §11's frozen decision (forwarding them would produce a third, unaudited behavior; Q1 itself *deletes* them, `v1.11.3:src/command/render/pandoc.ts:1056-1057`, guarded at `:1048-1052`, with the comment at `:1044-1047`; **note P7/design cite `1049-1062`, the actual span is `1048-1062`**). Revert ⟨add `number-sections` to the allow-list⟩ → ⟨`assert!(!allow_list.contains("number-sections"))`⟩ RED. **Verified (measured):** docx is neither latex, typst, nor markdown output, so docx **does** take Q1's delete branch — Q1's Lua owns numbering for docx, which is precisely why forwarding is wrong. |
| 3 | **No code-block `filename` headers** (upstream, quarto-cli#14906) | `accepted-untested: the loss happens inside Q1's own vendored `decoratedcodeblock.lua`, which has renderers for html/markdown/latex only; there is no Q2-side hunk whose revert could redden a test, and design §11 resolved this as an accepted upstream gap explicitly out of epic scope. The `filename` fixtures are deliberately excluded from the golden set (Task 10) so no snapshot silently encodes the loss as a Q2 baseline.` |
| 4 | **Listing pages render prose-only** | `accepted-untested: `listing-generate`/`listing-render` are excluded for the Pandoc leg by **P1 Task 2**'s exclude list, which P1 tests directly; a P7 test would duplicate P1's binding and drift from it. T7.1's "contains" assertion covers the inverse direction (what must *not* be excluded).` |
| 5 | **No project-mode rendering** | **Bound** — Task 2 in full (T2.1-T2.6), with T2.2/T2.5 as the "path was actually exercised" rows. |
| 6 | **One format per invocation, now with a warning** | **Bound** — Task 1 (T1.1-T1.3, `U`) + T3.4 (`E`, the conjunction). |
| 7 | **`Post`-position user Lua filters can't see custom-node content** (`bd-o90yz5mg`) | **Bound structural seam.** Add **T3.8 (`I`)**: for a `Pandoc("docx")` render, assert `UserFiltersStage::post()` observes an AST that still contains `CustomNode` wrappers, and that the **`Pre`** position observes none (verified in round-4 review: `pre()` runs before any sugar transform). Revert ⟨reorder `UserFiltersStage::post()` after the wire-format cut⟩ → ⟨the `Pre`-sees-no-CustomNode assertion⟩ RED. This pins the *documented* shape so a future reorder is a reviewed change, not a surprise. |
| 8 | **An explicit `crossref:` override is honored on Pandoc and silently ignored on HTML** (`bd-wqdi1pd2`) | `accepted-untested: verified zero Q2 readers of `crossref.title-delim`/`fig-prefix`/`ref-hyperlink` anywhere in `crates/` — only `crossref_render.rs:29`'s own comment admits the gap (**re-verified 2026-09-18**: `crossref_render.rs:26-31` hard-codes `"<Kind> <N>: "`). A test asserting the asymmetry would pin a *bug* the epic explicitly declines to fix, and would redden the day `bd-wqdi1pd2` lands. The `crossref:`-presentation fixtures are excluded from the golden set for the same reason.` |
| 9 | **A callout whose crossref category Q2 knows and Q1 doesn't renders unnumbered, with a dangling ref** | `accepted-untested in P7: the `fail()`-fallback, its `by_ref_type` guard and its warning are **P5 companion Task 6**'s hunk and tests (design §12's last bullet, P6 Finding 2). P7's fixture 3 (`tests/docs/crossrefs/callouts.qmd`) exercises only Q1-known categories, so no P7 golden encodes the fallback. If P5 Task 6's tests are ever dropped, nothing in P7 catches it — recorded so that is a known, not a discovered, gap.` |

### Other load-bearing branches

| Behavior | Verdict |
|---|---|
| **The `algorithm`/`THEOREM_CLASSES` condition** | **Bound** — Task 12 in full, with T12.2 as the expiry mechanism and a committed README artifact (not a memory). |
| **`--reference-doc` — file missing** | **Bound** — T4.7 (`E`), asserting the Q2-side span-bearing diagnostic, which fires *before* pandoc. **(measured)** without it pandoc 3.8.1 exits 99 with the bare line `File X not found in resource path`. |
| **`--reference-doc` — file present but malformed** | `accepted-untested: outside `MarkPolicy::ExistenceDiagnose`'s reach (existence, not content), and P7's plan does not name a content check. **(measured)** pandoc 3.8.1 exits **1** with a GHC `CallStack` backtrace (`Data.Binary.Get.runGet at position 4: Did not find end of central directory signature`), which P4 Task 10's verbatim nonzero-exit passthrough would print to the user as-is. Surfaced as **Findings F6** rather than fixed here, because turning it into a `Q-18-*` would be a new content-validity check nobody has scoped.` |
| **`pandoc` absent, at the CLI level** | **Bound, by reference.** P4 Task 7 owns the check and its `Q-18-*` code. P7 adds **T3.9 (`E`)**: run `q2 render f.qmd --to docx` with a `PATH` containing no `pandoc` → non-zero exit, stderr contains the `Q-18-*` code and the word `pandoc`, and **no** partial `.docx` is left on disk. Revert ⟨P4's pre-flight check, letting the `Command::new("pandoc")` spawn fail⟩ → ⟨`assert!(stderr.contains("Q-18-"))` and `assert!(!out.join("f.docx").exists())`⟩ RED. This is the row that answers "does the CLI surface it *well*", which P4's own tier cannot: P4's `L`-tier tests *require* pandoc. |
| **`pandoc` present but below the floor** | `accepted-untested in P7: P4 Task 7 owns the version comparison and its gate, with its own tests. A P7 `E` row would need a stub `pandoc` on `PATH` reporting a low version, which is a second copy of P4's harness; the CLI-surfacing shape is already covered by T3.9's code-and-message assertion, which shares the diagnostic path.` |
| **Binary output — the contract beyond "a file exists"** | **Bound, three ways, because "a file exists" is exactly the failure mode.** (a) T3.1/T3.2 assert the zip magic `PK\x03\x04` and a required internal entry (`word/document.xml` / `ppt/slides/slide1.xml`) — this is what rules out HTML-in-a-`.docx`. (b) **T3.10 (`E`, new)**: assert the output file's length is **> 0** and that `Content-Type`-shaped sniffing does not match text — concretely, `assert_ne!(&bytes[..2], b"<!")` and `assert!(bytes.len() > 1000)`, guarding the zero-byte and HTML-written-to-a-binary-path cases. Revert ⟨P4's `content: String::new()` decision into "write `RenderedOutput.content` to the output path"⟩ → ⟨T3.10's length assertion reddens with a 0-byte file⟩ RED. (c) T9.9 makes the extractor reject a non-OOXML input with `Err`, so a corrupt output cannot pass Task 11 as an empty extraction. |
| **The HTML-leg nested-`<p>` symptom** | `accepted-untested: HTML-only, with no docx/pptx surface; the docx-relevant half (the `MetaBlocks`→`MetaInlines` coercion) **is** bound, at T6.2/T6.5. Flagged, not fixed, per Task 8; needs a strand if one does not already exist.` |
| **The `latex` stub** | **Bound negatively** — T4.9 asserts `--to latex` still refuses. See Task 4's vacuity check for what it deliberately does not assert. |
| **The `G` tier's own skip** | **Bound** — T10.1/T10.2 make a missing or wrong-version `quarto` a loud failure, never a skip; T10.7 keeps the capture out of `verify`/CI. |
| **`cargo nextest run --workspace` + `cargo xtask verify`** | **Bound as a task-exit gate, not a test.** Per the user-level instruction, each task gates on `cargo clippy -p <crate> --all-targets -- -D warnings` + `cargo nextest run -p <crate>`; the **workspace** run happens once per phase boundary and before any push, reported as a delta against the live baseline. `RenderContext` gains a field in **P1 Task 1**, so P7's phase-boundary verify must be **full** `cargo xtask verify` (not `--skip-hub-build`) — `quarto-core` is in `wasm-quarto-hub-client`'s closure. |

---

## Findings for Gordon

Measured contradictions and drift. **None of these reopens a frozen design decision**; each is
either a citation correction, a fact that changes how a task must be written, or a scope question I
declined to decide.

**F1 — The local quarto-cli checkout is `v1.11.5-1-g83d48d8e8`, not the pinned `v1.11.3`.**
`git -C /Users/gordon/src/quarto-cli describe --tags`. P4's companion already adopted the
workaround (read the tag via `git show v1.11.3:<path>`); recorded here because P7's Task 10 *renders
with a real `quarto` binary*, and a binary built from the worktree would be v1.11.5. The release
binary for the pinned tag is what Task 10 must locate — as the plan already says. **All 17
quarto-cli paths this document names were verified present at tag `v1.11.3`.** No decision needed.

**F2 — `.algorithm` is not a Quarto class; the algorithm theorem type is the `#alg-` id prefix.**
The epic DoD, P5 and P7 all phrase the gap as "the `algorithm` theorem class". Verified: Q1
registers `alg = { env = "algorithm", … }` in
`v1.11.3:src/resources/filters/customnodes/theorem.lua:45-49`, Q1's only algorithm fixture uses
`::: {#alg-gcd}`, and `alg-cap` does not exist anywhere in quarto-cli. A `.algorithm` grep — the
literal reading of the DoD item — returns zero hits **for the wrong reason**. Both Q2 gap sites are
real and confirmed (`theorem.rs:61-70` has no `algorithm` row; `registry.rs:78-106` has no `alg`
row). Task 12 uses the corrected predicate and pre-computes the expected outcome (condition does
not fire → file the strand). **No decision needed; the plan text is worth correcting when next
touched.**

**F3 — Four citation drifts in P7's own text, and five citations re-verified as accurate.**
Drifted: (a) the nested-`<p>` bug's `template.rs:221` → the real site is `titleblock_field_to_html`
at `template.rs:1361`, caller `:1347` (line 221 today is an unrelated TOC doc comment; the research
doc's `template.rs:220,677`/`:221`+`:679` have drifted too). (b) the multi-id crossref drop's
`crossref_resolve.rs:487` → that is the *test* `multi_crossref_cite_resolved_to_first`; the
production site is `cite.citations.first()?` at `crossref_resolve.rs:238`. (c)
`format.rs:447` → the `Err("Unknown format: …")` is at `format.rs:449`. (d) the fixture paths are
written `docs/crossrefs/all-docx.qmd` — the real prefix is `tests/`. Also `pandoc.ts:1049-1062` →
the actual span is `1048-1062` (deletes at `1056-1057`). **Verified accurate today:**
`render.rs:680-684`, `format.rs:58-60` (`is_native`), `format_paths.rs:99-105` (5 entries),
`orchestrator.rs:517-580` (`post_render`), `pipeline.rs:322` (`engine_stage`), `mermaid.rs:175`,
`crossref_render.rs:28-31`, `Cargo.toml:37` (`quick-xml = "0.39"`, no `zip`). Surfaced rather than
silently propagated or silently fixed, per the grounding rules.

**F4 — `WebsiteProjectType::post_render` takes no `format` argument, and `post_resources` is a
second unconditional HTML-assuming hook that §13's wording does not cover.** The §13 gate as worded
("gate `WebsiteProjectType::post_render`'s hook sequence on `format.identifier.is_html_based()`")
needs either a trait-signature change (`orchestrator.rs:397-406` + the test double at
`crates/quarto-core/tests/integration/project_pipeline.rs:227`) or a call-site gate
(`orchestrator.rs:1220-1231`, where `self.format` is already in scope and used at `:1122`). **Task 2
picks the call site** — it is the "one-line fix" §13 describes — and says so, because the choice
determines which revert hunk exists. The part I am **not** deciding: `async fn post_resources`
(immediately after `post_render`, writing llms.txt + per-page markdown companions) is *also*
unconditional and *also* assumes HTML outputs exist. §13 names only `post_render`. Whether the
containment gate should cover `post_resources` too is a scope question for you, not something I
should widen Task 2 to absorb.

**F5 — `is_html_based()` and `is_native()` are behaviorally indistinguishable today.** Both are
`matches!(self, Html | Revealjs)` (`format.rs:58-60` and `:63-65`), and P1 Task 1 adds `Pptx` to
neither. So no outcome-level test can tell whether the §13 gate was written against the right
predicate, and Task 3's relaxation is exactly the change that makes `is_native()`'s meaning
("renders in-process") stop matching "is a supported format". Task 2 pins the predicate's own table
(T2.6) and Task 2's vacuity check records the hazard. No code change requested — recorded so that
if the two predicates are ever deliberately diverged, the reviewer knows which tests depend on
which.

**F6 — A malformed `--reference-doc` surfaces a GHC backtrace to the user.** **(measured, pandoc
3.8.1)** a non-zip file passed as `--reference-doc` exits **1** with
`pandoc: Data.Binary.Get.runGet at position 4: Did not find end of central directory signature`
plus a `CallStack`/`HasCallStack` trace. Under P4 Task 10's verbatim nonzero-exit passthrough, that
is what the user reads. The *missing*-file case is clean (Task 4 diagnoses it with a span before
pandoc runs, via `MarkPolicy::ExistenceDiagnose`), but the malformed case is outside that policy's
reach and P7's plan does not name it. Logged as `accepted-untested` in the Missing-test pass and
surfaced here because a content-validity check (e.g. a `PK\x03\x04` pre-flight on `reference-doc`)
would be new functionality nobody has scoped — your call, not mine.

**F7 — the P5/P6 harness-ordering question resolved itself while this file was being written, and
the residual coupling is now small but unstated.** P5 (`P5-lua-shim.md:655-659`) and P6
(`P6-*.md:272-278`) each recorded the same "(a) build a narrower one-off harness now, or (b)
explicitly schedule this item after P7" choice, with P6 adding "Don't leave it silently
unschedulable." **Both sibling companions picked (a)** — P5 companion **Task 8** asserts on the
post-filter Pandoc AST, P6 companion **Task 5** is titled "schedulable without P7". So P7 inherits
no deferred predecessor items, and I removed the fixture-superset obligation I was going to raise.

What remains is one unstated overlap worth a sentence in P7's "Produces" seam list if you want it
explicit: **P6 Task 5 and P7 T11.3 both assert number identity, at different depths** — P6 at the
post-filter AST, P7 at the rendered OOXML. They are not duplicates (the `<m:oMath>` case is
correct-in-AST-but-lost-in-writer, which only P7 can see), but nothing says so, so a future reader
could reasonably delete one as redundant. I have not added it.

**F8 — One measured fact that makes P7's own extraction spec sharper, folded into this document
rather than left as a surprise.** P7's extraction spec says "record the relationship/media
inventory (`word/_rels/document.xml.rels` entry count + targets)". **(measured)** an image-free docx
already carries **7+** relationships (numbering, styles, settings, theme, fontTable, webSettings,
footnotes), so a raw entry count neither indicates image presence nor is stable across pandoc's
default-reference-doc changes; and `docProps/core.xml` carries wall-clock
`dcterms:created`/`dcterms:modified`, which would make every snapshot fail on every run. Task 9's
contract therefore filters relationships to `Type` ending `/image` and excludes the two timestamp
fields. I read this as a precision on P7's own wording rather than a scope change, but it is the
kind of refinement worth seeing.

**Everything else checked out.** The extractor's central tension — suppress Pandoc-version noise
while preserving the discriminator — resolves cleanly and was measured, not reasoned: pandoc 3.8.1
emits U+00A0 verbatim inside `<w:t>` and `<a:t>` (hexdumped: `4669 6775 7265 c2a0 31`), so the
extractor can preserve crossref numbers, prefixes and the nbsp between them while normalizing
attribute order, run splitting, generated ids, and zip metadata. **The one thing it must not do is
normalize whitespace**, and that is the single instruction most likely to be "cleaned up" by a
future contributor — which is why T9.1 is a byte-literal assertion and why T11.3 compares
`as_bytes()`.
