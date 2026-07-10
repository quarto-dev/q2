# P8 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P8-content-hidden.md`](2026-08-20-pandoc-hybrid-P8-content-hidden.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§6, §8, §9)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Depends on:** independent to start (Tasks 1–3 are startable today); the docx/pptx smoke fixture
(Task 4) is gated on P7's Pandoc tail, and its pptx leg additionally on **P1 Task 1**
(`FormatIdentifier::Pptx`).
**Status:** Ready for subagent-driven execution.

This file adds nothing to P8's scope — it converts P8's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P8 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P8 + the design doc; where this file and the plan disagree, the plan wins.

Every `file.rs:line` / `file.lua:line` below was opened and confirmed against the worktree at
`feature/pandoc-writer-hybrid` and against `/Users/gordon/src/quarto-cli` (tag v1.11.3) on
2026-09-18. Where a citation had drifted, this file cites what is actually there and records the
drift in **Findings for Gordon** rather than silently propagating or silently "fixing" it.

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **U** | Rust unit test | `#[test]` in `mod tests` inside the crate under test — for P8 that is `crates/quarto-core/src/transforms/conditional_content.rs::tests` (`:628-990`), `crates/quarto-core/src/format.rs::tests`, and `crates/quarto-core/src/transforms/llms.rs::tests` | `cargo nextest run -p quarto-core` |
| **I** | Rust integration test | `crates/quarto-core/tests/integration/<name>.rs`, registered `pub mod <name>;` in `crates/quarto-core/tests/integration/main.rs` (**96** `pub mod` lines today). **Never** a top-level `crates/quarto-core/tests/<name>.rs` — `.claude/rules/integration-tests.md` | `cargo nextest run -p quarto-core` (binary `integration`) |
| **E** | end-to-end CLI test | drives the real `q2` binary (`cargo run --bin q2 -- render … --to docx`) and inspects the produced file | **P8 has two, both gated — see Task 4.** Not startable today: `crates/quarto/src/commands/render.rs:680-685` bails on every non-native format |
| **L** | Lua/pandoc integration test | a real `pandoc -f json -t <fmt>` invocation | `cargo nextest run -p quarto-core`; **no skip guard** — pandoc is a hard test-suite requirement in this repo (`crates/pampa/CLAUDE.md`: "Some tests require pandoc to be properly installed to pass"), with precedent at `crates/pampa/tests/integration/test.rs:95` and `:165`, neither of which skips |
| **G** | dev-only golden capture | real Q1 `quarto` + `external-sources/`; insta snapshots | **P8 needs zero G-tier tests.** P7 owns the golden harness (`cargo xtask capture-pandoc-goldens`); P8 adds no fixture to it |

**`I` is deliberately zero.** Everything P8 exercises — `ConditionalContentTransform`,
`lua_format_for`, `llms_view_active` — lives inside `quarto-core`, so the lowest faithful tier for
all of it is `U`. Pushing any of it to `I` would be the "cheap-tier-testable logic in the slow
tier" failure the discipline names.

---

## Revert hunks for a verification-only plan (pre-existing vs. cross-plan vs. none)

**Read this before writing a single test.** P8's scope is "verify that something already works."
`ConditionalContentTransform` shipped 2026-08-10…08-18 as Phase 4 of the project-profiles epic
(bd-fu16z22k / PR #492, plus bd-stbdlesy and bd-wbnaa2ud). **P8 adds no production hunk of its
own.** Every revert hunk below is therefore one of three things, and each test row says which:

- **`pre-existing`** — the hunk is already on `main`; the test is a *regression guard* over
  already-shipped behavior. This is legitimate and valuable, and it is what most of P8 is. It is
  *not* a P8-local hunk, and this file never pretends otherwise. 11 of P8's 14 tests are in this
  class; they are itemized under **Pre-existing-hunk seams** at the end of this file.
- **`cross-plan`** — the hunk belongs to another plan (P1 Task 1, P7); the seam is written
  `seam deferred until <plan>'s <capability>`. 2 tests.
- **`none`** — no hunk anywhere reddens the assertion, because the behavior under test is a
  deliberate *absence*. Logged `accepted-untested` in the **Missing-test pass**, never dressed up
  as bound. 1 assertion (T1.6's zero-diagnostic half).

**The consequence an implementer must internalize:** for a `pre-existing` row, "run the test and
watch it fail first" (CLAUDE.md's mandatory TDD step 2) **cannot be performed as written** — the
test passes on the first run because the behavior is already there. The substitute, which is
mandatory and not optional, is: **temporarily revert the named hunk, observe the named assertion
go RED, restore the hunk.** A row whose implementer cannot make that happen has a wrong hunk
named, and the row must be escalated rather than committed green.

---

## The discriminating input (what makes a `when-format` fixture non-vacuous for a Pandoc target)

Design doc §6's `ConditionalContentTransform` row asserts the transform is "already correct for a
genuine Pandoc `target_format` string with no further work." **That is a claim, and testing it is
P8's entire job.** A fixture proves nothing unless its expected value differs between the correct
state and the plausible-incorrect states. So: what *are* the states, and what separates them?

**The two functions in the path.** `ConditionalContentTransform::transform`
(`conditional_content.rs:141-172`) computes `lua_format`, once, at `:142`:

```rust
let lua_format = crate::format::lua_format_for(&ctx.format.target_format).to_string();
```

`lua_format_for` (`format.rs:164-170` — **P8's plan cites this range and it is exactly right**) is
a three-arm match: two preview pseudo-format arms, then `other => other`. Format matching then goes
through `check_format` (`conditional_content.rs:334-342`) → `pampa::lua::quarto_doc::is_format_match`
(`crates/pampa/src/lua/quarto_doc.rs:74-89`), which is an exact-match early return (`:76-78`)
followed by a `match query` over six alias families (`html`, `html:js`, `latex`|`pdf`, `epub`,
`markdown`, `asciidoc`|`asciidoctor`) and `_ => false` (`:87`).

**Empirically measured truth table** (the two functions were extracted verbatim and executed;
this is measurement, not reading):

| `target_format` | `lua_format_for` → | query | match? |
|---|---|---|---|
| `docx` | `docx` | `docx` | **true** |
| `docx` | `docx` | `html` | false |
| `docx` | `docx` | `pdf` / `latex` | false |
| `pptx` | `pptx` | `pptx` | **true** |
| `pptx` | `pptx` | `docx` | false |
| `latex` | `latex` | `pdf` | **true** |
| `beamer` | `beamer` | `pdf` | **true** |
| `docx` | `docx` | `docs` (typo) | false, **silently** |
| `acm-docx` | `acm-docx` | `docx` | **false — see Finding F1** |
| `acm-html` | `acm-html` | `html` | **false — see Finding F1** |

**So the claim is true, but only for a *bare* Pandoc writer name.** For `docx`, `pptx`, `latex`
the passthrough is correct and no work is needed. For an **extension format** (`acm-docx`,
`my-journal-docx` — both of which `Format::from_format_string` accepts via
`format.rs:418-429`, producing `identifier = Docx` but `target_format = "acm-docx"`)
`lua_format_for` passes the *un-canonicalized* string through and every `when-format` query fails.
`Format::lua_format()` (`format.rs:383-389`) *does* canonicalize (`self.identifier.as_str()`), and
its own doc comment claims the two helpers "agree on the pseudo-format and base cases" — a
correctly-scoped claim that quietly excludes extension formats. The transform calls the
string-only one. **That is Finding F1; P8 does not resolve it — filed as `bd-n5hjadpr`
(decided with Gordon, 2026-09-18), outside this epic.** Every fixture in this file therefore uses
a bare writer name; an extension-format fixture would be exercising that strand, not P8.

### The discriminating fixture, stated

A fixture that only asserts `when-format="html"` content is **kept** under an HTML render is
vacuous — it passes identically whether the format is threaded correctly or hardcoded to `"html"`.
The discriminating shape is the **four-quadrant matrix under a Pandoc target**:

| marker | condition | `target_format = "docx"` | what a wrong state does |
|---|---|---|---|
| `.content-visible` | `when-format="docx"` | **survives** | RED if format is hardcoded `"html"`, or if `lua_format_for` gains a rewriting arm |
| `.content-visible` | `when-format="html"` | **dropped** | RED if format is hardcoded `"html"` |
| `.content-hidden` | `when-format="docx"` | **dropped** | RED if format is hardcoded `"html"` |
| `.content-hidden` | `when-format="html"` | **survives** | RED if format is hardcoded `"html"` |

Quadrants 2 and 3 are the load-bearing ones: they are the only two whose expected value *inverts*
between "the Pandoc format string reached the predicate" and "it did not." A P8 test that omits
them is a test of nothing.

**And one more seam, easy to miss.** `conditional_content.rs`'s existing `mod tests` drives the
`Walker` directly through a `run(blocks, format, active, meta)` helper (`:670-697`) that pushes
`format` straight into `ConditionEnv` — **bypassing `lua_format_for` entirely**. Every existing
test, and any new four-quadrant test written with that helper, would survive replacing `:142` with
`let lua_format = "html".to_string();`. That is exactly the discipline's "a test that *exercises*
the wrong thing passes whether or not the fix exists." **T1.4 exists solely to close that hole**,
by driving `ConditionalContentTransform::transform` with a real docx `Format`.

---

## Task 1: Verify `when-format` / `unless-format` against genuine Pandoc format strings

**Scope.** P8 checklist item 1. Prove design §6's "already correct for a genuine Pandoc
`target_format` string" claim by measurement, for `docx`, `pptx`, and `latex` — including the
`lua_format_for` leg the existing test helper bypasses. Pin the `unless-*` symmetry and the
unknown-format behavior while here.

**Files.** **No production file changes.** Test-only, all additions to existing `mod tests`
blocks:
- `crates/quarto-core/src/transforms/conditional_content.rs` — new `#[test]`s in `mod tests`
  (`:628`); reuses the existing `div`/`attr`/`run`/`empty_meta`/`texts` helpers (`:635-707`).
- `crates/quarto-core/src/format.rs::tests` — extend the **existing**
  `test_lua_format_for_passes_through_real_formats` (`:895-900`, whose array already contains
  `"docx"`) with `"pptx"` and `"beamer"`.

Read-only anchors this task verifies against (all confirmed present today):
- `conditional_content.rs:142` — the `lua_format_for(&ctx.format.target_format)` call.
- `conditional_content.rs:303-332` (`conditions_match`), `:313-318` (the six key arms),
  `:329` (`result = result && (invert != any)`), `:334-342` (`check_format`).
- `crates/pampa/src/lua/quarto_doc.rs:74-89` (`is_format_match`), `:76-78` (exact-match early
  return), `:87` (`_ => false`).
- `crates/quarto-core/src/pipeline.rs:1172` — `pipeline.push(Box::new(ConditionalContentTransform::new()))`,
  unconditional, first in Normalization, inside `build_transform_pipeline` (`:1144`). **P8's plan
  cites `pipeline.rs:1172` and it is exactly right.**

**Acceptance criterion.**
1. The four-quadrant matrix above holds for `target_format = "docx"` and for `"pptx"` — all four
   rows, both formats, asserted individually with distinct expected values (T1.2, T1.5).
2. `unless-format` inverts `when-format` on the same four inputs: `.content-visible
   unless-format="docx"` is **dropped** under docx, `unless-format="html"` **survives** (T1.3).
3. `ConditionalContentTransform::transform` — not the `Walker` helper — driven with
   `Format::from_format_string("docx").unwrap()` keeps `when-format="docx"` content and drops
   `when-format="html"` content (T1.4).
4. `lua_format_for` passes `"pptx"` and `"beamer"` through unchanged (T1.1).
5. `when-format` naming a string q2 does not know (`"docs"`) drops `.content-visible` content and
   emits **zero** diagnostics (T1.6) — Q1-parity silence, deliberately not a warning.
6. `cargo clippy -p quarto-core --all-targets -- -D warnings` and `cargo nextest run -p
   quarto-core` green; zero `.snap` files change.

**Prerequisite.** **None.** Startable today. The pptx *unit* rows (T1.5) need no `Format` at all —
they pass `"pptx"` straight into the `Walker` — so they are writable before P1 Task 1 adds
`FormatIdentifier::Pptx`. Only T1.4's docx leg needs a real `Format`, and `FormatIdentifier::Docx`
already exists (`format.rs:29`).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | U | `quarto_core::format::lua_format_for` | extend `test_lua_format_for_passes_through_real_formats` (`format.rs:895`) with `"pptx"`, `"beamer"` → `assert_eq!(lua_format_for(f), f)` | none | **pre-existing** — the `other => other` arm, `format.rs:168` |
| T1.2 | U | `quarto_core::transforms::conditional_content::Walker` (via `run`) | four `run(&mut blocks, "docx", &[], &meta)` calls, one per matrix quadrant → survives / `is_empty()` per the table | none | **pre-existing** — the exact-match early return `if format == query { return true; }`, `quarto_doc.rs:76-78` |
| T1.3 | U | same | `.content-visible unless-format="docx"` under docx → `blocks.is_empty()`; `unless-format="html"` → `blocks.len() == 1` | none | **pre-existing** — the `"unless-format" => (true, Kind::Format)` arm, `conditional_content.rs:317` |
| T1.4 | U | `quarto_core::transforms::ConditionalContentTransform::transform` (the real `AstTransform`) | build `Format::from_format_string("docx")`, a `RenderContext`, a `Pandoc` with the q2/q4 quadrant Divs; `.transform(&mut ast, &mut ctx).await` → docx content survives, html content dropped | `ProjectContext` / `DocumentInfo` / `BinaryDependencies` test doubles already used by `pipeline.rs::tests` (`make_test_runtime()`, `pipeline.rs:3835`) | **pre-existing** — the `lua_format_for(&ctx.format.target_format)` binding, `conditional_content.rs:142` |
| T1.5 | U | `Walker` (via `run`) | the same four quadrants with `"pptx"` → per the table | none | **pre-existing** — `quarto_doc.rs:76-78`, as T1.2 |
| T1.6 | U | `Walker` (via `run`) | `.content-visible when-format="docs"` under docx → `blocks.is_empty()` **and** `diags.is_empty()` | none | drop half: **pre-existing** `_ => false`, `quarto_doc.rs:87`. Silence half: **none** — see Missing-test pass |

**Revert hunks, stated exactly:**

- **T1.1** — Revert ⟨the `other => other` fallthrough arm of `lua_format_for`, `format.rs:168`, to
  e.g. `other => "html"`⟩ → ⟨`assert_eq!(lua_format_for("pptx"), "pptx")`⟩ RED. **Pre-existing
  hunk**; this is a regression guard on the passthrough the design doc's claim rests on, not a
  test of anything P8 adds.
- **T1.2** — Revert ⟨`if format == query { return true; }`, `quarto_doc.rs:76-78`⟩ →
  ⟨`assert_eq!(blocks.len(), 1)` on the `.content-visible when-format="docx"` quadrant⟩ RED
  (`is_format_match("docx","docx")` then falls to `_ => false`, and the Div is dropped).
  **Pre-existing hunk.** Note the html quadrants stay GREEN under that revert — which is why all
  four quadrants are needed, not just quadrant 1.
- **T1.3** — Revert ⟨`"unless-format" => (true, Kind::Format)` to `(false, Kind::Format)`,
  `conditional_content.rs:317`⟩ → ⟨both assertions in T1.3 simultaneously⟩ RED (each flips).
  **Pre-existing hunk.**
- **T1.4** — Revert ⟨`conditional_content.rs:142` to `let lua_format = "html".to_string();`⟩ →
  ⟨T1.4's "docx content survives" assertion⟩ RED. **Pre-existing hunk, and this is the single
  most important row in P8:** under that same revert, T1.2/T1.3/T1.5 all stay GREEN, because the
  `run` helper never calls `lua_format_for`. T1.4 is the only test in the plan that would notice.
- **T1.5** — as T1.2, with `"pptx"`.
- **T1.6** — Revert ⟨`_ => false`, `quarto_doc.rs:87`, to `_ => true`⟩ →
  ⟨`assert!(blocks.is_empty())`⟩ RED. **Pre-existing hunk.** The `diags.is_empty()` half has
  **no** revert hunk — logged `accepted-untested`.

### Refactor-induced vacuity check

- **The "already correct, no further work" claim (design §6).** Every T1.* test passes on `main`
  today, before P8 touches anything. They therefore discriminate **no P8 change** — there is none
  to discriminate. What they are is a regression guard over three pre-existing hunks
  (`format.rs:168`, `quarto_doc.rs:76-78`, `conditional_content.rs:142`), and the file says so on
  every row rather than implying a P8 hunk exists. **And separately: is the claim ever false?** It
  was checked four ways against real code, and it is false in one: extension formats (F1). The
  other three probes came back clean — `lua_format_for` has **no** Pandoc branch and does not need
  one (bare passthrough is the correct answer for `docx`/`pptx`/`latex`); `when-format: docx`
  resolves against the **`target_format` string**, which for a bare format equals both the `--to`
  string and the Pandoc writer name Q1 would put in `FORMAT`; and `unless-format` is handled
  symmetrically, one arm apart, at `conditional_content.rs:313-318`. The pptx-unresolvable finding
  P7 hit at `Format::from_format_string` **does not** reach `lua_format_for`'s correctness — a
  `"pptx"` string maps correctly the moment one can be constructed; what it blocks is only the
  *E-tier* leg (Task 4), never the U-tier predicate.
- **The corrected "absent" characterization (`657f1b0b5`).** That commit fixed P8's own plan for
  claiming the design doc "repeats the stale 'absent' framing in §7." Two corrections landed: the
  section is **§8** ("Cross-cutting decisions"), not §7 ("Upstream Q1 contribution"); and §8's
  actual text does **not** say the feature is absent — it says content-hidden gating is "deferred
  to P8, out of scope for docx/pptx v1," which is a statement about *verification* being deferred,
  not about the feature not existing. **No seam in this file asserts against the stale
  characterization.** Concretely: no test here is written as "content-hidden does not work for
  docx yet" or gated on the feature being missing; every row's expected value is the
  *already-correct* behavior, and T1.2/T1.5's expected values (`survives`, `dropped`) are the ones
  a working implementation produces. A row phrased the other way would have been a stale-expected-
  value test that went permanently green the day bd-fu16z22k merged, which was before this plan
  was drafted.
- **The unknown-format expected value.** T1.6 asserts `"docs"` drops the content. Does that value
  still differ across the states the test exists to distinguish? Yes: correct = dropped,
  `_ => true` = kept. But note it does **not** distinguish "q2 doesn't know `docs`" from "q2 knows
  `docs` and it doesn't match `docx`" — both drop. T1.6 is deliberately only a guard on the
  closed-world default, not on typo detection, and its name must say so.

---

## Task 2: Verify the resolved-Div unwrapping is format-independent and Pandoc-writer-safe

**Scope.** P8 checklist items 2 and 3, grouped — they are one subject (what the wrapper resolves
to, and whether that decision has an HTML assumption in it). Confirm bd-wbnaa2ud's unwrapping
produces an AST shape Pandoc's docx/pptx writers accept, and that `is_bare_wrapper` behaves
identically across formats.

**Files.** **No production file changes.** Test-only:
- `crates/quarto-core/src/transforms/conditional_content.rs::tests` — T2.1.
- `crates/quarto-core/tests/integration/conditional_content_pandoc.rs` (**new**), registered
  `pub mod conditional_content_pandoc;` in `crates/quarto-core/tests/integration/main.rs`
  (alphabetized; 96 `pub mod` lines today) — T2.2, T2.3.

Read-only anchors:
- `conditional_content.rs:624-626` — `fn is_bare_wrapper(attr: &Attr) -> bool`. **Checklist item 3
  is answered by the signature before any test runs: the function takes an `Attr` and nothing
  else. It cannot contain a format-dependent assumption because it never sees a format.** The
  test below turns that reading into a mechanical assertion rather than leaving it as prose.
- `conditional_content.rs:394` (`splice = is_bare_wrapper(&div.attr)`), `:415-417` (the
  `BlockAction::Splice` return), `:372-377` (`Block::Div(div) => blocks.extend(div.content)`).
- `conditional_content.rs:578-610` (`strip_condition_attrs`), `:591`, `:609`.
- Q1 parity anchors, verified at v1.11.3: `customnodes/content-hidden.lua:66` (`return
  el.content`), `:154` (the "only called on spans and codeblocks" comment), `:211-217`
  (`clearHiddenVisibleAttributes`). **All three of q2's module-doc citations are exactly right.**

**Acceptance criterion.**
1. The same bare-wrapper fixture, run under `"html"`, `"docx"`, and `"pptx"`, produces the
   **identical** resolved block list — no `Block::Div` survives, content present — proving
   `is_bare_wrapper` is format-independent by measurement (T2.1).
2. A document whose conditional Divs have all resolved, serialized to Pandoc JSON and fed to
   `pandoc -f json -t docx`, exits 0 and produces a non-empty `.docx` whose `word/document.xml`
   contains the visible content's text and **not** the hidden content's (T2.2).
3. The same for `-t pptx` (T2.3).
4. Neither T2.2 nor T2.3 contains a stray empty `Div` in the JSON handed to pandoc — asserted on
   the JSON, before the invocation, so a pandoc-accepts-anything result cannot mask it.
5. `cargo clippy` / `cargo nextest run -p quarto-core` green.

**Prerequisite.** **None for T2.1.** T2.2/T2.3 need only `pandoc` on `PATH` (3.8.1 locally) — they
deliberately do **not** go through P4's transport or P7's tail, because the unit under test is the
*AST shape*, and inserting the whole Pandoc leg would make the lowest faithful tier much slower
for no added fidelity. A conditional-content-only fixture contains no `CustomNode`, so plain
Pandoc JSON suffices and P2's wire format is not needed either.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | U | `Walker` + `is_bare_wrapper` | one `.content-visible when-format=<fmt>` bare Div per format; `run(.., "html"/"docx"/"pptx", ..)` → in all three: `blocks.len()==1`, `!matches!(blocks[0], Block::Div(_))`, text present | none | **pre-existing** — `splice = is_bare_wrapper(&div.attr)`, `conditional_content.rs:394` |
| T2.2 | L | `Walker` → `pampa` Pandoc-JSON writer → real `pandoc -f json -t docx` | transform a fixture with one visible + one hidden conditional Div; assert the JSON has no empty-`Div` node; `Command::new("pandoc").args(["-f","json","-t","docx","-o",…])` → exit 0, output non-empty, `word/document.xml` contains `VISIBLE-SENTINEL`, does not contain `HIDDEN-SENTINEL` | real `pandoc` binary is the environment dep (not a mock); the unit under test — the transform + its AST output — is never mocked | **pre-existing** — the `BlockAction::Splice` arm `Block::Div(div) => blocks.extend(div.content)`, `conditional_content.rs:373` (bd-wbnaa2ud's fix) |
| T2.3 | L | same, `-t pptx` | as T2.2, asserted against the extracted `ppt/slides/*.xml` | same | same as T2.2 |

**Revert hunks, stated exactly:**

- **T2.1** — Revert ⟨`splice = is_bare_wrapper(&div.attr)` to `splice = false`,
  `conditional_content.rs:394`⟩ → ⟨`assert!(!matches!(blocks[0], Block::Div(_)))`, in all three
  format arms simultaneously⟩ RED. **Pre-existing hunk.** The three arms failing *together* is
  itself the evidence for format-independence; if a future refactor made one arm differ, T2.1
  would catch it in exactly the assertion that matters.
- **T2.2 / T2.3** — Revert ⟨the `Block::Div(div) => blocks.extend(div.content)` line of the
  `BlockAction::Splice` match, `conditional_content.rs:373`, to `other => blocks.push(other)`⟩ →
  ⟨the "JSON contains no empty-`Div` node" assertion⟩ RED. **Pre-existing hunk (bd-wbnaa2ud).**
  Note the pandoc-exit-0 assertion does **not** go red under that revert — pandoc accepts an empty
  `Div` happily — which is precisely why the JSON-level assertion is required and why criterion 4
  puts it *before* the invocation.

### Refactor-induced vacuity check

- **The positive control.** T2.2/T2.3 assert `HIDDEN-SENTINEL` is **absent** from the rendered
  docx. Absence is satisfied by an empty render, a failed render, a truncated unzip, or a sentinel
  string that was never in the fixture. Three positive controls are therefore mandatory in the
  same test, not optional: exit status 0, output file non-empty, and **`VISIBLE-SENTINEL` present**.
  A test asserting only the absence is the discipline's canonical vacuous test.
- **The "path was actually exercised" assertion.** T2.2/T2.3 must additionally assert the
  *pre-pandoc* JSON no longer contains the string `content-hidden` anywhere — establishing that
  the transform ran, rather than that the fixture never had a conditional Div. Without it, a
  fixture whose front matter accidentally disabled the transform would pass.
- **Expected-value collapse across the three T2.1 arms.** All three arms assert the *same* value
  (no surviving `Div`). That is the point of the test, but it means the assertion alone cannot
  distinguish "format-independent" from "the format argument is ignored" — those are the same
  state here, and that state is the correct one. The discriminator lives in T1.2/T1.4 instead
  (where the format argument *must* change the outcome). T2.1's value is kept as shape/gating
  only, and its name must not promise format sensitivity.

---

## Task 3: Verify the cut-boundary invariants — no marker survives, and the llms view is inert

**Scope.** P8 checklist item 4 (the llms two-view path is orthogonal to the Pandoc tail), plus the
structural contract P5's shim positioning depends on: that **no** `.content-visible` /
`.content-hidden` class and no `when-*` / `unless-*` attribute survives the transform, anywhere in
the AST. Both are "prove something is absent or inert at the cut."

**Files.** **No production file changes.** Test-only:
- `crates/quarto-core/src/transforms/conditional_content.rs::tests` — T3.1, T3.3.
- `crates/quarto-core/src/transforms/llms.rs::tests` — T3.2.

Read-only anchors:
- `conditional_content.rs:591` (`attr.2.retain(|k, _| !CONDITION_KEYS.contains(…))`) and `:609`
  (`attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)`) — the two hunks the invariant
  rests on.
- `conditional_content.rs:106-113` — `CONDITION_KEYS`, all six.
- `conditional_content.rs:278-284` — the `if !self.env.llms_view { return … }` early return. **A
  load-bearing safety branch; T3.3 binds it.**
- `crates/quarto-core/src/transforms/llms.rs:122-125` — `llms_view_active`, whose second conjunct
  is `crate::format::lua_format_for(&ctx.format.target_format) == "html"`. **This single conjunct
  is the whole mechanical answer to checklist item 4**: for `target_format = "docx"` it is false,
  so the two-view path cannot activate for a Pandoc target regardless of project config.
- Q1 side, why this invariant matters: `customnodes/content-hidden.lua:29`
  (`ast_name = "ConditionalBlock"`, class-keyed on both markers) and `:136-138`
  (`content_hidden()` registering `CodeBlock` and `Span`, with `Div` commented out because the
  custom-node handler owns it). If a marker class survived the cut, *both* of those Q1 mechanisms
  would fire on it.

**Acceptance criterion.**
1. After the transform, a `texts()`/walk assertion over the whole block list finds **no** occurrence
   of `content-visible`, `content-hidden`, or any key starting `when-`/`unless-` — with the
   path-exercised assertions in criterion 2 satisfied in the same test (T3.1).
2. T3.1's input contained, and the assertions confirm the fate of, **all five** marker carriers:
   a `.content-hidden` Div (text gone), a `.content-visible` Div (text present), a
   `.content-visible` Span (survives, marker stripped), a `.content-hidden` CodeBlock (gone), and
   a `.content-visible` Div nested inside a `Block::Custom` slot (text present). The fifth
   exercises `conditional_content.rs:501-518`.
3. `llms_view_active` returns `false` for a docx `Format` **even with** `project_kind == Website`
   and `website.llms-txt: true` in meta — i.e. the format conjunct, not the config conjunct, is
   what makes it false (T3.2).
4. With `target_format = "docx"`, `when-format="llms"` is inert: the `.quarto-llms-omit` /
   `.quarto-llms-keep` marker classes appear **nowhere** in the output (T3.3).
5. `cargo clippy` / `cargo nextest run -p quarto-core` green.

**Prerequisite.** **None.** All three tests are startable today. T3.1 is the test **P5 should be
able to point at** for its "no-op over an empty set" argument; it is written here rather than in
P5 because it asserts a property of Q2's transform, not of the shim.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | U | `Walker` + `strip_condition_attrs` | the five-carrier fixture from criterion 2; `run(.., "docx", ..)` → no marker class / condition attr anywhere, **and** all five per-carrier fate assertions | none | **pre-existing** — `attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)`, `conditional_content.rs:609` |
| T3.2 | U | `quarto_core::transforms::llms::llms_view_active` | build a website `RenderContext` with `llms-txt: true` and a docx `Format`; call → `false`. Control: the same context with an html `Format` → `true` | `ProjectContext`/`DocumentInfo` doubles | **pre-existing** — the `&& crate::format::lua_format_for(&ctx.format.target_format) == "html"` conjunct, `llms.rs:124` |
| T3.3 | U | `Walker` (via `run`, `llms_view = false`) | `.content-visible when-format="llms"` + `.content-hidden when-format="llms"` under `"docx"` → visible-when-llms dropped, hidden-when-llms kept, `!texts.contains("quarto-llms-")` | none | **pre-existing** — the `if !self.env.llms_view { return … }` early return, `conditional_content.rs:278-284` |

**Revert hunks, stated exactly:**

- **T3.1** — Revert ⟨`attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)`,
  `conditional_content.rs:609`⟩ → ⟨the "no `content-visible` class survives anywhere" assertion⟩
  RED. **Pre-existing hunk.** Crucially, the *removal* assertions (hidden Div's text gone) stay
  GREEN under that revert — the discriminator is the surviving element's class list, not the
  document's content. A second revert to check: ⟨`attr.2.retain(|k, _| !CONDITION_KEYS.contains(…))`,
  `:591`⟩ → ⟨the "no `when-*` attribute survives" assertion⟩ RED.
- **T3.2** — Revert ⟨the format conjunct at `llms.rs:124`, leaving only
  `llms_companions_enabled(meta, ctx)`⟩ → ⟨`assert!(!llms_view_active(&meta, &docx_ctx))`⟩ RED.
  **Pre-existing hunk.** The html control assertion stays GREEN, which is what makes the docx
  assertion load-bearing rather than tautological.
- **T3.3** — Revert ⟨the `if !self.env.llms_view { return … }` early return,
  `conditional_content.rs:278-284`, so the four-quadrant llms evaluation always runs⟩ →
  ⟨`assert!(!texts(&blocks).contains("quarto-llms-"))`⟩ RED. **Pre-existing hunk**, and it is an
  early-return safety branch, i.e. exactly the shape Check 3 says to hunt for.

### Refactor-induced vacuity check

- **The no-op-over-an-empty-set argument (P5).** P5's claim is that Q1's own
  `content-hidden.lua` / `quarto-pre/hidden.lua` pass "runs, matches zero Divs, no-ops. Not a
  collision; a no-op over an empty set." **An assertion of the form "no `.content-hidden` Div
  survives" is satisfied by a document that never had one** — the archetypal vacuous absence test.
  T3.1 is specified with criterion 2's five per-carrier fate assertions precisely so the path is
  proven exercised: a `.content-hidden` Div **was** present and **was** resolved, a
  `.content-visible` Div **did** survive with its content, and the Span / CodeBlock / nested-custom
  carriers each get their own assertion so no single recursion arm can silently stop being
  visited. Without those, T3.1 would pass on an empty `Vec<Block>`.
- **`ConditionalBlock`'s class-keyed registration (P5's flagged collision).** P5 lists
  `ConditionalBlock` (Q1 `customnodes/content-hidden.lua`, **line 29** — see Finding F4) among the
  class-keyed handler registrations that *would* collide with the wire wrapper if the shim were
  mispositioned, "relevant if any of P8's content-hidden types ever reach the cut." Under the
  frozen design they do not, because the transform resolves them pre-cut. **T3.1 is the assertion
  of that invariant** — before T3.1 exists, the invariant P5's entire shim-positioning argument
  rests on is asserted nowhere in the tree, in either plan. Its expected value ("no marker class
  anywhere") still differs across the states it distinguishes (`:609` present vs. reverted), so it
  has not collapsed.
- **Did the llms expected value collapse?** `llms_conditions_inert_without_llms_view`
  (`conditional_content.rs:975-989`) already asserts llms-inertness — but with `format = "html"`
  and `llms_view = false`, i.e. it distinguishes the flag, not the format. T3.3 changes the format
  to `"docx"`; the expected values are the same three as the existing test, so T3.3 adds nothing
  as an *assertion* and is justified only as the Pandoc-target instance of the same contract. It
  must be named as such, and it must **not** be presented as P8's answer to checklist item 4 —
  **T3.2 is**, because T3.2 is the only one of the two whose subject is the format gate itself.

---

## Task 4: docx/pptx `.content-visible` / `.content-hidden` smoke fixture — **GATED ON P7**

> **Do not start this task now, and do not let the checklist item above read as startable.**
> It is the item P8 took ownership of in `85ffccd78`, resolving the latent P7↔P8 cycle in which
> each plan pointed at the other and neither held the item. The resolved direction is: **P7
> produces the Pandoc tail; P8 verifies against it.** Concretely, today
> `crates/quarto/src/commands/render.rs:680-685` bails with `"Format '{}' is not yet supported.
> Only HTML and revealjs are available in this version."` for every non-native format, so there is
> no `--to docx` render path to smoke at all. An implementer dispatched on this task before P7
> lands has nothing to write but a placeholder.

**Scope.** One fixture exercising `.content-visible` / `.content-hidden` end-to-end through the
real `q2` binary to `--to docx` and `--to pptx`, inspecting the produced file — the
`End-to-end verification before declaring success` bar in CLAUDE.md, which no in-process test in
Tasks 1–3 satisfies.

**Files (when unblocked).**
- `crates/quarto-core/tests/fixtures/conditional_content_pandoc.qmd` (**new**) — one
  `.content-visible when-format="docx"` block carrying `DOCX-ONLY-SENTINEL`, one
  `.content-visible when-format="pptx"` carrying `PPTX-ONLY-SENTINEL`, one
  `.content-hidden when-format="docx"` carrying `NOT-IN-DOCX-SENTINEL`, and one unconditional
  paragraph carrying `ALWAYS-SENTINEL`.
- `crates/quarto-core/tests/integration/conditional_content_pandoc.rs` — extend the module Task 2
  created; register once in `main.rs`.

**Acceptance criterion (when unblocked).**
1. `cargo run --bin q2 -- render <fixture>.qmd --to docx` exits 0 and writes a non-empty `.docx`.
2. Its `word/document.xml` contains `DOCX-ONLY-SENTINEL` and `ALWAYS-SENTINEL`, and contains
   neither `NOT-IN-DOCX-SENTINEL` nor `PPTX-ONLY-SENTINEL`.
3. The mirror for `--to pptx`: `PPTX-ONLY-SENTINEL` and `ALWAYS-SENTINEL` present,
   `DOCX-ONLY-SENTINEL` absent.
4. The exact invocation and a snippet of the inspected output are recorded in the session
   transcript or in P8's plan file, per CLAUDE.md's point 3.

**Prerequisite (both legs).** **P7's per-format tail** — the capability is "a `--to docx` /
`--to pptx` render that produces a real file," i.e. P7's `render.rs:680-685` relaxation plus its
invocation builder (P7 Coarse-checklist item "Invocation builder (docx, pptx) …"). **P7's
companion has since landed** ([`2026-09-18-pandoc-hybrid-P7-implementation.md`](2026-09-18-pandoc-hybrid-P7-implementation.md));
the capability named here is its **Task 3** (relax the format gate, admit docx and pptx, route
through `render_qmd_to_pandoc`) plus its **Task 4** (the per-format invocation builder).
**The pptx leg additionally requires
[P1 Task 1](2026-09-18-pandoc-hybrid-P1-implementation.md) (`FormatIdentifier::Pptx`)** — verified today:
`FormatIdentifier` (`format.rs:23-40`) has no `Pptx` variant and `KNOWN_BASE_FORMATS`
(`crates/quarto-core/src/extension/discover.rs:241-250`) does not list `pptx`, so
`Format::from_format_string("pptx")` returns `Err("Unknown format: pptx")` from `format.rs:449`.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | E | the real `q2` binary, docx leg | `cargo run --bin q2 -- render <fixture>.qmd --to docx`; unzip → `word/document.xml` sentinel assertions per criteria 1–2 | none — the whole binary is the unit | **cross-plan** — `seam deferred until P7's per-format tail` (the `render.rs:680-685` relaxation + P7's docx invocation builder) |
| T4.2 | E | the real `q2` binary, pptx leg | as T4.1 with `--to pptx`, asserted against `ppt/slides/*.xml` | none | **cross-plan** — `seam deferred until P7's per-format tail` **and** P1 Task 1's `"pptx" => Ok(FormatIdentifier::Pptx)` arm in `TryFrom<&str> for FormatIdentifier` (`format.rs:82-96`) |

**Revert hunks, stated exactly:**

- **T4.1** — `seam deferred until P7's per-format tail`. No hunk in P8 or on `main` today reddens
  it; there is no docx render path to revert. Once P7 lands, the honest hunk is P7's
  `render.rs:680-685` relaxation, and the assertion that discriminates is criterion 2's
  **`DOCX-ONLY-SENTINEL` present** (the positive control), not the absence assertion.
- **T4.2** — `seam deferred until P7's per-format tail`; additionally RED today for an unrelated
  reason (P1 Task 1's `TryFrom` arm), which must not be mistaken for this test doing its job.

**Gate and skip policy — explicit, because a silently-skipping test is a vacuous test.** T4.1/T4.2
are **not** written now and **not** committed as `#[ignore]`. An `#[ignore]`d or
env-var-skipped E-tier test would report green in CI while asserting nothing, which is precisely
the failure mode CLAUDE.md's two recorded incidents (2026-04-20 `CodeHighlightStage`, 2026-05-20
stale-WASM preview) describe. Instead this task stays an **unchecked item on P8's checklist**, which
is visible by inspection, and the plan file records the gate. When P7 lands, the tests are written
unguarded: they either run and assert, or the suite is red.

### Refactor-induced vacuity check

- **The smoke fixture's expected value is an absence, and absence is cheap.**
  `NOT-IN-DOCX-SENTINEL` is absent from an empty `.docx`, from a zero-byte file, from a render
  that errored after creating the output path, and from an unzip that read the wrong member. The
  fixture is therefore specified with **three positive controls inside the same test** —
  exit 0, `ALWAYS-SENTINEL` present (the transform-independent control), and
  `DOCX-ONLY-SENTINEL` present (the feature's own positive control). The last of these is the one
  that actually proves gating worked: `ALWAYS-SENTINEL` would survive even if
  `ConditionalContentTransform` never ran.
- **Cross-format discrimination.** Asserting `PPTX-ONLY-SENTINEL` absent from the docx output and
  `DOCX-ONLY-SENTINEL` absent from the pptx output is what makes the pair non-vacuous: a single-
  format fixture cannot distinguish "the format string reached the predicate" from "every
  conditional block was kept." This is the E-tier restatement of the four-quadrant matrix.

---

## Missing-test pass

Reasoned across P8's load-bearing branches. Every one gets a bound seam or an explicit
`accepted-untested`. Silent omission would read as "covered."

1. **`when-format` naming a format q2 doesn't know.** Found the branch: there is no dedicated
   branch — `is_format_match` (`quarto_doc.rs:74-89`) closes its world at `_ => false` (`:87`).
   An unrecognized query is **silently treated as non-matching**: no early return, no warning.
   `.content-visible when-format="docs"` therefore drops its content with no diagnostic.
   `verdict()`'s existing Q-2-42 warning (`conditional_content.rs:249-267`) fires on unknown
   *attribute keys* (`when-profil`), never on unknown attribute *values*.
   → **Drop behavior: BOUND — T1.6**, revert `quarto_doc.rs:87`.
   → **Zero-diagnostic behavior: `accepted-untested: no production hunk emits a diagnostic here,
   so no revert can redden an `assert!(diags.is_empty())`. The assertion is kept in T1.6 as a
   change-detector on a deliberate Q1-parity choice (Q1's `isFormat` likewise ends in
   `else return false`, `pandoc/datadir/_format.lua:284-286`), and is labeled in the test's own
   comment as unbound. Whether q2's existing Q-2-42 strictness divergence should extend to unknown
   format *values* is a design question, not P8's to answer.`**
2. **`unless-format`'s symmetry with `when-format`.** Verified in code: one arm apart at
   `conditional_content.rs:313-318`, sharing `invert != any` at `:329`.
   → **BOUND — T1.3**, revert the `"unless-format" => (true, Kind::Format)` arm at `:317`.
3. **The `ConditionalBlock` class-keyed collision P5 flagged.** Under the frozen design no
   content-hidden type reaches the cut, because the transform resolves them all pre-cut. That
   invariant was asserted **nowhere in the tree** before this file — neither P5 nor P8 held a test
   for it, and P5's shim-positioning argument depends on it.
   → **BOUND — T3.1**, revert `conditional_content.rs:609` (and `:591` for the attribute half).
   This is the single highest-value new test in P8.
4. **The pptx leg specifically.** P7's finding is confirmed: `FormatIdentifier` (`format.rs:23-40`)
   has no `Pptx`, `KNOWN_BASE_FORMATS` (`discover.rs:241-250`) omits `pptx`, so
   `from_format_string("pptx")` → `Err("Unknown format: pptx")` (`format.rs:449`). **But the
   `when-format` predicate does not depend on that resolution** — `lua_format_for("pptx")` and
   `is_format_match("pptx","pptx")` both work on the bare string (measured).
   → **U-tier pptx: BOUND and startable today — T1.5, T2.3.**
   → **E-tier pptx: `cross-plan` — blocked on P1 Task 1 (`FormatIdentifier::Pptx`) *and* P7's
   tail — T4.2.** So P8's pptx coverage is not blocked wholesale; only its end-to-end leg is.
5. **The deferral itself (design §8 vs. P8's existence).** §8 says content-hidden / `when-format`
   gating is "deferred to P8, out of scope for docx/pptx v1," while P8 exists to verify it works.
   Reconciled, plainly: **P8 verifies the feature works for Pandoc targets; docx/pptx v1 does not
   promise it.** The verification is real (Tasks 1–3 measure it, and the answer is "it works for a
   bare Pandoc writer name"), the *promise* is what is deferred — no user-facing documentation,
   no golden-parity fixture in P7's harness, and Finding F1's extension-format hole stays open as
   `bd-n5hjadpr`. A
   reader who takes §8 to mean "the feature is absent for docx" is reading the corrected-away
   framing (`657f1b0b5`); a reader who takes P8's green tests to mean "supported in v1" is reading
   past §8. Both readings are wrong and this paragraph is the reconciliation.
   → **No test. Documentation-level; recorded here and raised as Finding F3** (whether anything
   user-facing should say so is Gordon's call, not P8's).
6. **`Inline::Custom` recursion.** `keep_block` has a deliberate `Block::Custom` arm recursing into
   every slot (`conditional_content.rs:501-518`); `keep_inline` (`:528-569`) has **no**
   `Inline::Custom` arm and falls through `_ => {}`. `Inline::Custom(CustomNode)` is a real variant
   (`crates/quarto-pandoc-types/src/inline.rs:53`) with live constructors (`Equation`,
   `CrossrefResolvedRef`).
   → **`accepted-untested: a marker Span nested inside an inline CustomNode's slot is never
   visited — it keeps its marker class and condition attributes past the cut, where Q1's own
   `content_hidden()` Span filter (content-hidden.lua:136-138) does fire on it, which is the one
   hole in P5's no-op-over-an-empty-set argument. Almost certainly unreachable today, because
   ConditionalContentTransform runs first in Normalization (pipeline.rs:1172), before any sugar
   transform creates a CustomNode — but the Block::Custom arm exists precisely because custom
   nodes were thought reachable there (plausibly via quarto-ast-reconcile's merge of executed
   output), and the asymmetry is unexplained. Not bound because writing the test requires deciding
   whether the arm should exist, which is a design decision — raised as Finding F2.`**
7. **`when-meta` / `unless-meta` under a Pandoc target.** Format-independent by construction
   (`check_meta`, `conditional_content.rs:350-361`, reads only `self.env.meta`), and covered for
   `"html"` by the existing `meta_truthiness` test (`:776-800`).
   → **`accepted-untested: no format dependency exists in the branch, so a docx instance of
   meta_truthiness would assert the same value for the same reason and discriminate nothing. Not
   in P8's checklist either.`**
8. **`when-profile` / `unless-profile` under a Pandoc target.** Same argument — `check_profile`
   (`:344-346`) reads only `active_profiles`. Covered for `"html"` at `:710-719`.
   → **`accepted-untested: format-independent by construction; a Pandoc instance adds no
   discriminating assertion.`**

---

## Findings for Gordon

**Status (updated 2026-09-18): nothing on this plan is open.** F1 was filed as `bd-n5hjadpr`
(not fixed here, per Gordon); F3 is resolved by a design-doc §12 bullet (below); F4 and F5 are
informational; **F2 is the one remaining item, and it is deliberately left open** — an unexplained
asymmetry in `keep_inline`, almost certainly unreachable, whose resolution is a contract decision
rather than a verification. It parks no seam and blocks no task.

**F1 — design §6's "already correct for a genuine Pandoc `target_format` string with no further
work" is true for a bare writer name and false for an extension format. STOP item.**
`ConditionalContentTransform` computes its format via the string-only `lua_format_for`
(`conditional_content.rs:142` → `format.rs:164-170`), which passes any non-pseudo string through
verbatim. For `format: acm-docx` — which `Format::from_format_string` accepts at `format.rs:418-429`,
yielding `identifier = Docx` but `target_format = "acm-docx"` — `lua_format_for` returns
`"acm-docx"` and `is_format_match("acm-docx", "docx")` is **false** (measured, not inferred): the
exact-match arm misses and `"docx"` is not one of `is_format_match`'s six alias families. So
`::: {.content-visible when-format="docx"}` in an `acm-docx` document is **silently dropped**.
Q1 does not behave this way — it sets `FORMAT` to the pandoc writer name, so `isFormat("docx")` is
true. The canonicalizing helper already exists: `Format::lua_format()` (`format.rs:383-389`) uses
`self.identifier.as_str()`, and its own doc comment's claim that the two "agree on the
pseudo-format and base cases" is scoped exactly tightly enough to exclude this case. **Two things
make this Gordon's call rather than P8's:** (a) it is **pre-existing and not Pandoc-specific** —
`acm-html` + `when-format="html"` is broken today on the HTML leg by the identical mechanism, and
so is `llms_view_active` (`llms.rs:124`) for any extension HTML format; (b) changing
`conditional_content.rs:142` to `ctx.format.lua_format()` is a behavior change to a shipped
transform, which is not something a verification-only plan should land.

**Resolved 2026-09-18, decided with Gordon: filed as `bd-n5hjadpr`, not fixed here and not added
to any of P1–P8.** Per this repo's braid-vs-plans rule this is out-of-plan, team-shared work —
it is a pre-existing Q2 gap that this epic merely made newly visible, not part of completing any
plan. P8's own tasks are therefore written against the **bare-writer-name** case only, which is
what v1 ships (`docx`, `pptx`, `latex` — no extension format is in scope), and Task 1's vacuity
check records that a `when-format` fixture using an extension format would be testing
`bd-n5hjadpr`, not P8.

**The strand is wider than this finding, because the documented grammar is wider.** Gordon asked
for quarto-web's own format-variant documentation to be consulted before filing; doing so
(`docs/authoring/conditional.qmd` §Format Matching + its `_format-aliases.md` include, and Q1's
own grammar comment at `core/pandoc/pandoc-formats.ts:126-129`) turned up three distinct gaps
where this finding had seen one:

1. **Matching uses the wrong string** — this finding, exactly as written above. Also
   **bidirectional**, which this finding did not note: `when-format="acm-docx"` is *true* in q2
   and *false* in Q1 (Q1's `FORMAT` is `docx`, and no alias branch matches `acm-docx`).
2. **The alias table is a proper subset of Q1's** — F5 below, whose documented contract is
   `_format-aliases.md`.
3. **The format-string grammar itself is a subset.** Q1's grammar is
   `baseName+<variants | modifiers>-<variants>[<extension>]` with modifiers and variants in any
   order (its own examples: `acm-pdf+foo`, `gfm-raw_html`, `acm-2023-pdf+foobar`, and the
   bracket form `pdf[aspa]`). q2's `parse_format_descriptor`
   (`extension/discover.rs:252-271`) implements only `<extension>-<base>` against an
   8-entry `KNOWN_BASE_FORMATS` (`:241-250`), so `gfm-raw_html` and `acm-pdf+foo` are not
   mismatched but **rejected outright** at `format.rs:449`. q2 also has no `variant` render key,
   which Q1 documents and appends to the writer string.

**One scope question this research closed, measured rather than reasoned:** pandoc **strips**
`+EXT`/`-EXT` before setting `FORMAT` (pandoc 3.8.1: `--to gfm+emoji` → `FORMAT=[gfm]`,
`--to markdown+smart-raw_html` → `FORMAT=[markdown]`). So the `+`/`-` half of the grammar does
**not** affect format *matching* in either implementation — it affects only parsing, gap 3. Gap 1
is strictly about the extension-*prefix* shape, and the two are independently fixable.

**F2 — `keep_inline` has no `Inline::Custom` arm, asymmetrically with `keep_block`'s deliberate
`Block::Custom` arm.** `conditional_content.rs:501-518` recurses into every slot of a
`Block::Custom`; `:528-569` has no counterpart for `Inline::Custom`
(`quarto-pandoc-types/src/inline.rs:53`), which falls through `_ => {}`. A marker Span inside an
inline CustomNode slot would keep its marker class past the cut, where Q1's `content_hidden()`
Span handler (`customnodes/content-hidden.lua:136-138`) fires on it — the one hole in P5's
"no-op over an empty set." Almost certainly unreachable, since the transform runs first in
Normalization before any CustomNode is built — but that argument applies equally to
`Block::Custom`, whose arm exists anyway, so the asymmetry is unexplained. Deciding whether to add
the arm is a design decision about the transform's contract, not a verification. Recorded, not
resolved; logged `accepted-untested` above.

**F3 — §8's "out of scope for docx/pptx v1" versus P8's green tests: nothing user-facing states
which one ships.** The reconciliation is written out in Missing-test pass item 5 — P8 verifies it
works, v1 doesn't promise it. That is coherent, but it lives only in plan files. Whether the
docs/ site, a `Q-*` diagnostic, or design §12's "Known limitations" list should say so is a
scope/communication call. Raising rather than deciding.

**RESOLVED 2026-09-18, decided with Gordon: one bullet in design doc §12**, at
`claude-notes/designs/pandoc-hybrid-architecture.md:425-433`, with an Amendment-log entry at
`:496-507`. Chosen over the Definition-of-done paragraph and the docs site: §12 is where this
epic's limitations already live, and the DoD's "What 'docx/pptx support' means in v1" paragraph
already draws from §12 — so one edit reaches both without duplicating a claim that could then
drift. The bullet's substance: **the behavior is verified, the support is not claimed** — "works
for a bare Pandoc writer name, but is not promised or documented for v1" — plus an explicit "do not
draft release notes that promise it."

**Why this one was worth settling while the other advisory findings were not.** F2, and P7's F4/F6,
are scope questions about code behavior with a working default already in their tasks; leaving them
open costs nothing until someone deliberately widens scope. F3 was the only one whose risk *grew*
with time: two true statements (§8 says out of scope, P8's tests say it works) with nothing
user-facing adjudicating, which is the same shape as the defect round 4 already caught once — the
DoD bullet that sampled four of §12's seven limitations and "read materially rosier than §12 in
full" to a PM writing release notes. A release note drawn from either half alone would have been
defensible and one of them would have been wrong.

**No scope, code or test change.** §8's deferral stands; P8's tests stand; nothing in this
companion's tasks moves. The bullet also records that the *extension-format* case (`acm-docx`) is
genuinely broken rather than merely unclaimed, pointing at `bd-n5hjadpr` — so the two halves of
F1/F3 read as one coherent story rather than as a limitation and an unrelated bug.

**F4 — citation drift (one line), in P5, not P8.** P5 cites `ConditionalBlock` at
`customnodes/content-hidden.lua:27`; at v1.11.3 `ast_name = "ConditionalBlock"` is at **line 29**.
Cited as `:29` throughout this file. **Everything else checked out exactly:** P8's own
`format.rs:164-170` and `pipeline.rs:1172` are both correct to the line, and all three of
`conditional_content.rs`'s Q1 citations (`content-hidden.lua:66`, `:154`, `:211`/`:216`) are
correct. Not silently fixed in P5; flagged here.

**F5 — q2's `is_format_match` alias table is a proper subset of Q1's `isFormat`, and the gap is
silent.** Q1 (`pandoc/datadir/_format.lua:244-287`) additionally handles `odt`/`opendocument` as
synonyms, matches **any** query containing `epub` via the Lua pattern `to:match 'epub'`, and
recognizes `confluence`, `docusaurus`/`docusaurus-md`, `email`, `dashboard`, `hugo`/`hugo-md`.
q2 (`quarto_doc.rs:80-88`) has none of these, so e.g. `when-format="opendocument"` under `--to odt`
and `when-format="epub3"` under `--to epub2` are both **false in q2, true in Q1** (measured). All
of it is outside v1's docx/pptx/latex scope, and none of it is on P8's checklist — but they are
real `when-format` divergences from Q1 in a transform the design doc calls "already correct," so
recording them beats discovering them. No test specified. **Resolved 2026-09-18 with F1: this is
gap 2 of `bd-n5hjadpr`, which also records the documented contract to implement against —
quarto-web's `docs/authoring/_format-aliases.md`, whose table is slightly wider than the `isFormat`
source alone suggests (`html` additionally aliases `dashboard` and `email`; `markdown` additionally
aliases `hugo-md` and `docusaurus-md`).**

**F6 — q2 strips more than Q1 does, in q2's favor.** Q1's `clearHiddenVisibleAttributes`
(`content-hidden.lua:211-217`) clears `when-format`/`unless-format`/`when-profile`/`unless-profile`
and the two marker classes — it does **not** clear `when-meta`/`unless-meta`. q2 strips all six
`CONDITION_KEYS` (`conditional_content.rs:106-113`, `:591`). Almost certainly the better behavior
(a resolved condition has no business surviving into output), and harmless for docx/pptx where a
stray `when-meta` attribute on a Div is dropped by the writer anyway — but it is a real divergence
that a byte-parity golden over Q1's markdown or HTML writer would surface. Noting it so it is not
diagnosed from scratch inside a P7 golden diff.
