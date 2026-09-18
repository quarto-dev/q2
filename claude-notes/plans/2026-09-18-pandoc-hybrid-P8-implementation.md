# P8 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-20
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P8-content-hidden.md`](2026-08-20-pandoc-hybrid-P8-content-hidden.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§6, §8, §9)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Depends on:** Tasks 1–3 are done. The docx/pptx smoke fixture (Task 4) is gated on
**P7-foundation's Task 3** (Pandoc-tail reachability) *and* **P7's Task 4** (the invocation
builder). It is not additionally gated on P1 Task 1 (`FormatIdentifier::Pptx`), which has landed.
**Status:** Tasks 1-3 done. Task 4 remains gated as above.

This file converts P8's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P8 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P8 + the design doc; where this file and the plan disagree, the plan wins.

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **U** | Rust unit test | `#[test]` in `mod tests` inside the crate under test — for P8 that is `crates/quarto-core/src/transforms/conditional_content.rs::tests` (`:628-990`), `crates/quarto-core/src/format.rs::tests`, and `crates/quarto-core/src/transforms/llms.rs::tests` | `cargo nextest run -p quarto-core` |
| **I** | Rust integration test | `crates/quarto-core/tests/integration/<name>.rs`, registered `pub mod <name>;` in `crates/quarto-core/tests/integration/main.rs`. **Never** a top-level `crates/quarto-core/tests/<name>.rs` — `.claude/rules/integration-tests.md` | `cargo nextest run -p quarto-core` (binary `integration`) |
| **E** | end-to-end CLI test | drives the real `q2` binary (`cargo run --bin q2 -- render … --to docx`) and inspects the produced file | **P8 has two, both gated — see Task 4.** Not startable until the CLI admits non-native formats |
| **L** | Lua/pandoc integration test | a real `pandoc -f json -t <fmt>` invocation | `cargo nextest run -p quarto-core`; **no skip guard** — pandoc is a hard test-suite requirement in this repo (`crates/pampa/CLAUDE.md`), with precedent at `crates/pampa/tests/integration/test.rs:95` and `:165`, neither of which skips |
| **G** | dev-only golden capture | real Q1 `quarto` + `external-sources/`; insta snapshots | **P8 needs zero G-tier tests.** P7 owns the golden harness (`cargo xtask capture-pandoc-goldens`); P8 adds no fixture to it |

**`I` is deliberately zero.** Everything P8 exercises — `ConditionalContentTransform`,
`lua_format_for`, `llms_view_active` — lives inside `quarto-core`, so the lowest faithful tier for
all of it is `U`.

---

## Revert hunks for a verification-only plan

P8's scope is "verify that something already works." `ConditionalContentTransform` shipped as
part of the project-profiles epic (bd-fu16z22k). **P8 adds no production hunk of its own.** Every
revert hunk below is one of three things, and each test row says which:

- **`pre-existing`** — the hunk is already on `main`; the test is a *regression guard* over
  already-shipped behavior. This is most of P8's tests; they are itemized under
  **Pre-existing-hunk seams** at the end of this file.
- **`cross-plan`** — the hunk belongs to another plan (P1 Task 1, P7); the seam is written
  `seam deferred until <plan>'s <capability>`.
- **`none`** — no hunk anywhere reddens the assertion, because the behavior under test is a
  deliberate *absence*. Logged `accepted-untested` in the **Missing-test pass**, never dressed up
  as bound.

**The consequence an implementer must internalize:** for a `pre-existing` row, "run the test and
watch it fail first" cannot be performed as written — the test passes on the first run because
the behavior is already there. The substitute, mandatory: **temporarily revert the named hunk,
observe the named assertion go RED, restore the hunk.**

---

## The discriminating input (what makes a `when-format` fixture non-vacuous for a Pandoc target)

Design doc §6's `ConditionalContentTransform` row asserts the transform is already correct for a
genuine Pandoc `target_format` string with no further work. A fixture proves nothing unless its
expected value differs between the correct state and the plausible-incorrect states.

**The two functions in the path.** `ConditionalContentTransform::transform`
(`conditional_content.rs:141-172`) computes `lua_format`, once, at `:142`:

```rust
let lua_format = crate::format::lua_format_for(&ctx.format.target_format).to_string();
```

`lua_format_for` (`format.rs:164-170`) is a three-arm match: two preview pseudo-format arms, then
`other => other`. Format matching then goes through `check_format`
(`conditional_content.rs:334-342`) → `pampa::lua::quarto_doc::is_format_match`
(`crates/pampa/src/lua/quarto_doc.rs:74-89`), an exact-match early return (`:76-78`) followed by a
`match query` over six alias families (`html`, `html:js`, `latex`|`pdf`, `epub`, `markdown`,
`asciidoc`|`asciidoctor`) and `_ => false` (`:87`).

**Measured truth table:**

| `target_format` | `lua_format_for` → | query | match? |
|---|---|---|---|
| `docx` | `docx` | `docx` | **true** |
| `docx` | `docx` | `html` | false |
| `docx` | `docx` | `pdf` / `latex` | false |
| `pptx` | `pptx` | `pptx` | **true** |
| `pptx` | `pptx` | `docx` | false |
| `latex` | `latex` | `pdf` | **true** |
| `beamer` | `beamer` | `pdf` | **true** |
| `docx` | `docx` | `docs` (typo) | false, silently |
| `acm-docx` | `acm-docx` | `docx` | **false** |
| `acm-html` | `acm-html` | `html` | **false** |

**The claim is true, but only for a *bare* Pandoc writer name.** For `docx`, `pptx`, `latex` the
passthrough is correct. For an **extension format** (`acm-docx`, `my-journal-docx` — both of which
`Format::from_format_string` accepts via `format.rs:418-429`, producing `identifier = Docx` but
`target_format = "acm-docx"`) `lua_format_for` passes the un-canonicalized string through and
every `when-format` query fails. `Format::lua_format()` (`format.rs:383-389`) does canonicalize
(`self.identifier.as_str()`); the transform calls the string-only one instead. This gap is
pre-existing (not Pandoc-specific — the identical mechanism breaks the HTML leg for extension HTML
formats too) and is filed outside this epic as `bd-n5hjadpr`. Every fixture in this file uses a
bare writer name; an extension-format fixture would be exercising that strand, not P8.

### The discriminating fixture, stated

A fixture that only asserts `when-format="html"` content is **kept** under an HTML render is
vacuous. The discriminating shape is the **four-quadrant matrix under a Pandoc target**:

| marker | condition | `target_format = "docx"` | what a wrong state does |
|---|---|---|---|
| `.content-visible` | `when-format="docx"` | **survives** | RED if format is hardcoded `"html"`, or if `lua_format_for` gains a rewriting arm |
| `.content-visible` | `when-format="html"` | **dropped** | RED if format is hardcoded `"html"` |
| `.content-hidden` | `when-format="docx"` | **dropped** | RED if format is hardcoded `"html"` |
| `.content-hidden` | `when-format="html"` | **survives** | RED if format is hardcoded `"html"` |

Quadrants 2 and 3 are load-bearing: they are the only two whose expected value *inverts* between
"the Pandoc format string reached the predicate" and "it did not."

**One more seam, easy to miss.** `conditional_content.rs`'s existing `mod tests` drives the
`Walker` directly through a `run(blocks, format, active, meta)` helper (`:670-697`) that pushes
`format` straight into `ConditionEnv` — bypassing `lua_format_for` entirely. Every existing test,
and any new four-quadrant test written with that helper, would survive replacing `:142` with
`let lua_format = "html".to_string();`. **T1.4 exists solely to close that hole**, by driving
`ConditionalContentTransform::transform` with a real docx `Format`.

---

## Task 1: Verify `when-format` / `unless-format` against genuine Pandoc format strings

**Scope.** Prove design §6's claim by measurement, for `docx`, `pptx`, and `latex` — including the
`lua_format_for` leg the existing test helper bypasses. Pin the `unless-*` symmetry and the
unknown-format behavior while here.

**Files.** No production file changes. Test-only, all additions to existing `mod tests` blocks:
- `crates/quarto-core/src/transforms/conditional_content.rs` — new `#[test]`s in `mod tests`
  (`:628`); reuses the existing `div`/`attr`/`run`/`empty_meta`/`texts` helpers (`:635-707`).
- `crates/quarto-core/src/format.rs::tests` — extend the existing
  `test_lua_format_for_passes_through_real_formats` (`:895-900`, whose array already contains
  `"docx"`) with `"pptx"` and `"beamer"`.

Anchors:
- `conditional_content.rs:142` — the `lua_format_for(&ctx.format.target_format)` call.
- `conditional_content.rs:303-332` (`conditions_match`), `:313-318` (the six key arms),
  `:329` (`result = result && (invert != any)`), `:334-342` (`check_format`).
- `crates/pampa/src/lua/quarto_doc.rs:74-89` (`is_format_match`), `:76-78` (exact-match early
  return), `:87` (`_ => false`).
- `crates/quarto-core/src/pipeline.rs:1172` — `pipeline.push(Box::new(ConditionalContentTransform::new()))`,
  unconditional, first in Normalization, inside `build_transform_pipeline` (`:1144`).

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

**Prerequisite.** None. Startable today. The pptx *unit* rows (T1.5) need no `Format` at all —
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
  e.g. `other => "html"`⟩ → ⟨`assert_eq!(lua_format_for("pptx"), "pptx")`⟩ RED.
- **T1.2** — Revert ⟨`if format == query { return true; }`, `quarto_doc.rs:76-78`⟩ →
  ⟨`assert_eq!(blocks.len(), 1)` on the `.content-visible when-format="docx"` quadrant⟩ RED
  (`is_format_match("docx","docx")` then falls to `_ => false`, and the Div is dropped). Note the
  html quadrants stay GREEN under that revert — which is why all four quadrants are needed, not
  just quadrant 1.
- **T1.3** — Revert ⟨`"unless-format" => (true, Kind::Format)` to `(false, Kind::Format)`,
  `conditional_content.rs:317`⟩ → ⟨both assertions in T1.3 simultaneously⟩ RED (each flips).
- **T1.4** — Revert ⟨`conditional_content.rs:142` to `let lua_format = "html".to_string();`⟩ →
  ⟨T1.4's "docx content survives" assertion⟩ RED. **This is the single most important row in
  P8:** under that same revert, T1.2/T1.3/T1.5 all stay GREEN, because the `run` helper never
  calls `lua_format_for`. T1.4 is the only test in the plan that would notice.
- **T1.5** — as T1.2, with `"pptx"`.
- **T1.6** — Revert ⟨`_ => false`, `quarto_doc.rs:87`, to `_ => true`⟩ →
  ⟨`assert!(blocks.is_empty())`⟩ RED. The `diags.is_empty()` half has no revert hunk — logged
  `accepted-untested`.

### Refactor-induced vacuity check

- **The "already correct, no further work" claim.** Every T1.* test passes on `main` today, before
  P8 touches anything. What they are is a regression guard over three pre-existing hunks
  (`format.rs:168`, `quarto_doc.rs:76-78`, `conditional_content.rs:142`). The claim is false in one
  case: extension formats (see above). The other three probes came back clean — `lua_format_for`
  has no Pandoc branch and does not need one (bare passthrough is correct for
  `docx`/`pptx`/`latex`); `when-format: docx` resolves against the `target_format` string, which
  for a bare format equals both the `--to` string and the Pandoc writer name Q1 would put in
  `FORMAT`; and `unless-format` is handled symmetrically, one arm apart, at
  `conditional_content.rs:313-318`. The pptx-unresolvable issue at `Format::from_format_string`
  does not reach `lua_format_for`'s correctness — a `"pptx"` string maps correctly the moment one
  can be constructed; what it blocks is only the *E-tier* leg (Task 4), never the U-tier predicate.
- **Design §8's actual text.** §8 ("Cross-cutting decisions") says content-hidden gating is
  "deferred to P8, out of scope for docx/pptx v1" — a statement about *verification* being
  deferred, not about the feature not existing. No seam in this file asserts against the "feature
  is absent" framing; every row's expected value is the already-correct behavior.
- **The unknown-format expected value.** T1.6 asserts `"docs"` drops the content: correct =
  dropped, `_ => true` = kept, so the value differs across states. It does **not** distinguish "q2
  doesn't know `docs`" from "q2 knows `docs` and it doesn't match `docx`" — both drop. T1.6 is
  deliberately only a guard on the closed-world default, not on typo detection, and its name must
  say so.

---

## Task 2: Verify the resolved-Div unwrapping is format-independent and Pandoc-writer-safe

**Scope.** Confirm the unwrapping produces an AST shape Pandoc's docx/pptx writers accept, and
that `is_bare_wrapper` behaves identically across formats.

**Files.** No production file changes. Test-only:
- `crates/quarto-core/src/transforms/conditional_content.rs::tests` — T2.1.
- `crates/quarto-core/tests/integration/conditional_content_pandoc.rs` (**new**), registered
  `pub mod conditional_content_pandoc;` in `crates/quarto-core/tests/integration/main.rs`
  (alphabetized) — T2.2, T2.3.

Anchors:
- `conditional_content.rs:624-626` — `fn is_bare_wrapper(attr: &Attr) -> bool`. The function takes
  an `Attr` and nothing else — it cannot contain a format-dependent assumption because it never
  sees a format.
- `conditional_content.rs:394` (`splice = is_bare_wrapper(&div.attr)`), `:415-417` (the
  `BlockAction::Splice` return), `:372-377` (`Block::Div(div) => blocks.extend(div.content)`).
- `conditional_content.rs:578-610` (`strip_condition_attrs`), `:591`, `:609`.
- Q1 parity, at v1.11.3: `customnodes/content-hidden.lua:66` (`return el.content`), `:154` (the
  "only called on spans and codeblocks" comment), `:211-217` (`clearHiddenVisibleAttributes`).

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

**Prerequisite.** None for T2.1. T2.2/T2.3 need only `pandoc` on `PATH` — they deliberately do not
go through P4's transport or P7's tail, because the unit under test is the AST shape, and
inserting the whole Pandoc leg would make the lowest faithful tier much slower for no added
fidelity. A conditional-content-only fixture contains no `CustomNode`, so plain Pandoc JSON
suffices and P2's wire format is not needed either.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | U | `Walker` + `is_bare_wrapper` | one `.content-visible when-format=<fmt>` bare Div per format; `run(.., "html"/"docx"/"pptx", ..)` → in all three: `blocks.len()==1`, `!matches!(blocks[0], Block::Div(_))`, text present | none | **pre-existing** — `splice = is_bare_wrapper(&div.attr)`, `conditional_content.rs:394` |
| T2.2 | L | `Walker` → `pampa` Pandoc-JSON writer → real `pandoc -f json -t docx` | transform a fixture with one visible + one hidden conditional Div; assert the JSON has no empty-`Div` node; `Command::new("pandoc").args(["-f","json","-t","docx","-o",…])` → exit 0, output non-empty, `word/document.xml` contains `VISIBLE-SENTINEL`, does not contain `HIDDEN-SENTINEL` | real `pandoc` binary is the environment dep (not a mock); the unit under test — the transform + its AST output — is never mocked | **pre-existing** — the `BlockAction::Splice` arm `Block::Div(div) => blocks.extend(div.content)`, `conditional_content.rs:373` |
| T2.3 | L | same, `-t pptx` | as T2.2, asserted against the extracted `ppt/slides/*.xml` | same | same as T2.2 |

**Revert hunks, stated exactly:**

- **T2.1** — Revert ⟨`splice = is_bare_wrapper(&div.attr)` to `splice = false`,
  `conditional_content.rs:394`⟩ → ⟨`assert!(!matches!(blocks[0], Block::Div(_)))`, in all three
  format arms simultaneously⟩ RED. The three arms failing *together* is itself the evidence for
  format-independence.
- **T2.2 / T2.3** — Revert ⟨the `Block::Div(div) => blocks.extend(div.content)` line of the
  `BlockAction::Splice` match, `conditional_content.rs:373`, to `other => blocks.push(other)`⟩ →
  ⟨the "JSON contains no empty-`Div` node" assertion⟩ RED. Note the pandoc-exit-0 assertion does
  **not** go red under that revert — pandoc accepts an empty `Div` happily — which is precisely
  why the JSON-level assertion is required and why criterion 4 puts it *before* the invocation.

### Refactor-induced vacuity check

- **The positive control.** T2.2/T2.3 assert `HIDDEN-SENTINEL` is **absent** from the rendered
  docx. Absence is satisfied by an empty render, a failed render, a truncated unzip, or a sentinel
  string that was never in the fixture. Three positive controls are therefore mandatory in the
  same test: exit status 0, output file non-empty, and **`VISIBLE-SENTINEL` present**.
- **The "path was actually exercised" assertion.** T2.2/T2.3 must additionally assert the
  *pre-pandoc* JSON no longer contains the string `content-hidden` anywhere — establishing that
  the transform ran, rather than that the fixture never had a conditional Div.
- **Expected-value collapse across the three T2.1 arms.** All three arms assert the same value (no
  surviving `Div`). The discriminator for format-sensitivity lives in T1.2/T1.4 instead. T2.1's
  value is kept as shape/gating only, and its name must not promise format sensitivity.

---

## Task 3: Verify the cut-boundary invariants — no marker survives, and the llms view is inert

**Scope.** The structural contract P5's shim positioning depends on: that **no**
`.content-visible` / `.content-hidden` class and no `when-*` / `unless-*` attribute survives the
transform, anywhere in the AST. Plus the llms two-view path (orthogonal to the Pandoc tail).

**Files.** No production file changes. Test-only:
- `crates/quarto-core/src/transforms/conditional_content.rs::tests` — T3.1, T3.3.
- `crates/quarto-core/src/transforms/llms.rs::tests` — T3.2.

Anchors:
- `conditional_content.rs:591` (`attr.2.retain(|k, _| !CONDITION_KEYS.contains(…))`) and `:609`
  (`attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)`) — the two hunks the invariant
  rests on.
- `conditional_content.rs:106-113` — `CONDITION_KEYS`, all six.
- `conditional_content.rs:278-284` — the `if !self.env.llms_view { return … }` early return, a
  load-bearing safety branch T3.3 binds.
- `crates/quarto-core/src/transforms/llms.rs:122-125` — `llms_view_active`, whose second conjunct
  is `crate::format::lua_format_for(&ctx.format.target_format) == "html"`: for
  `target_format = "docx"` it is false, so the two-view path cannot activate for a Pandoc target
  regardless of project config.
- Q1 side: `customnodes/content-hidden.lua:29` (`ast_name = "ConditionalBlock"`, class-keyed on
  both markers) and `:136-138` (`content_hidden()` registering `CodeBlock` and `Span`, with `Div`
  commented out because the custom-node handler owns it). If a marker class survived the cut, both
  Q1 mechanisms would fire on it.

**Acceptance criterion.**
1. After the transform, a `texts()`/walk assertion over the whole block list finds **no** occurrence
   of `content-visible`, `content-hidden`, or any key starting `when-`/`unless-` — with the
   path-exercised assertions in criterion 2 satisfied in the same test (T3.1).
2. T3.1's input contains, and the assertions confirm the fate of, **all five** marker carriers:
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

**Prerequisite.** None. All three tests are startable today. T3.1 is the test P5's "no-op over an
empty set" argument depends on; it is written here rather than in P5 because it asserts a property
of Q2's transform, not of the shim.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | U | `Walker` + `strip_condition_attrs` | the five-carrier fixture from criterion 2; `run(.., "docx", ..)` → no marker class / condition attr anywhere, **and** all five per-carrier fate assertions | none | **pre-existing** — `attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)`, `conditional_content.rs:609` |
| T3.2 | U | `quarto_core::transforms::llms::llms_view_active` | build a website `RenderContext` with `llms-txt: true` and a docx `Format`; call → `false`. Control: the same context with an html `Format` → `true` | `ProjectContext`/`DocumentInfo` doubles | **pre-existing** — the `&& crate::format::lua_format_for(&ctx.format.target_format) == "html"` conjunct, `llms.rs:124` |
| T3.3 | U | `Walker` (via `run`, `llms_view = false`) | `.content-visible when-format="llms"` + `.content-hidden when-format="llms"` under `"docx"` → visible-when-llms dropped, hidden-when-llms kept, `!texts.contains("quarto-llms-")` | none | **pre-existing** — the `if !self.env.llms_view { return … }` early return, `conditional_content.rs:278-284` |

**Revert hunks, stated exactly:**

- **T3.1** — Revert ⟨`attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)`,
  `conditional_content.rs:609`⟩ → ⟨the "no `content-visible` class survives anywhere" assertion⟩
  RED. Crucially, the *removal* assertions (hidden Div's text gone) stay GREEN under that revert —
  the discriminator is the surviving element's class list, not the document's content. A second
  revert to check: ⟨`attr.2.retain(|k, _| !CONDITION_KEYS.contains(…))`, `:591`⟩ → ⟨the "no
  `when-*` attribute survives" assertion⟩ RED.
- **T3.2** — Revert ⟨the format conjunct at `llms.rs:124`, leaving only
  `llms_companions_enabled(meta, ctx)`⟩ → ⟨`assert!(!llms_view_active(&meta, &docx_ctx))`⟩ RED.
  The html control assertion stays GREEN, which is what makes the docx assertion load-bearing
  rather than tautological.
- **T3.3** — Revert ⟨the `if !self.env.llms_view { return … }` early return,
  `conditional_content.rs:278-284`, so the four-quadrant llms evaluation always runs⟩ →
  ⟨`assert!(!texts(&blocks).contains("quarto-llms-"))`⟩ RED — an early-return safety branch.

### Refactor-induced vacuity check

- **The no-op-over-an-empty-set argument (P5).** P5's claim is that Q1's own
  `content-hidden.lua` / `quarto-pre/hidden.lua` pass "runs, matches zero Divs, no-ops. Not a
  collision; a no-op over an empty set." An assertion of the form "no `.content-hidden` Div
  survives" is satisfied by a document that never had one. T3.1 is specified with criterion 2's
  five per-carrier fate assertions precisely so the path is proven exercised: a `.content-hidden`
  Div **was** present and **was** resolved, a `.content-visible` Div **did** survive with its
  content, and the Span / CodeBlock / nested-custom carriers each get their own assertion so no
  single recursion arm can silently stop being visited.
- **`ConditionalBlock`'s class-keyed registration.** `ConditionalBlock` (Q1
  `customnodes/content-hidden.lua:29`) is among the class-keyed handler registrations that would
  collide with the wire wrapper if the shim were mispositioned. Under the frozen design they do
  not collide, because the transform resolves them pre-cut. T3.1 is the assertion of that
  invariant — its expected value ("no marker class anywhere") still differs across the states it
  distinguishes (`:609` present vs. reverted).
- **The llms expected value.** `llms_conditions_inert_without_llms_view`
  (`conditional_content.rs:975-989`) already asserts llms-inertness — but with `format = "html"`
  and `llms_view = false`, i.e. it distinguishes the flag, not the format. T3.3 changes the format
  to `"docx"`, justified as the Pandoc-target instance of the same contract — **T3.2 is** the
  answer to the format-gate question specifically, since T3.2's subject is the format gate itself.

---

## Task 4: docx/pptx `.content-visible` / `.content-hidden` smoke fixture

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

**Prerequisite (both legs).** **P7-foundation's Task 3** (the `render.rs:680-685` gate relaxation +
`render_qmd_to_pandoc` routing) **and P7's Task 4** (the per-format invocation builder). Not
startable until both land. The pptx leg does **not** additionally require P1 Task 1
(`FormatIdentifier::Pptx`) — it has landed: `FormatIdentifier` (`format.rs:23-40`) has a `Pptx`
variant with a `"pptx" => Ok(FormatIdentifier::Pptx)` arm in `TryFrom<&str>` (`format.rs:92`),
so `Format::from_format_string("pptx")` succeeds; P8's own T2.3 integration test exercises this.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | E | the real `q2` binary, docx leg | `cargo run --bin q2 -- render <fixture>.qmd --to docx`; unzip → `word/document.xml` sentinel assertions per criteria 1–2 | none — the whole binary is the unit | **cross-plan** — `seam deferred until P7-foundation's Task 3` (the `render.rs:680-685` relaxation) **and** P7's Task 4 (the docx invocation builder) |
| T4.2 | E | the real `q2` binary, pptx leg | as T4.1 with `--to pptx`, asserted against `ppt/slides/*.xml` | none | **cross-plan** — `seam deferred until P7-foundation's Task 3` **and** P7's Task 4 only. Not additionally blocked by P1 Task 1's `TryFrom` arm — it has landed. |

**Revert hunks, stated exactly:**

- **T4.1** — `seam deferred until P7-foundation's Task 3 and P7's Task 4`. No hunk in P8 or on
  `main` today reddens it; there is no docx render path to revert. Once both land, the honest hunk
  is P7-foundation's `render.rs:680-685` relaxation, and the assertion that discriminates is
  criterion 2's **`DOCX-ONLY-SENTINEL` present** (the positive control), not the absence
  assertion.
- **T4.2** — `seam deferred until P7-foundation's Task 3 and P7's Task 4` only. (Originally noted
  as "additionally RED today for an unrelated reason (P1 Task 1's `TryFrom` arm)" — that reason
  no longer applies as of 2026-09-20; P1 Task 1 landed 2026-09-18 and T2.3 already exercises
  `Format::from_format_string("pptx")` successfully.)

**Gate and skip policy.** T4.1/T4.2 are **not** written now and **not** committed as `#[ignore]`.
An `#[ignore]`d or env-var-skipped E-tier test would report green in CI while asserting nothing.
Instead this task stays an unchecked item on P8's checklist, visible by inspection. When
P7-foundation's Task 3 and P7's Task 4 both land, the tests are written unguarded: they either run
and assert, or the suite is red.

### Refactor-induced vacuity check

- **The smoke fixture's expected value is an absence, and absence is cheap.**
  `NOT-IN-DOCX-SENTINEL` is absent from an empty `.docx`, from a zero-byte file, from a render
  that errored after creating the output path, and from an unzip that read the wrong member. The
  fixture is therefore specified with **three positive controls inside the same test** —
  exit 0, `ALWAYS-SENTINEL` present (the transform-independent control), and
  `DOCX-ONLY-SENTINEL` present (the feature's own positive control). `ALWAYS-SENTINEL` would
  survive even if `ConditionalContentTransform` never ran.
- **Cross-format discrimination.** Asserting `PPTX-ONLY-SENTINEL` absent from the docx output and
  `DOCX-ONLY-SENTINEL` absent from the pptx output is what makes the pair non-vacuous: a
  single-format fixture cannot distinguish "the format string reached the predicate" from "every
  conditional block was kept." This is the E-tier restatement of the four-quadrant matrix.

---

## Missing-test pass

Reasoned across P8's load-bearing branches. Every one gets a bound seam or an explicit
`accepted-untested`.

1. **`when-format` naming a format q2 doesn't know.** `is_format_match` (`quarto_doc.rs:74-89`)
   closes its world at `_ => false` (`:87`). An unrecognized query is silently treated as
   non-matching: no early return, no warning. `.content-visible when-format="docs"` drops its
   content with no diagnostic. `verdict()`'s existing Q-2-42 warning
   (`conditional_content.rs:249-267`) fires on unknown attribute *keys* (`when-profil`), never on
   unknown attribute *values*.
   → **Drop behavior: BOUND — T1.6**, revert `quarto_doc.rs:87`.
   → **Zero-diagnostic behavior: `accepted-untested`** — no production hunk emits a diagnostic
   here, so no revert can redden an `assert!(diags.is_empty())`. The assertion is kept in T1.6 as
   a change-detector on a deliberate Q1-parity choice (Q1's `isFormat` likewise ends in
   `else return false`, `pandoc/datadir/_format.lua:284-286`), and is labeled in the test's own
   comment as unbound. Whether q2's existing Q-2-42 strictness divergence should extend to unknown
   format *values* is a design question, not P8's to answer.
2. **`unless-format`'s symmetry with `when-format`.** One arm apart at
   `conditional_content.rs:313-318`, sharing `invert != any` at `:329`.
   → **BOUND — T1.3**, revert the `"unless-format" => (true, Kind::Format)` arm at `:317`.
3. **The `ConditionalBlock` class-keyed collision.** Under the frozen design no content-hidden
   type reaches the cut, because the transform resolves them all pre-cut. That invariant was
   asserted nowhere in the tree before this file — neither P5 nor P8 held a test for it, and P5's
   shim-positioning argument depends on it.
   → **BOUND — T3.1**, revert `conditional_content.rs:609` (and `:591` for the attribute half).
   This is the single highest-value new test in P8.
4. **The pptx leg specifically.** `FormatIdentifier` (`format.rs:23-40`) has a `Pptx` variant with
   a `"pptx" => Ok(FormatIdentifier::Pptx)` arm in `TryFrom<&str>` (`format.rs:92`), so
   `from_format_string("pptx")` succeeds via that arm without ever consulting
   `KNOWN_BASE_FORMATS` (`discover.rs:241-250`, which still omits `"pptx"` but only governs
   extension-style parsing like `"acm-pptx"` — unrelated to bare `"pptx"`). The `when-format`
   predicate does not depend on that resolution either way — `lua_format_for("pptx")` and
   `is_format_match("pptx","pptx")` both work on the bare string (measured).
   → **U-tier pptx: BOUND and done — T1.5, T2.3.**
   → **E-tier pptx: `cross-plan` — blocked on P7-foundation's Task 3 *and* P7's Task 4 only —
   T4.2.** P1 Task 1 is not a blocker for T4.2. P8's pptx coverage is not blocked wholesale; only
   its end-to-end leg is, and only on the P7 side.
5. **The deferral itself.** §8 says content-hidden / `when-format` gating is "deferred to P8, out
   of scope for docx/pptx v1," while P8 exists to verify it works. Reconciled: **P8 verifies the
   feature works for Pandoc targets; docx/pptx v1 does not promise it.** The verification is real
   (Tasks 1–3 measure it, and the answer is "it works for a bare Pandoc writer name"); the
   *promise* is what is deferred. Design doc §12 documents this: the behavior is verified but not
   promised or documented for v1, and release notes should not claim it. The extension-format case
   (`acm-docx`) is genuinely broken, not merely unclaimed — tracked as `bd-n5hjadpr`.
   → No test. Documentation-level.
6. **`Inline::Custom` recursion.** `keep_block` has a deliberate `Block::Custom` arm recursing into
   every slot (`conditional_content.rs:501-518`); `keep_inline` (`:528-569`) has no counterpart for
   `Inline::Custom` (`quarto-pandoc-types/src/inline.rs:53`, live constructors `Equation`,
   `CrossrefResolvedRef`) and falls through `_ => {}`.
   → **`accepted-untested`** — a marker Span nested inside an inline CustomNode's slot is never
   visited; it keeps its marker class and condition attributes past the cut, where Q1's own
   `content_hidden()` Span filter (`content-hidden.lua:136-138`) does fire on it — the one hole in
   P5's no-op-over-an-empty-set argument. Almost certainly unreachable today, because
   `ConditionalContentTransform` runs first in Normalization (`pipeline.rs:1172`), before any sugar
   transform creates a CustomNode — but the `Block::Custom` arm exists anyway, so the asymmetry is
   unexplained. See **Open questions** below.
7. **`when-meta` / `unless-meta` under a Pandoc target.** Format-independent by construction
   (`check_meta`, `conditional_content.rs:350-361`, reads only `self.env.meta`), covered for
   `"html"` by the existing `meta_truthiness` test (`:776-800`).
   → **`accepted-untested`** — no format dependency exists in the branch; a docx instance would
   discriminate nothing.
8. **`when-profile` / `unless-profile` under a Pandoc target.** Same argument — `check_profile`
   (`:344-346`) reads only `active_profiles`. Covered for `"html"` at `:710-719`.
   → **`accepted-untested`** — format-independent by construction.

---

## Open questions

- **`keep_inline` has no `Inline::Custom` arm, asymmetrically with `keep_block`'s deliberate
  `Block::Custom` arm.** `conditional_content.rs:501-518` recurses into every slot of a
  `Block::Custom`; `:528-569` has no counterpart for `Inline::Custom`
  (`quarto-pandoc-types/src/inline.rs:53`), which falls through `_ => {}`. A marker Span inside an
  inline CustomNode slot would keep its marker class past the cut, where Q1's `content_hidden()`
  Span handler (`customnodes/content-hidden.lua:136-138`) fires on it — the one hole in P5's
  "no-op over an empty set." Almost certainly unreachable, since the transform runs first in
  Normalization before any CustomNode is built — but that argument applies equally to
  `Block::Custom`, whose arm exists anyway. Deciding whether to add the arm is a design decision
  about the transform's contract, not a verification; logged `accepted-untested` in the
  Missing-test pass above until decided.

## Known divergences from Q1 (out of v1 scope; not P8's to fix)

- **The alias table.** q2's `is_format_match` (`quarto_doc.rs:80-88`) is a proper subset of Q1's
  `isFormat` (`pandoc/datadir/_format.lua:244-287`): Q1 additionally handles `odt`/`opendocument`
  synonyms, matches any query containing `epub` via the Lua pattern `to:match 'epub'`, and
  recognizes `confluence`, `docusaurus`/`docusaurus-md`, `email`, `dashboard`, `hugo`/`hugo-md`. So
  e.g. `when-format="opendocument"` under `--to odt` and `when-format="epub3"` under `--to epub2`
  are both false in q2, true in Q1 (measured). All outside v1's docx/pptx/latex scope. Tracked
  under `bd-n5hjadpr`, whose documented contract to implement against is quarto-web's
  `docs/authoring/_format-aliases.md` (whose table is slightly wider than the `isFormat` source
  alone suggests: `html` additionally aliases `dashboard` and `email`; `markdown` additionally
  aliases `hugo-md` and `docusaurus-md`).
- **The extension-format grammar.** Q1's format-string grammar is
  `baseName+<variants | modifiers>-<variants>[<extension>]`, with modifiers and variants in any
  order. q2's `parse_format_descriptor` (`extension/discover.rs:252-271`) implements only
  `<extension>-<base>` against an 8-entry `KNOWN_BASE_FORMATS` (`:241-250`), so `gfm-raw_html` and
  `acm-pdf+foo` are rejected outright rather than mismatched. Pandoc strips `+EXT`/`-EXT` before
  setting `FORMAT` (measured against pandoc 3.8.1), so the `+`/`-` half of the grammar does not
  affect format *matching* in either implementation — only parsing.
- **q2 strips more condition-attribute keys than Q1.** Q1's `clearHiddenVisibleAttributes`
  (`content-hidden.lua:211-217`) clears `when-format`/`unless-format`/`when-profile`/`unless-profile`
  and the two marker classes — it does not clear `when-meta`/`unless-meta`. q2 strips all six
  `CONDITION_KEYS` (`conditional_content.rs:106-113`, `:591`) — the better behavior, and harmless
  for docx/pptx, but a real divergence a byte-parity golden over Q1's markdown or HTML writer would
  surface.
