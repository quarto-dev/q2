---
title: 'EXR-2: one shared scanner for execution units (cells and inline expressions)'
date: 2026-10-28
description: 'Replaces the divergent fence and inline-span regexes with one scanner that returns cells and inline expressions in document order (ownership is applied per engine afterwards, not by the scanner), moves the non-legacy helpers out of the legacy splice module, and owns the kernel-per-unit seam (the KernelRouter) that the Jupyter owned-languages plans fill in.'
status: draft
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** nothing hard. Soft: JOL-1 (each engine's `owns` rule can start as a
closure over today's rules and switch to `ctx.owned_languages` later).
**Blocks:** EXR-3, EXR-4 (unit ids), EXR-12. It does not wait for JOL-2a or JOL-2b:
it defines the kernel seam with today\'s behavior as the default, and those plans fill
the seam in, in either order. Per the dependency table in the epic, it also blocks EXR-7, EXR-8, EXR-11, EXR-14, EXR-16, JOL-3.
**Lane:** A (`workspace-4`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2-3.

> **For implementers:** TDD. Characterize today's behavior first (Phase 0); the
> executor must behave identically until a phase explicitly changes it.

**Sequencing with EXR-1 (epic decision 37).** Both lanes start now, so this is a phase-order
gate, not a plan-level edge (the epic dependency table is unchanged). EXR-1 Phase 3 (the move
of the presenter functions) lands **before** Phases 1, 4 and 5 here, because both plans edit
the same hunks of `text_execute.rs` (`parse_code_blocks`, the imports, `execute_blocks_inner`)
and of `stored.rs`. Phases 0, 2 and 3 here do not collide and may run in parallel with EXR-1.
EXR-1's trailing Phase 5 (the replay-engine move) waits for Phase 4 here, because Phase 4
replaces the `text_execute.rs` imports that `stored.rs` uses.

## Goal

A single function that takes serialized qmd text and returns the document's
**units** in order: braced-fence cells and brace-spelling
inline expressions. Every consumer that today scans for cells or spans (the
Jupyter executor, stored-notebook replay, the nested-cell mask, later the knitr
inline pass and the record writer) uses the same patterns and the same notion of
"gap". Unit identity is the largest risk in the epic (risk 1); this plan removes
the easiest source of disagreement.

It also owns the `kernel_for(unit)` seam (decided with Gordon, 2026-10-29): this plan
defines the `KernelRouter` trait and makes the unit loop call it, with today\'s
first-cell behavior as the default router. JOL-2a (`select_kernel`) and JOL-2b (kernelspec
lookup by language, venv-first, the `jupyter:` key) supply the real policy behind it, and
a reopened JOL-3 (deferred) could make it vary per unit, so none of them needs a second
rewrite of the loop. Production uses one kernel per document. It
also moves `engine_cell_lang` and the other helpers that are not part of
the legacy structural splice out of `capture_splice.rs`, so that module can be
deleted later (EXR-17) without touching its callers.

Two more roles, from the review. The scanner reports the structural facts that make the
legacy executor skip a cell (indentation, container prefix, fence length, attribute tail),
and holds today's executor rule in one quarantined function, so the executor needs no scanner
of its own. And units are scanned **once**, in the engine-execution stage, on the text
entering the stage before any engine runs, and the same list goes to every engine (epic
decision 38); this plan provides the `Unit` type with a slot for the id that EXR-4's function
fills, and computes no hash itself.

## Background (verified 2026-10-28, workspace-1 at `2ee0bd966`; items marked 2026-10-29 were re-verified in the review)

There are at least seven scanners, none sharing a definition.

- **Jupyter executor** (`engine/jupyter/text_execute.rs:128`): ```` (?m)^```\s*\{(\w+)\}\s*\n([\s\S]*?)^```\s*$ ````.
  Line-start only, no indentation, fence length not modeled, `\w` is
  Unicode-aware, bare `{lang}` only.
- **Nested-cell mask, openers** (`engine/nested_cell_mask.rs:50`): `MASK_OPENER_RE`,
  with an ASCII language class `[A-Za-z0-9_]+` followed by a rest group
  `[^}\r\n]*`. It accepts indentation, three or more backticks and trailing
  options: a deliberate superset (see its doc comment).
- **Nested-cell mask, inline spans** (`nested_cell_mask.rs:123`): `MASK_INLINE_RE`,
  separator class `[ \t#]`, both the brace and the classic `r` spelling. It
  applies only inside *display* code blocks (`is_in_scope_for_masking`, `:253`)
  and must be a superset of every span scanner.
- **knitr inline pass** (`engine/knitr/preprocess.rs:202`): `INLINE_R_PATTERN`,
  `r` only, with an extra backslash guard.
- **Resolver language presence** (`engine/resolution.rs:128`): an AST walk
  (`walk_block_for_langs`) using `capture_splice::engine_cell_lang`
  (`capture_splice.rs:86`). It recurses into blockquotes, divs, figures and lists,
  takes any braced class without inner braces, skips `HANDLED_LANGUAGES`, and
  sees no inline spans. That is deliberate and unchanged (decision 6).
- **Julia runner** (`external-sources/QuartoNotebookRunner.jl/src/evaluation.jl:487`,
  the span marker at `:488`): CommonMark inline code nodes starting with the julia, python or r brace
  spelling. Spans inside fenced blocks are never seen, which differs from Q1's regex.
  Its cell recognizer `toplevel_language` (`parsing.jl:48-57`, verified 2026-10-29)
  accepts exactly the info strings `{julia}`, `{python}` and `{r}`, and only for a
  block whose parent is the document, so it never sees a cell nested in a
  blockquote or list. EXR-8 deals with that.
- **Positron** (`external-sources/positron/src/vs/workbench/contrib/positronQuarto/common/quartoParser.ts:21`,
  verified 2026-10-29): ``^```\{(?<language>\w+)(?<options>[^}]*)\}\s*$``. JS `\w` is
  ASCII; exactly three backticks, no indentation; anything after the word goes
  into `options`, so Positron reads `{python.marimo}` as language `python` with
  options `.marimo`. Raw blocks (`{=format}`) are matched separately and are not cells.
- **TypeScript fence and span scanners** (`ts-packages/quarto-api/src/markdownRegex/index.ts`:
  `breakQuartoMd`, `languagesWithClasses`; used by marimo). They also find fences or
  spans, so they belong on this list and in the conformance corpus. Reviewer-reported,
  not yet verified by running them.

**Q1** builds its span regex per notebook language
(`external-sources/quarto-cli/src/core/execute-inline.ts:17`:
``(^|[^`])`{lang}[ \t]([^`]+)` ``). Q1's fence scan for language presence is
`languagesWithClasses` (`external-sources/quarto-cli/src/core/pandoc/pandoc-partition.ts:144-166`,
verified 2026-10-29): ``^[\t >]*```+\s*\{([a-zA-Z][a-zA-Z0-9_.]*)([^}]*)?\}\s*$``,
lowercased, with the first class taken from the attribute tail. It scans fences only,
never spans.

**What pampa accepts** (verified 2026-10-29 by rendering one-case files with
`trace: true` under `q2` and reading the code-block classes):

| Fence | pampa result |
|---|---|
| `{python.marimo}` | one class `{python.marimo}`; `engine_cell_lang` returns `python.marimo` |
| `{python .marimo}` | classes `{python}` and `marimo`: language `python`, first class `marimo` |
| `{Python}` | case preserved |
| `{py-thon}`, `{py.thon}` | accepted as one token |
| `{python echo=false}` | rejected, Q-2-36 |
| `{1abc}`, `{_x}` | rejected, Q-2-36 |
| `{é}` | parse error |

So options inside the braces do not exist in valid q2 documents, the dotted form is
one language token (the marimo claim table has `python.marimo`, as in Q1), and a
language starts with a letter. A span is an inline `Code` node whose text is the
language in braces, a space, then the expression; the escaped form with doubled
braces stays literal text.

**Consequences already visible in the code:**

- The executor and the resolver can disagree about a cell nested in a list or
  blockquote: the resolver sees it (AST), the executor's line-start regex does not.
  Verified by running `pampa -t qmd`: the writer indents list-nested fences and prefixes
  blockquote cells with `> `, so the executor never sees either. Div-nested cells are not
  indented and do run. Phase 0 turns this into an assertion.
- **Quirks of the executor regex** (run in Rust against
  ``(?m)^```\s*\{(\w+)\}\s*\n([\s\S]*?)^```\s*$``, `text_execute.rs:128`):
  the match end swallows blank lines after the closing fence (`"```\n\n\nText"` leaves
  `"\nText\n"`); `\s*` between the fence and the brace spans newlines, so
  `"```\n{python}\nx=1\n```\n"` is one cell (a closing fence followed by a `{python}` line
  starts a cell); with CRLF the match ends before `\n` and `\r` stays in the gap; `\w` is
  Unicode, so `{é}` runs in the executor and is rejected by pampa; and four or more
  backticks, indentation, list or blockquote nesting and attribute-tail cells are all
  skipped. Phase 0 pins each.
- Pre-existing facts: a `{{python}}` span gives warning Q-2-50 and literal code. The error
  catalog titles Q-2-35 "Invalid List-Table Structure" (`error_catalog.json:408`) while
  pampa emits "\[Q-2-35\] Indented code blocks are not supported" for a four-space fence
  (`pampa/resources/error-corpus/Q-2-35.json`); epic decision 26 is right at runtime
  (Phase 1 reconciles the title).
- The mask runs before serialization (`engine_execution.rs:515-537`) and unmask
  after (`:623`), so display-block examples never reach any scanner.
  The mask must therefore accept everything a scanner accepts.
- The executor derives one kernel from the first executable cell
  (`text_execute.rs:76`) and `execute_blocks_inner` (`:262`) loops over blocks with a
  single session key.
- `engine_cell_lang` is used outside the legacy splice by the resolver
  (`resolution.rs:136`), `nested_cell_mask.rs` (`:259`), `stage/stages/lmd_desugar.rs`
  (`:29`), `unexecuted_cell_count.rs` (`:13`) and `lmd/emit.rs` (`:48`, `:236`), as well as
  by `capture_splice.rs` itself (`:120`, `:256`, `:395`), but it lives in
  `capture_splice.rs`, the module EXR-17 deletes. Two integration tests import it from the
  old public path `quarto_core::engine::capture_splice::engine_cell_lang`
  (`tests/integration/capture_splice_seam.rs:36,60` and `capture_splice_engines.rs:26,162`).
- The mask's inline pattern `MASK_INLINE_RE` (`nested_cell_mask.rs:123`) accepts only
  `[A-Za-z0-9_]+` inside the braces, so spans like `{python.marimo}` or `{py-thon}` are not
  masked inside display blocks, though the scanner (decision 5) claims them.

## Design decisions (from the epic and the review; do not re-litigate)

1. One scanner crate-internal module, regex only, target-neutral (it must run in
   WASM for preview and hub).
2. Spans are found only in the **gaps** between braced fences, including gaps
   around ceded and display-only blocks, so spans in unrun blocks are never
   claimed. Spans inside *unbraced* fences whose info string names a language (` ```text `, ` ```r `) are still claimed and flagged `in_fence` (Q1 and docs parity). Bare class-less and ` ```markdown ` fences are masked before the scan (`is_in_scope_for_masking`, `nested_cell_mask.rs:253-260`, `MASK_INLINE_RE` at `:123`), so the scanner never sees their spans and they stay displayed (settled arbitrarily, Gordon 2026-10-31: revisit if wrong).
   A span inside a braced fence is never a span (q1's accidental evaluation of one is a
   deliberate departure, EXR-3 divergence 8). A span inside an HTML comment is not claimed
   (EXR-3 divergence 10), while a span in the document's own front matter is (q1 evaluates
   it; decision 11 says which front matter counts). **Brace spelling only (Gordon,
   2026-10-31):** the shared span regex is the only definition of a *brace* span; the classic
   `` `r expr` `` spelling stays knitr's private business and is never a unit, so it has no id
   and takes no counter number (EXR-12 rewrites it without recording it).
3. The scanner takes no ownership predicate and never embeds a language list. It returns
   every unit with its language token; each engine applies its own `owns` rule to that list
   in its view (epic decision 42). Ownership is not a property of a unit.
4. Locations use `SourceInfo::substring` so errors can say `file.qmd:12:34`.
5. **One language-token rule.** The token is `[A-Za-z][A-Za-z0-9_.-]*`, compared
   case-insensitively (lowercase, as Q1 does), followed by an optional attribute tail
   after whitespace (`{python .marimo}`). There are no options inside the braces.
   The dotted form (`python.marimo`) is one token; the spaced form is language
   `python` plus first class `marimo`. **Pampa is the authority:** the scanner accepts
   at least every token pampa accepts, so the resolver and the scanner cannot
   disagree about a cell. The mask and the scanner import the same constants; the
   mask may stay a superset on the first character. `MASK_INLINE_RE` is widened to the
   shared token class (a deliberate behavior change in the mask, consistent with epic
   decision 13). The hyphen is in the class
   because pampa accepts it (Q1 does not); confirmed with Gordon, 2026-10-30. This replaces the earlier narrowing to `[A-Za-z0-9_]+`.
6. **The resolver does not see spans.** A document with inline expressions but no
   executable cell still resolves to the markdown engine, as in Q1; the workaround is
   an engine-selecting front matter key (`jupyter: python3`, `engine: jupyter`).
   No dependency edge from this scanner into the resolver or JOL-1 is needed.
   Whether to change that later is strand bd-my5fqvok.
7. **Ids.** The scanner returns every braced non-raw fence, with a
   **document-wide cell index** (raw `{=format}` blocks are excluded, as in Positron),
   so cell indices agree across engines in a multi-engine document (confirmed with
   Gordon, 2026-10-30). Ids match Positron's in format and content hash only; the label
   segment and the index can differ (epic decision 39). Spans carry **their own counter**,
   document-wide over **all** spans (like cells), and every span is returned, so adding a
   span never shifts a cell index, a count never collides when records coalesce and
   `span_index` has no gaps. Matching never uses the span index.
8. **Units are scanned once, in the stage (epic decision 38).** In a multi-engine run the
   second engine would receive the text after the first engine's reconciliation, where the
   first engine's fences are gone, so a per-engine scan would number its cells differently.
   The engine-execution stage scans **once**, on the text entering the stage before any
   engine runs, and passes the same list to every engine through `ExecutionContext.units`.
   The stage applies **no ownership filter** (epic decision 42, the one scan filters nothing
   and each engine's view marks its own units): the list holds every cell and every span,
   whatever its language. The stage passes **only that shared list**
   (`ExecutionContext.units`); it cannot build an engine's view, because `ExecutionEngine`
   has no `owns` method and Jupyter's rule needs the kernel selected for the document. Each
   engine calls `view_for_engine(ctx.units, owns)` itself with its own rule (Jupyter:
   `ctx.owned_languages` plus the selected kernel's language; `TsEngine::build_execute_options`
   computes the wire `owned` from `ctx.owned_languages`) and validates counts for the units it
   owns only. A span no engine owns stays in the list, is owned by no view and is left as
   literal text. Because the scan needs no resolver output, the preview splice (EXR-14) can
   call it too.
   **Relocation.** The stage re-masks and re-serializes the AST for every engine
   (`engine_execution.rs:505-537`), so engine 2 receives different text from engine 1 and the
   offsets from the one shared scan are wrong for it (`[knitr, jupyter]` is a real
   configuration, `text_execute.rs:1113`). `locate_units` (Phase 4) re-derives `start`, `end`,
   `code_start` and `line` in the text an engine receives and keeps the shared `id`, `index`
   and `span_index`. `Unit` carries a typed id slot that EXR-4's id function fills; this plan
   computes no hash.
9. **The scanner reports what the legacy executor skips (epic decision 40).** `CellUnit`
   carries `indent`, `container_prefix`, `fence_len` and `attr_tail`. A pure function
   `runs_in_legacy_executor(&CellUnit) -> bool` holds today's executor rule in one
   quarantined place, deleted when the legacy rule goes. The executor then needs no scanner
   of its own (Phase 4 proves it).
10. **Helpers that are not legacy move out early.** `engine_cell_lang` and any other
   helper in `capture_splice.rs` that the legacy structural splice does not need move
   to a module that stays, with their callers updated, in this plan (EXR-17 then only
   verifies it). `is_echo_of` and the output-map code are legacy and stay.
11. **Only the document's own front matter is scanned for spans (Gordon, 2026-10-31;
   refines epic decision 41, which says spans are claimed in prose and in unbraced fences that name a language, and
   that a front-matter span is evaluated).** The metadata the stage serializes is the
   *merged* map: project (`_quarto.yml`), `_metadata.yml`, extension and format-default
   values sit in the same front matter block. q1 evaluates only the document's own front
   matter, so a span in a project value stays literal, takes no counter number and passes
   through the engine unchanged. Provenance decides. Every merged `ConfigValue` carries a
   `source_info`, and `metadata_merge.rs:212-240` registers the project and `_metadata.yml`
   files under hash-based FileIds so a value maps back to the file it was written in. A span
   is claimed only when its value's `SourceInfo` root file id is the document's own file; a
   value with no root file id (synthesized values such as `rendered:` and `listing-item:`)
   counts as not the document's. `scan_units` is text-only and cannot see provenance, so the
   stage enforces the rule on the clone it serializes (the same clone-and-restore pattern as
   the nested-cell mask): it masks the spans of non-document values before the one scan, so
   they are not units, and restores them after execution. Spans the document owns take ids
   and counter numbers but are never spliceable (EXR-14). UNVERIFIED: which `source_info`
   the synthesized, format-default and extension values carry; the first Phase 2 provenance
   test settles it. If masking proves awkward, the alternative is to have the scanner tag
   front-matter spans with their key path and let the stage drop foreign ones; pick one in
   Phase 2 and record it here.

## Out of scope

- Unit ids and hashes (EXR-4); executing spans (EXR-3); making the resolver see spans
  (decision 6); the multi-kernel router itself (JOL-3, deferred).

## Proposed API

```rust
pub enum Unit { Cell(CellUnit), Inline(InlineUnit) }

pub struct CellUnit {
    pub start: usize, pub end: usize, pub code_start: usize,
    pub lang: String,      // the full token as written, e.g. "python.marimo"
    pub code: String,
    pub index: usize,      // document-wide: every braced non-raw fence
    pub indent: usize,           // leading spaces before the fence
    pub container_prefix: String, // e.g. "> " for a blockquote cell
    pub fence_len: usize,        // backtick count
    pub attr_tail: String,       // text after the language token inside the braces
    pub id: Option<UnitId>,      // filled by EXR-4's id function; None from scan_units
}
pub struct InlineUnit {
    pub start: usize, pub end: usize, pub code_start: usize,
    pub lang: String, pub code: String,
    pub span_index: usize, // document-wide over all spans
    pub in_fence: bool,    // the span sits inside an unbraced fence (QNR never runs those)
    pub in_front_matter: bool, // the span sits in the leading YAML block (EXR-7 sends it as `inFrontMatter`; Julia's view excludes it)
    pub id: Option<UnitId>, // filled by EXR-4's id function; None from scan_units
}

/// No ownership predicate (epic decision 42): every cell and every span, in document order.
pub fn scan_units(input: &str) -> Vec<Unit>;

/// Re-derive `start`, `end`, `code_start` and `line` in the text one engine receives, keeping
/// the shared `id`, `index` and `span_index` (Phase 4).
pub fn locate_units(shared: &[Unit], engine_text: &str) -> Vec<Unit>;

/// One engine's view of the shared list: each unit paired with whether that engine owns it.
pub struct EngineUnit<'a> { pub unit: &'a Unit, pub owned: bool }
pub fn view_for_engine<'a>(units: &'a [Unit], owns: &dyn Fn(&str) -> bool) -> Vec<EngineUnit<'a>>;

/// Today's executor rule, quarantined: bare `{lang}` opener, column 0, exactly three
/// backticks, no container prefix, empty attribute tail. Deleted with the legacy rule.
pub fn runs_in_legacy_executor(cell: &CellUnit) -> bool;
```

`UnitId` is defined here as a typed slot (a newtype over `String`, or the type EXR-4 chooses);
EXR-4's function computes it and fills `id`.

All braced cells and all spans are always returned, so callers can pass ceded
cells through untouched and a view marks what is its own. A shared
`patterns` module defines the one language class and builds the fence and span
regexes; the mask imports the same constants. The attribute tail after the token is
skipped by the scanner and is not part of unit identity (EXR-4 hashes the lines
between the fences).

## Phased checklist

### Phase 0: characterize what exists

- [ ] Tests for `parse_code_blocks` pinning today's behavior: bare fence,
  trailing spaces after the opener, four-backtick fence, indented fence, fence
  inside a list item and a blockquote after a serialization round trip,
  `{python echo=false}` (not matched), `{=html}` (not matched), non-ASCII
  language name, CRLF input, `{python.marimo}`, `{python .marimo}`, `{py-thon}`,
  `{Python}`, `{1abc}`. Each case that differs from q1 (verified 2026-10-30 by
  running both; see the Risks section) gets a line in the checked-in divergence list
  with its strand.
- [ ] Pin the quirks (a) to (j), one test each:
  (a) the match end swallows blank lines after the closing fence (`"```\n\n\nText"` leaves
  `"\nText\n"`), so the scanner's `end` and the gap definition must reproduce it or the
  splice gains a blank line;
  (b) `\s*` between the fence and the brace spans newlines, so
  `"```\n{python}\nx=1\n```\n"` is one cell;
  (c) CRLF: the match ends before `\n` and `\r` stays in the gap;
  (d) `\w` is Unicode: `{é}` runs in the executor and is rejected by pampa;
  (e) four or more backticks, indentation, list or blockquote nesting, and
  attribute-tail cells are all skipped;
  (f) leading blank lines after the opener are swallowed:
  `"```{python}\n\n\nx=1\n```\n"` gives code `x=1\n` and a shifted `code_start`, which
  changes `code`, per-cell source info and EXR-4's "lines between the fences";
  (g) an unterminated cell runs to the next bare line of exactly three backticks and
  swallows the next cell's opener;
  (h) the closer must be exactly three backticks plus whitespace: ```` ```  trailing ```` and a
  four-backtick line do not close the cell;
  (i) a column-0 `{lang}` opener inside an unbraced, longer fence executes, and only the mask
  prevents that (`text_execute.rs:899-934`, test T14, covers the masked case only);
  (j) a space between the backticks and the brace is accepted.
- [ ] Pin `partition_cells` ceding case-insensitively: a `{Python}` cell with
  `handled = ["python"]` (today's tests, `text_execute.rs:946-1100`, never pin this).
- [ ] A test that serializes a parsed document containing a list-nested cell, a
  blockquote cell and a div-nested cell, and asserts what the writer emits: list-nested
  fences are indented and blockquote cells are prefixed with `> ` (so the executor skips
  them), div-nested cells are not indented (so it runs them). This replaces the old open
  question.
- [ ] Tests pinning the mask's superset relation: every string `INLINE_R_PATTERN`
  matches is also matched by `MASK_INLINE_RE` (Phase 2 extends this to the scanner's span
  pattern once it exists); and a test that records today's gap (`{python.marimo}` and `{py-thon}` spans are not masked), to be
  flipped in Phase 1.
- Exit: behavior documented in tests; nothing changes yet. Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 1: shared patterns and the helper move

- [ ] New `patterns` module: the language class from decision 5, the attribute-tail
  pattern, a fence-opener builder, a brace-span builder.
- [ ] The mask and the executor import the constants; the existing mask tests
  (T9-T29 in `nested_cell_mask.rs`) pass unchanged.
- [ ] Widen `MASK_INLINE_RE` (`nested_cell_mask.rs:123`) to the shared token class. Test
  first: flip the Phase 0 gap test so a `{python.marimo}` and a `{py-thon}` span inside a
  display block are masked. This is a deliberate mask behavior change (epic decision 13).
- [ ] Move `engine_cell_lang` (and any other non-legacy helper found by reading
  `capture_splice.rs`) to a module that stays. Update every non-test caller: the resolver
  (`resolution.rs:136`), the mask (`nested_cell_mask.rs:259`), `lmd_desugar.rs`,
  `unexecuted_cell_count.rs`, `lmd/emit.rs` (`:48`, `:236`) and `capture_splice.rs` itself
  (`:120`, `:256`, `:395`). The in-module unit tests that call it move with the imports,
  including the three in `nested_cell_mask.rs` that name the full path (`:660`, `:869`,
  `:1105`); update the comment at `pampa/src/writers/qmd.rs:538` that names the old path.
- [ ] Edit the two integration tests to the new path
  (`tests/integration/capture_splice_seam.rs:36,60`, `capture_splice_engines.rs:26,162`).
  Leave **no** re-export at the old path (epic decision 10: the legacy module stays
  deletable). Check by grep that nothing imports `capture_splice::engine_cell_lang`.
- [ ] Reconcile the Q-2-35 catalog title: `error_catalog.json:408` says "Invalid List-Table
  Structure" while pampa emits "Indented code blocks are not supported"
  (`pampa/resources/error-corpus/Q-2-35.json`). Fix the catalog entry to match the
  runtime message and run the catalog tests.
- Exit: no scanner behavior change except the documented language-class change
  (the executor's Unicode `\w` becomes the ASCII class that pampa accepts, which also
  admits `.` and `-` and rejects a leading digit or underscore), covered by a named
  test, and the widened mask pattern. One consequence to state in that test: once the class
  admits `.` and `-`, a `{python.marimo}` or `{py-thon}` cell reaches `partition_cells`,
  finds no executable language, and in a multi-engine document raises `NoHandlerForLanguage`
  (before, the executor never saw it). Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`. Lands after EXR-1 Phase 3.

### Phase 2: the scanner

- [ ] Tests first, one per rule: spans in prose, spans in attribute values
  (`[x](u){title="..."}`), span adjacent to a display fence, span inside a ceded
  braced block (not claimed), span inside a language-named unbraced fence (claimed, `in_fence`), span inside a bare or ` ```markdown ` fence (masked, not scanned), span in
  a language no engine owns (returned; owned by no view), doubled-brace
  `{{python}}` (not matched), tab separator, multi-line body, no-space after
  the language (not matched), two spans with no character between them (the
  documented accepted limitation).
- [ ] Front matter and comments, tests first (EXR-3 divergence 10): a span inside the
  leading YAML front matter block **is** a unit (q1 evaluates it; the scanner does not skip
  the block), while a span inside an HTML comment is **not** claimed and not counted (block,
  inline and multi-line comments). Comment ranges are recognized only outside fences, so
  `<!--` shown in a fence does not hide the spans after it. Then implement both.
- [ ] Front-matter provenance (decision 11), tests first. First, a probe test that prints
  the `SourceInfo` root file id of a document's own key, a `_quarto.yml` key, a `_metadata.yml`
  key, a format-default key and a synthesized key (`rendered:`, `listing-item:`) after the
  merge; it turns the UNVERIFIED line of decision 11 into facts. Then: a span in a
  document-owned `title:` is a unit and takes a span counter number; a span in a
  `_quarto.yml` value stays literal, is not a unit and shifts no counter
  (`quarto_yml_front_matter_span_stays_literal`); the same for `_metadata.yml`, a format
  default and a synthesized value; a project value passes through the engine text unchanged.
- [ ] Bare class-less fences, pinned behavior (settled: the mask's scope stands and the claim
  rule narrows, `nested_cell_mask.rs:253-260`; the docs already say a language-less fence
  displays a span, `computations.qmd:175-202`) (settled arbitrarily, Gordon 2026-10-31: revisit if wrong). Tests first, through the
  stage's masked serialization: a span in a bare ```` ``` ```` fence is not a unit; a span in a
  ```` ```markdown ```` fence is not a unit; a span in a ```` ```r ```` or ```` ```text ```` fence is a unit
  with `in_fence: true`; after `unmask` the masked spans are restored byte for byte. No code
  change in the mask; the scanner needs none either, since it scans the masked text.
- [ ] Index tests: cell indices count every braced fence, run or ceded, and skip
  `{=html}`; adding a span before a cell does not change that cell's index; span
  indices count spans only, document-wide over every span (a span in a language nobody
  owns advances the counter and is returned). View tests: `view_for_engine` marks exactly
  the units whose language the supplied `owns` closure accepts, for cells and spans, and two
  views over one list disagree only in `owned`.
- [ ] Unit shape tests: `id` is `None` from `scan_units`; `indent`, `container_prefix`,
  `fence_len` and `attr_tail` are reported for an indented fence, a blockquote cell, a
  four-backtick fence and `{python .marimo}`; `runs_in_legacy_executor` is false for each
  of those and true for a bare column-0 three-backtick cell and for a bare `{python.marimo}`
  or `{py-thon}` cell (the executor's class is widened in Phase 1, and these have no
  attribute tail).
- [ ] `in_fence` tests: a span in prose is `false`; a span inside a language-named unbraced fence is
  claimed and `true`; a span inside a four-backtick fence and inside an indented code
  block are checked against QNR's CommonMark view (the EXR-8 corpus).
- [ ] Token tests: the table above, case-insensitive token comparison (`view_for_engine`
  ownership is case-insensitive too), the attribute tail.
- [ ] Property test: for generated documents, every `Inline` unit found is also
  matched by the mask's (widened) inline pattern, and no unit overlaps another. The
  generator MUST include dotted and hyphenated language tokens (`{python.marimo}`,
  `{py-thon}`).
- [ ] Implement `scan_units`; positions are byte offsets into `input`.
- [ ] Location test: `SourceInfo::substring` over a unit resolves to the right
  `file:line:column` through a multi-file source map.
- Exit: Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`. May run in parallel with EXR-1.

### Phase 3: conformance fixtures

- [ ] Fixture format: `crates/quarto-core/tests/fixtures/units/<name>.qmd` plus
  `<name>.units.json` listing, in order, `kind`, `lang`, `index` (cells) or
  `span_index` and `in_fence` (spans), 1-based start line, and the code text. EXR-4 later adds
  `content_hash` and `cell_id` fields.
- [ ] Corpus: all Phase 2 cases plus nested constructs (list, blockquote,
  four-backtick fences, a cell inside a display fence) and the dotted and
  class-attribute forms. Additions (reviewer-verified):
  a double-backtick span ``` ``text ``{python} x`` more`` ``` (the scanner needs a
  non-backtick before the opening backtick, so it does not match, while QNR evaluates any
  CommonMark Code node whose literal starts with `{python} ` at `evaluation.jl:476-505`,
  marker `:488`, so QNR substitutes inside and the sites disagree);
  a tab separator (the scanner accepts `[ \t]`, QNR's marker is a literal space, so a tab
  span counts but is never run: a count mismatch and no record);
  a front matter span (`title: "x `{python} 1`"` in the document's own front matter is a
  unit, as in q1, which evaluates it; the same span in a `_quarto.yml` value is not)
  and an HTML-comment span (`<!-- `{python} 1+1` -->` is not claimed, although q1 and a
  plain text scan would claim it; an AST walker does not see it, so skipping it keeps the
  scanner and the walker in agreement).
  Tilde fences `~~~{python}` are a pampa parse error (Q-2-41), so they cannot exist in q2
  and are not in the corpus. Where Positron's parser differs on nesting (epic decision 39),
  the fixture carries the expected Positron cell index, for EXR-16 S1.
  The TS scanners (`breakQuartoMd`, `languagesWithClasses`) are run over the same corpus
  and their differences recorded.
- [ ] A Rust test loads every fixture and asserts the scanner output.
- [ ] A cross-check test: for each fixture, the AST walker
  (`engine_cell_lang`) and the text scanner agree on cell languages, except for a
  checked-in list of expected divergences with a reason each.
- [ ] Document the fixture format so the Positron side can run the same files
  (spike S1 in EXR-16 runs them in a Positron checkout; nothing is sent from here).
- Exit: Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`. May run in parallel with EXR-1.

### Phase 4: migrate the callers (after EXR-1 Phase 3)

- [ ] Test first, the corpus-equality test: `scan_units` filtered by
  `runs_in_legacy_executor` equals `parse_code_blocks` on the whole Phase 3 corpus
  (same cells, same code, same gaps including the blank-line and CRLF quirks). It gates
  the switch, so the executor needs no scanner of its own.
- [ ] Test first, in the engine-execution stage: the stage scans units once, before any
  engine runs, and a two-engine document gives the second engine the same cell indices as
  the document-wide scan (the first engine's fences are gone from its text, the indices
  are not renumbered); the second engine's `line` is located in the text it receives.
- [ ] Test first, `locate_units`: engine 1 reconciles away its cells, then engine 2's `start`,
  `end` and `code_start` still splice correctly in the text engine 2 receives (a `[knitr,
  jupyter]` document, as at `text_execute.rs:1113`). Then implement
  `locate_units(shared, engine_text)`: rescan the engine's text with `scan_units`, match by
  (kind, language, code) in document order; a matched unit takes the new `start`, `end`,
  `code_start` and `line` and keeps the shared `id`, `index` and `span_index`; a shared unit
  with no match is dropped from that engine's list. EXR-6 and EXR-7 cite this function for
  their relocation steps.
- [ ] Add `ExecutionContext.units` (EXR-7 later puts the wire form on it) and have the stage
  scan once and pass **only the shared list** to every engine (the stage builds no view).
  Each engine calls `view_for_engine(ctx.units, owns)` with its own rule, over the list
  `locate_units` relocated for it.
- [ ] `execute_qmd` and `IpynbReplayEngine` consume the context's units for cells
  (spans are returned but ignored until EXR-3), replacing `parse_code_blocks`.
- [ ] `partition_cells` consumes the engine view's `owned` flag; behavior identical.
- [ ] Re-point the two test sites that still use the old scanner: the T14 seam test
  (`t14_masked_display_block_yields_no_jupyter_cells`, `text_execute.rs:899-934`) moves to
  `scan_units`, and the `make_block` helper behind the partition tests (`:946-1100`) is
  ported to `CellUnit`. JOL-2b Phase 1 rewrites the same partition tests, so land the two in
  one agreed order and do not edit the same tests twice.
- Exit: existing Jupyter tests (kernel-gated ones run locally) green. Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`. EXR-1's Phase 5 may start after this.

### Phase 5: the `kernel_for(unit)` seam (after EXR-1 Phase 3)

- [ ] Define the seam: `trait KernelRouter { fn kernel_for(&self, unit: &Unit) ->
  Result<KernelName, ExecutionError> }`, owned by this plan. The signature takes the
  unit (language, cell or inline) so a reopened JOL-3 (deferred) can vary the answer per
  unit without a change to the loop.
- [ ] Refactor `execute_blocks_inner` to loop over `Unit`s and ask the `KernelRouter` for
  a session key per unit. The default router returns the one kernel chosen from the
  first owned cell (today\'s behavior); once JOL-2a lands it wraps `select_kernel`, and
  once JOL-2b lands the router behind it is JOL-2b\'s. Sessions are already keyed by
  `SessionKey` in `engine/jupyter/daemon.rs`.
- [ ] Test the seam without a kernel. `execute_blocks_inner` calls
  `daemon().get_or_start_session` (`text_execute.rs:268-273`) and `execute_in_session`
  (`:331-334`) directly, so a stub router cannot drive the loop (the injectable executor
  arrives in EXR-5 Phase 0, the fake kernel in EXR-3). Extract a pure planning function
  `plan_units(units: &[Unit], router: &dyn KernelRouter) -> Vec<(usize, KernelName)>`
  (the `usize` is the unit's position in `units`; `KernelName` is a newtype over `String`)
  that the loop calls, and test that: a stub router returning two different names for two
  languages is called once per unit, in document order, even though production uses only
  one name (JOL-3, several kernels, is deferred). EXR-5 Phase 0 later refactors the same
  loop.
- [ ] A pure `splice_units(input, replacements)` function with tests, so the
  gap-copy logic is testable without a kernel.
- Exit: kernel-gated integration tests unchanged; JOL-2a and JOL-2b (and a reopened
  JOL-3) can replace the router without touching the loop. Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

## Risks and open questions

- **Cells with an attribute tail.** Today the executor's bare-only regex never runs
  `{python .marimo}`. The scanner returns it, and a view owns it by the
  language-level rule, so Phase 4 must keep the executor's behavior identical (ignore cells
  whose tail the executor ignored today, via `runs_in_legacy_executor`) until a later plan decides otherwise
  (strand bd-rh4cce9e asks whether q2 should run them, as q1 does). Phase 0 pins the
  current behavior.
- **The `in_fence` flag (decided with Gordon, 2026-10-30).** QuartoNotebookRunner
  evaluates spans only as CommonMark inline code and never inside fences, while this
  scanner claims spans in language-named unbraced fences (decision 2). `InlineUnit` therefore carries
  an `in_fence` flag that says a span sits inside a fence; EXR-7 sends it on the wire as
  `inFence` and EXR-8 uses it with a conformance corpus of tricky spans (multi-backtick
  spans, indented code). The corpus lives here with the other conformance fixtures.
  Open item: how often the scanner and QNR disagree on such spans in practice
  (unverified); the Phase 3 corpus additions (double-backtick spans, tab separator, front
  matter, HTML comments) are the first measurements.
- **Indented and nested cells (decided, epic decision 26).** The executor stays
  unchanged in this epic; the divergence list records each difference with its
  strand: list-nested cells (bd-9j6z311f, q1 runs them), attribute-tail cells
  (bd-rh4cce9e), capitalized `{Python}` (bd-7gvl455a, q2 runs it, q1 does not). A
  fence indented four spaces (Q-2-35) and knitr-style options in the braces (Q-2-36)
  are deliberate q2 parse errors. Phase 0 still pins the current behavior; the writer
  indents list-nested fences (verified by running `pampa -t qmd`), which explains
  bd-9j6z311f.
- **Julia runner.** QuartoNotebookRunner finds spans with CommonMark, not a regex,
  and recognizes only three languages at the top level; EXR-8 aligns it and uses
  these fixtures as the oracle.
- **Count-assertion contract.** Engines that scan themselves (knitr, Julia,
  marimo) must assert their unit count against this scanner's.

## References

- `engine/jupyter/text_execute.rs:124-154,262`, `engine/nested_cell_mask.rs:50,123,253,259`,
  `engine/knitr/preprocess.rs:202`, `engine/resolution.rs:128`, `engine/capture_splice.rs:86`,
  `stage/stages/lmd_desugar.rs:29`, `stage/stages/unexecuted_cell_count.rs:13`,
  `lmd/emit.rs:48,236`
- `ts-packages/quarto-api/src/markdownRegex/index.ts`
- `external-sources/quarto-cli/src/core/execute-inline.ts`,
  `external-sources/quarto-cli/src/core/pandoc/pandoc-partition.ts`,
  `external-sources/QuartoNotebookRunner.jl/src/{evaluation,parsing}.jl`,
  `external-sources/positron/src/vs/workbench/contrib/positronQuarto/common/quartoParser.ts`
- [the Jupyter owned-languages epic](2026-10-17-jupyter-owned-languages-epic.md)
- Strand bd-my5fqvok (should span-only documents route to an engine)
