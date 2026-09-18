# P7-foundation — Implementation tasks & Test Seam Spec

**Plan (authoritative scope):** [`2026-09-20-pandoc-hybrid-P7-foundation.md`](2026-09-20-pandoc-hybrid-P7-foundation.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§12 known limitations, §13 project-mode gate, §14 multi-format guardrail)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Sibling companion:** [`2026-09-18-pandoc-hybrid-P7-implementation.md`](2026-09-18-pandoc-hybrid-P7-implementation.md) — this file's Tasks 1-4 correspond to that document's Tasks 1, 2, 3, and 7 (Task 7 renumbered Task 4 here, with its row ids renumbered `T7.x` → `T4.x`). That document's remaining tasks keep their original numbers 4-6, 8-12.
**Depends on:** P1, P2, P4.
**Status:** Ready for subagent-driven execution.

This file converts the plan's Coarse checklist into `## Task N` units `superpowers:subagent-driven-development` can dispatch, binding every test to a named production seam and revert hunk (the `/prevalidating-test-seams` discipline). The Spec is the plan + the design doc; where this file and the plan disagree, the plan wins.

**Reading the vendored Q1 source.** The local `quarto-cli` checkout tracks `main` and will diverge from the pinned tag `v1.11.3`. Always read pinned content via `git show v1.11.3:<path>`, never from the worktree directly. Behaviour claims marked **(measured)** were reproduced by running pandoc 3.8.1 locally.

---

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **`U`** | Rust unit test | `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **`I`** | Rust integration test, in-process, no external binary | `crates/<crate>/tests/integration/<name>.rs`, registered `pub mod <name>;` in that crate's `tests/integration/main.rs` | `cargo nextest run -p <crate>` |
| **`E`** | **End-to-end CLI test** — spawns the real `q2` binary and inspects the produced file | `crates/quarto/tests/integration/<name>.rs`, using `const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");` (the established idiom — `render_cli_e2e.rs:28`, `attribution_cli_e2e.rs:43`, and 5 more) | `cargo nextest run -p quarto` |
| **`L`** | Lua/pandoc integration test — a **real** `pandoc` subprocess below the CLI level | same layout as `I`; uses P4 Task 2's `run_main_lua` | `cargo nextest run -p quarto-core` |
| **`X`** | `cargo xtask` lint / verify gate | `crates/xtask/src/lint/<rule>.rs`, or a repo-level `check(workspace_root)` | `cargo xtask lint` / `cargo xtask verify` |

**Per `.claude/rules/integration-tests.md`, never add a top-level `crates/<crate>/tests/<name>.rs`.**
One `integration` binary per crate. New files are appended alphabetically.

### `E`- and `L`-tier gate policy

P7-foundation adopts P4's gate policy: **a hard gate, never a skip.**

1. A real `pandoc` is already a hard dependency of `cargo nextest run --workspace` today — pampa's
   four oracle tests call `assert_good_pandoc_version()`
   (`crates/pampa/tests/integration/test.rs:161`), which `.expect()`-panics when pandoc is absent.
   `E`/`L` tiers here introduce no new environment requirement and must panic, not skip, when
   pandoc is missing or below P4 Task 7's floor.
2. No `QUARTO_TEST_PANDOC=1`-style opt-in. An opt-in gate is a skip by default.
3. **`.snap` accounting.** Per CLAUDE.md's "Snapshot Test Changes" rule, any task below that adds
   or updates `.snap` files must report the count and summarize what changed. (None of the four
   tasks here add `.snap` files — that machinery is P7's.)

**Row tally across the 4 tasks in this file:** 26 bound rows — `U=6`, `I=4`, `E=10`, `L=4`, `X=2`.
(P7's companion totals 47 bound rows across its 8 remaining tasks — `U=31`, `I=4`, `E=5`, `L=4`,
`G=1`, `X=2` — plus 3 deferred seams, T8.3/T12.3/T12.4; combined 73 bound rows across both
documents.) The `I`/`L` split is by *environment*, not file location: a row that reaches
`PandocWriteStage` shells out to a real `pandoc` and is therefore `L`, even though it lives in an
`integration` binary and is invoked in-process.

---

## Task 1: The multi-format render warning — pure diagnostic + its `Q-18-*` code, page and sidebar entry

**Scope.** A pure function that, given a document's `format:` declaration and the single key that
was actually used, returns a `Q-18-*` warning naming the used key and the skipped keys. Lands
**before** Task 3's relaxation, so the warning exists the moment the guardrail it replaces is
removed (design doc §14). No rendering capability is added — this is one diagnostic.

**Files.**
- `crates/quarto-core/src/format.rs` — new `pub fn multi_format_diagnostics(...) -> Vec<DiagnosticMessage>`,
  next to the two places that silently reduce a multi-key `format:` to one:
  `format_key_from_frontmatter` at `format.rs:270-278` (`serde_yaml::Value::Mapping(m) =>
  m.keys().find_map(...)` — the CLI path) and `format_key_from_config_value` at `format.rs:297-304`
  (`entries.first()` — the project/pipeline path). Model the new function on
  `project_kind_diagnostics` (`crates/quarto-core/src/project/mod.rs:351-379`), which is the house
  shape: a pure function returning `Vec<DiagnosticMessage>`, built with
  `DiagnosticMessageBuilder::warning(...) .with_code("Q-5-18").problem(...)` (`mod.rs:365-378`).
- `crates/quarto-error-catalog/error_catalog.json` — one new `Q-18-*` entry in the `pandoc`
  subsystem P4 Task 6 reserves (subsystem number 18). This task *extends* that code set; it does
  not reserve a subsystem. `Q-18-1` through `Q-18-4` are already in use (P4 Task 6); this task
  claims **`Q-18-5`**. P5's companion Task 6 separately claims `Q-18-6` and `Q-18-7` for its own
  two codes in the same catalog file and sidebar block — re-check the catalog for the current
  next-free number before landing either, since these numbers are a coordination default, not an
  enforced reservation.
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

## Task 2: The project-mode containment gate (design doc §13)

**Scope.** Gate `WebsiteProjectType::post_render`'s hook sequence on
`format.identifier.is_html_based()`, so a website/book/manuscript project rendered to a Pandoc
target does not write a sitemap of `.html` URLs that do not exist, and does not hard-fail in
`write_alias_redirects` with an HTML-specific diagnostic. Matches Q1's own
`websiteProjectType.postRender`, which already filters `outputFiles` to HTML-only. Lands before
Task 3, which is what makes the risk reachable.

**Files.**
- `crates/quarto-core/src/project/orchestrator.rs` — **the gate goes at the call site**,
  `orchestrator.rs:1220-1231` (`self.project_type.post_render(...)`), because `self.format` is
  already in scope there and used earlier at `:1121` (`self.format.identifier.as_str()`).
  **`async fn post_render` at `orchestrator.rs:517-580` takes no `format` argument** (params:
  `project, index, output_paths, project_artifacts, resolver, runtime, diagnostics`) — so the
  alternative shape, threading a `format: &Format` into the trait method, also touches the trait
  definition at `orchestrator.rs:397-406` and the test double at
  `crates/quarto-core/tests/integration/project_pipeline.rs:227`. The call-site gate is chosen
  here; it is the "one-line fix" §13 and `bd-bgeet2mw` both describe.
- `crates/quarto-core/src/format.rs:66-68` — `is_html_based()`, read-only. **Do not widen it.**
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
| T2.6 | U | `FormatIdentifier::is_html_based` | Assert `Docx`/`Pptx`/`Pdf`/`Gfm` → `false`; `Html`/`Revealjs` → `true` | none | the `matches!` arm at `format.rs:67` |

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
  that exists today** (`format.rs:61-63` vs `:66-68`, both `matches!(Html | Revealjs)`), and P1
  Task 1 adds `Pptx` to neither. So a refactor that routes this gate through `is_native()` by
  mistake is **behaviorally invisible** to any test that only checks outcomes. T2.6 pins the
  predicate's own table, and the gate must be written against `is_html_based()` because that is the
  one whose *meaning* is "HTML family" — `is_native()`'s meaning ("renders in-process") is exactly
  what Task 3 stops being true of a supported format.
- **T2.3's expected value is `Ok`, which is also what a no-op render returns.** Pair it with the
  same non-empty-output assertion as T2.1.

---

## Task 3: Relax the format gate — admit **docx and pptx**, route through `render_qmd_to_pandoc`, wire the warning in

**Scope.** Replace `render.rs`'s blanket non-native refusal with one that admits docx and pptx and
routes them to P4's Pandoc entry point, and emit Task 1's warning at the same site. Verified
end-to-end for **both** formats.

**Files.**
- `crates/quarto/src/commands/render.rs:680-684` — line 680 is `if !format.identifier.is_native() {`, 681-684 the
  `anyhow::bail!("Format '{}' is not yet supported. Only HTML and revealjs are available in this
  version.", …)`. Relax to admit `Docx | Pptx` (and keep refusing `Pdf | Epub | Typst | Gfm |
  CommonMark`).
- `crates/quarto/src/commands/render.rs:676` — `let format = resolve_format(&format_str)?;`,
  which delegates to `Format::from_format_string`. `FormatIdentifier::Pptx` exists
  (`format.rs:31`) and its `TryFrom<&str>` arm returns `Ok` (`format.rs:92`, inside the
  **non-exhaustive** match at `format.rs:84-101`), so `Format::from_format_string("pptx")` is
  already `Ok`.
- `crates/quarto/src/commands/render.rs` — call `multi_format_diagnostics` next to
  `detect_single_input_format` (`render.rs:673-676`), which is the site that reduces a multi-key
  `format:` to one via `format_key_from_frontmatter` (`format.rs:270` — see Task 1's Files section).
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
warning). This task's own assertions are structural; P5's shim and P6's numbering wiring are what
need to land for the rendered *content* to be correct, but this task does not wait on them, and
neither does P7's docx/pptx-specific work depend on this task alone.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | **E** | the real `q2` binary, full `execute()` chain through clap | `q2 render f.qmd --to docx` → exit 0; `f.docx` first 4 bytes `PK\x03\x04`; `word/document.xml` present; its concatenated `<w:t>` text contains the fixture's body string | nothing mocked | the `Docx` arm of the relaxed gate at `render.rs:680` |
| T3.2 | **E** | same | `q2 render f.qmd --to pptx` → exit 0; `f.pptx` is a zip containing `ppt/slides/slide1.xml`; its `<a:t>` text contains the body string | nothing mocked | `FormatIdentifier::Pptx`'s arm in `TryFrom<&str>` (`format.rs:84-101`) **and** the `Pptx` arm of the relaxed gate — two separate hunks, T3.3 separates them |
| T3.3 | U | `Format::from_format_string` | `from_format_string("pptx")` → `Ok`, `identifier == Pptx`, `output_extension == "pptx"`, `native_pipeline == false`; and `from_format_string("docx")` → `Ok` | none | the `"pptx" => Ok(FormatIdentifier::Pptx)` arm at `format.rs:92` (within the match at `:84-101`) |
| T3.4 | **E** | the real binary + the warning wiring | `q2 render multi.qmd` (front matter `format: {docx: default, html: default}`) → **exit 0**, stderr contains `Q-18-<n>` and both `docx` and `html`, `multi.docx` exists and is non-empty, `multi.html` does **not** exist | nothing mocked | the `multi_format_diagnostics` call in `render.rs` |
| T3.5 | **E** | same | `q2 render f.qmd --to pdf` → non-zero exit, stderr contains `is not yet supported` | nothing mocked | the `Pdf` arm still refused by the relaxed gate |
| T3.6 | L | `render_document_to_file`'s profile branch | Call with a docx `Format` → assert the returned `RenderedOutput.content.is_empty()` and `output_path` ends `.docx`, and the file on disk is non-empty | filesystem via `tempfile`; **real pandoc** | the `Pandoc(fmt)` branch to `render_qmd_to_pandoc` at `render_to_file.rs:357` |
| T3.7 | L | `OutputSink` + the artifact path | Same docx render → assert no `OutputSink` refusal, and that no zero-byte artifact was enqueued for the empty `content` | filesystem via `tempfile`; **real pandoc** | the empty-`content` handling below `render_to_file.rs:366` |
| T3.8 | I | `UserFiltersStage`'s `pre()`/`post()` positions on a `Pandoc(fmt)` render | For a `Pandoc("docx")` render, assert `post()` observes an AST that still contains `CustomNode` wrappers, and that `pre()` observes none | none | reordering `UserFiltersStage::post()` to run after the wire-format cut |
| T3.9 | **E** | the real binary, `pandoc` missing | `q2 render f.qmd --to docx` with a `PATH` containing no `pandoc` → non-zero exit, stderr contains P4's `Q-18-*` code and the word `pandoc`, and **no** partial `.docx` is left on disk | `PATH` via env, in-process; nothing else mocked | P4's pre-flight pandoc-presence check (letting `Command::new("pandoc")`'s spawn fail bare would still exit non-zero, but without the code/message pairing) |
| T3.10 | **E** | the real binary, binary-output contract | For a successful `--to docx` render, assert the output file's length is **> 0** and does not start with the two bytes `<!` (ruling out HTML written to a `.docx` path) | nothing mocked | P4's `content: String::new()` decision, reverted to "write `RenderedOutput.content` to the output path" |

**Revert hunks, stated exactly:**
- T3.1 — Revert ⟨restore the unconditional `anyhow::bail!` at `render.rs:681-684`⟩ → ⟨`assert_eq!(out.status.code(), Some(0))` and the `PK\x03\x04` assertion in `test_e2e_render_docx`⟩ RED.
- T3.2 — Revert ⟨drop the `"pptx"` arm from `TryFrom<&str> for FormatIdentifier`, `format.rs:84-101`, **leaving the relaxed `render.rs:680` gate in place**⟩ → ⟨`test_e2e_render_pptx` reddens with `Unknown format: pptx`⟩. This is the exact failure the naive "relax the gate" change would have shipped.
- T3.3 — Revert ⟨same `TryFrom` arm⟩ → ⟨`assert!(Format::from_format_string("pptx").is_ok())`⟩ RED.
- T3.4 — Revert ⟨the `multi_format_diagnostics` call in `render.rs`⟩ → ⟨`assert!(stderr.contains("Q-18-<n>"))` in `test_e2e_multi_format_warns`⟩ RED. **And** the paired assertion `assert_eq!(status.code(), Some(0))` reddens if the relaxation itself is reverted — see the vacuity check.
- T3.5 — Revert ⟨relax the gate to admit everything, i.e. delete the `bail!` outright⟩ → ⟨`assert_ne!(status.code(), Some(0))` in `test_e2e_pdf_still_refused`⟩ RED.
- T3.6 — Revert ⟨the `Pandoc(fmt)` branch, sending docx back through `render_qmd_to_html`⟩ → ⟨`assert!(out.output_path.extension() == Some("docx")) && assert!(fs::metadata(path)?.len() > 0)` reddens (the HTML writer writes HTML bytes to a `.docx` path, or writes nothing)⟩ RED.
- T3.7 — Revert ⟨treat the empty `content` as an artifact to enqueue⟩ → ⟨`assert!(!out.join("f.docx").metadata()?.len() == 0)`, i.e. the zero-byte-overwrite assertion⟩ RED.
- T3.8 — Revert ⟨reorder `UserFiltersStage::post()` after the wire-format cut⟩ → ⟨the `pre`-sees-no-`CustomNode` assertion⟩ RED.
- T3.9 — Revert ⟨P4's pre-flight check, letting the `Command::new("pandoc")` spawn fail bare⟩ → ⟨`assert!(stderr.contains("Q-18-"))` **and** `assert!(!out.join("f.docx").exists())`⟩ RED.
- T3.10 — Revert ⟨P4's `content: String::new()` decision into "write `RenderedOutput.content` to the output path"⟩ → ⟨the length/prefix assertion reddens with a 0-byte file⟩ RED.

### Refactor-induced vacuity check

- **"`--to docx` is no longer rejected" says nothing about pptx.** The two formats can
  independently regress at different points — the gate arm at `render.rs:680`, and the `TryFrom`
  arm at `format.rs:92` — so they need **two separately bound tests with two different revert
  hunks**: T3.1/T3.2, and T3.3 isolating the `TryFrom` arm. A single parameterized "both formats
  render" test with one revert hunk would let a pptx regression hide behind the docx pass.
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
  `<w:t>`) is what makes it more than a magic-number check; the *semantic* assertions are P7's
  golden-harness Task 11, not this task's. Do not strengthen T3.1 toward numbering — it would
  duplicate that and drift.
- **T3.6's `content.is_empty()` is also what an unimplemented stub returns.** Pair it with the
  on-disk non-empty assertion (as written), or the row passes against a stage that does nothing.
- **T3.9/T3.10 round out the "binary output is real" contract** alongside T3.1/T3.2's magic-number
  check — "a file exists" is exactly the failure mode all three guard against from different
  angles: a missing pandoc, a zero-byte write, and HTML bytes at a binary path.

---

## Task 4: B3 shared services wired into the Pandoc tail (staged resources, rewritten links)

This is this document's Task 4 (B3 shared services), distinct from
`2026-09-18-pandoc-hybrid-P7-implementation.md`'s own Task 4 (the per-format invocation builder) —
always disambiguate with the plan name when citing either from a third document.

**Scope.** Confirm — with tests, not by reading — that `ResourceCollector`'s mediabag/resource
staging and `LinkRewriteTransform` run **before** the wire-format handoff for a `Pandoc(fmt)`
render, so images and relative links resolve in the produced docx/pptx. Both are classified **B3**
(design doc §6): shared post-core services that cross the cut, needed by *any* Pandoc-tail format,
not only docx/pptx.

**Files.**
- `crates/quarto-core/src/pipeline.rs` — the `Pandoc(fmt)` transform list assembled by P1 Task 2:
  assert `ResourceCollector` and `link-rewrite` are **present**, i.e. *not* on the Pandoc-kind
  exclude list.
- `crates/quarto-core/src/transforms/link_rewrite.rs` — read-only. The doc comment at
  `link_rewrite.rs:19-30` documents that `Image::target.0` is rewritten via
  `resolve_static_resource_href`, explicitly "matching Q1" (landed via commit `1d17a9ce7`). The
  transform is reused verbatim; there is no fix in this task, only a binding test.
- `crates/quarto-core/tests/integration/pandoc_b3_services.rs` — new.
- `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs` — extend (created by Task 3).

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
| T4.1 | I | `build_transform_pipeline` for `Pandoc("docx")` | Build → assert the ordered transform-name list **contains** `resource-collector` and `link-rewrite` | none | those two names' presence in the `Pandoc` arm / absence from the exclude list |
| T4.2 | I | `LinkRewriteTransform` on a Pandoc-profile render | Run the pipeline on a fixture with `![cap](sub/pic.png)` and a relative `[text](other.qmd)` → assert the `Image.target` is the resolved staged href and the link target is rewritten | filesystem via `tempfile` | the `Image::target.0` rewrite in `link_rewrite.rs` |
| T4.3 | **E** | the real binary + real pandoc | `q2 render img.qmd --to docx` → `word/media/` contains one entry; `word/_rels/document.xml.rels` has an `image`-typed relationship targeting it; **stderr contains no `Could not fetch resource`** | nothing mocked | the `resource-collector` entry in the `Pandoc` transform list |
| T4.4 | **E** | same | The same fixture where the image path is authored project-root-absolute (`/sub/pic.png`) → same assertions | nothing mocked | the leading-`/` handling in `resolve_static_resource_href` |

**Revert hunks, stated exactly:**
- T4.1 — Revert ⟨add `resource-collector` to the `Pandoc`-kind exclude list⟩ → ⟨`assert!(names.contains(&"resource-collector"))`⟩ RED.
- T4.2 — Revert ⟨the `Image::target.0` rewrite⟩ → ⟨`assert_eq!(img.target.0, expected_staged_href)`⟩ RED.
- T4.3 — Revert ⟨exclude `resource-collector` for the Pandoc profile⟩ → ⟨`assert!(media_entries.len() == 1)` **and** `assert!(!stderr.contains("Could not fetch resource"))` in `test_e2e_docx_image_staged`⟩ RED.
- T4.4 — Revert ⟨the leading-`/`-means-project-root branch⟩ → ⟨the same assertions with the absolute-authored fixture⟩ RED.

### Refactor-induced vacuity check

- **"An image is missing" does not fail a docx render.** **(measured)** pandoc 3.8.1 emits
  `[WARNING] Could not fetch resource nope.png: replacing image with description` and substitutes
  the alt text, **exiting 0**. So an `E` row asserting only "the render succeeded" or "the docx
  exists" is fully non-discriminating about resource staging. The two discriminators are the
  `word/media/` entry count and the absence of that stderr line — both asserted in T4.3. This is
  the same blind spot P7's golden-harness extractor's media inventory closes for the goldens.
- **T4.1's presence assertion is not a substitute for T4.3.** A transform can be *in the list* and
  still have no effect for a profile that never reaches its self-gate — the exact failure mode
  behind CLAUDE.md's 2026-04-20 `CodeHighlightStage` incident. T4.3 is the "the path was actually
  exercised" row.
- **T4.4 is not redundant with T4.2.** CLAUDE.md's path-resolution rule is explicit that a fix for
  one key or one form routinely leaves the sibling form broken; the two authored forms are
  separate states.

---

## Open questions

- **Does the project-mode containment gate need to cover `post_resources` too?** §13's wording
  names only `WebsiteProjectType::post_render`. `async fn post_resources` (immediately after
  `post_render`, writing llms.txt + per-page markdown companions) is also unconditional and also
  assumes HTML outputs exist. Task 2 above gates only `post_render` (at the call site,
  `orchestrator.rs:1220-1231`) and does not widen to cover `post_resources` — whether it should is
  Gordon's call, not something Task 2 should absorb unilaterally.
