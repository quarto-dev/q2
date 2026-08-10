# Callout `title=` attribute is ignored; header shows the type name instead of the author's title (bd-callout-custom-title-dropped-9qi1p7iw)

**Date:** 2026-08-10
**Braid:** `bd-callout-custom-title-dropped-9qi1p7iw` (P1, bug, label `parity`)
**Origin strand:** `br-de85v0a8` — in the **connect-docs porting skein**, a
*different* braid project. It does not resolve against the q2 skein; the
q2-side strand above is the one to work.
**Branch:** investigated on `docs/feature-porting-process` @ `d1a8ac9f` (the
checkout the skill was invoked in — see "Where this should land" below).
**Status:** Investigation — pending design alignment with user. **Do not start
implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand's root-cause analysis is accurate at HEAD, the
fix site is a single function, the "parse a string as markdown inlines" helper
the Q1-faithful fix needs already exists and is already called from an
`AstTransform`, and there is a close in-tree precedent for the attribute-span
handling. What is left is a genuine design choice (how faithful to Q1's
markdown-parsing of the attribute, and how much adjacent divergence to absorb),
which is what the questions below are for.

## Issue context

Filed 2026-08-10 by Carlos Scheidegger, so its assumptions are current — none
of the usual "stale strand" risk. A callout written as

```markdown
::: {.callout-note title="Off-Host Execution"}
:::
```

renders a header reading "Note". The author's title survives only as the
`title=` attribute on the outer div (which Q1 also emits, so that part is
correct). The output is otherwise a well-formed titled callout: `callout-titled`
is set and `callout-title-container` is present — its *content* is the injected
type display name.

Q1 emits:

```html
<div class="callout-title-container flex-fill"><span class="screen-reader-only">Note</span>Off-Host Execution</div>
```

**Impact:** ~25 pages of the Posit Connect docs, which use the attribute form
throughout. Readers see an unlabeled "Note"/"Warning"/"Important". No warning is
emitted. Because the title is never marked user-supplied, the
`screen-reader-only` type span is also skipped, so a titled and an untitled
callout are indistinguishable to assistive technology — one fix addresses both.

## Dependency graph

**Empty.** `braid dep list` returns no edges and `braid dep tree` shows the
strand alone. No incoming `blocks` pressure and no `discovered-from` parent in
*this* skein — the "why filed" context lives in the connect-docs porting skein
(`br-de85v0a8`) and, usefully, was copied verbatim into the description.

Context therefore comes from the `parity` label instead. That cohort is 21
strands from the same porting effort; 12 are closed, and the closed ones
(`bd-named-entities-w6xbfftj`, `bd-obkvhlam`, `bd-fz6gwfq0`, …) are the model
for how this kind of work usually lands here: a narrow Q1-behavior-matching fix
plus focused regression tests. `bd-email-autolink-dropped-2jj38iiv` is the
currently in-progress sibling of the same shape.

## What the code looks like today

Both files named in the strand exist and still have the shape described.

- `crates/quarto-core/src/transforms/callout.rs:199-206` —
  `convert_div_to_callout` builds `title_inlines` **only** from a leading
  `Header` block. It reads `appearance`, `collapse`, and `icon` through
  `extract_attr_value(&div.attr, …)` at `:209-214` but never reads `"title"`.
  So the title slot set at `:253` is empty for an attribute-titled callout.
- `crates/quarto-core/src/transforms/callout_resolve.rs:255-265` — with an
  empty title and `appearance == "default"`, `resolve_callout` injects
  `callout_display_name(...)` and sets `title_is_user_supplied = false`, which
  at `:274-291` skips the `screen-reader-only` span. Both branches are correct
  Q1 mirrors; they are simply fed the wrong input.

**No existing test covers the attribute form.** `grep` over
`crates/quarto-core/tests/` finds callout title hits only in language-term
tests. The unit test at `callout.rs:388` (`test_convert_callout_with_title`)
exercises the Header path only.

### Q1's actual precedence (read, not assumed)

`external-sources/quarto-cli/src/resources/filters/customnodes/callout.lua:48-50`:

```lua
local title = string_to_quarto_ast_inlines(div.attr.attributes["title"] or "")
if not title or #title == 0 then
  title = resolveHeadingCaption(div)
end
```

Two consequences worth pinning down before implementing:

1. **The attribute value is parsed as markdown inlines**, so
   `title="Use \`renv\`"` yields a code span in the header.
2. **The leading heading is only *removed* inside `resolveHeadingCaption`**
   (`external-sources/quarto-cli/src/resources/filters/common/pandoc.lua:130-138`
   does `div.content:remove(1)`). When `title=` is non-empty that function is
   never called, so **a leading heading stays in the callout body**. A naive
   port that strips the header unconditionally would diverge here.

Q1 clears `appearance`/`collapse`/`icon` from `div.attr` but leaves `title` in
place, matching what q2 already emits.

### The helper the faithful fix needs already exists

`pampa::pandoc::meta::parse_config_string_as_markdown(&str, &SourceInfo, &mut Vec<DiagnosticMessage>) -> ConfigValueKind`
(`crates/pampa/src/pandoc/meta.rs:34`) is synchronous, needs no parser handle,
is wasm-safe, and is **already called from an `AstTransform`** —
`crates/quarto-core/src/transforms/config_markdown.rs:164`. It returns
`PandocInlines` when the parse yields a single paragraph and `PandocBlocks`
otherwise, and never fails (a parse error becomes a Q-1-20 warning plus a
literal-text span). Passing a real parent `SourceInfo` makes every produced node
a `SourceInfo::substring` of it — no throwaway `FileId`.

### Precedent for reading the attribute's source span

`AttrSourceInfo.attributes[i]` is the `(key_src, val_src)` for the i-th entry of
the `LinkedHashMap` in insertion order
(`crates/quarto-pandoc-types/src/attr.rs:52-56` and the invariant comment at
`:28-50`). That invariant is **known-broken in two parser paths** (bd-3aolj,
bd-1e6a5), so the documented pattern is a length guard plus `debug_assert` with
a `None` fallback. `crates/quarto-core/src/transforms/theorem.rs:336-360` is
the canonical example — and is a very close analogue, since it turns a `name=`
attribute into title inlines. Note it produces a plain `Inline::Str` rather
than markdown-parsing the value, which is one of the consistency questions
below.

### Repro

Committed at `claude-notes/plans/callout-title-attribute-investigation/repro.qmd`,
covering seven cases: attribute-titled, other type, untitled control,
heading-titled, **both** (attribute wins, heading stays), markdown-in-attribute,
and empty attribute (falls back to heading). The upstream two-case repro lives
outside this repo at
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/callout-custom-title-dropped/`;
the local copy is the durable one.

**Reproduced end-to-end at HEAD.** `cargo run --bin q2 -- render …/repro.qmd`
(exit 0, no warnings); the emitted `callout-title-container` for each case is
recorded in
`claude-notes/plans/callout-title-attribute-investigation/observed-output.md`.
Cases 1, 2, and 6 drop the title as reported; cases 3, 4, and 7 are correct.

**Case 5 is a divergence the strand did not call out**, and it changes the
shape of the fix. With both `title=` and a leading heading, q2 lets the
**heading win** *and* **consumes it from the body**; Q1 gives the attribute
precedence *and* leaves the heading in the body. So the two behaviors are
independent: adding an attribute-first branch fixes precedence but, if the
existing unconditional header-strip stays where it is, the body is still wrong.
Phase 1 has to move the removal into the fallback branch, not just reorder the
lookup.

Two smaller observations: `title=` is preserved on the outer div in every case
(so the fix must read it without removing it), and `title=""` is emitted as a
literal empty `title=""` attribute (case 7) — Q1's behavior there was not
checked.

## Proposed phases (draft)

- **Phase 0 — Reproduce + test plan (TDD).** Run `q2 render` on the fixture and
  capture actual output. Write failing tests for: attribute only, heading only,
  both (attribute wins, heading retained in body), markdown in the attribute,
  empty attribute value (falls back to heading), and the `screen-reader-only`
  span appearing for an attribute title.
- **Phase 1 — Read `title` in `convert_div_to_callout`.** Attribute-first with
  heading fallback; strip the leading header *only* in the fallback branch.
- **Phase 2 — Title inlines + source attribution.** Whichever of plain-`Str` vs
  markdown-parse the design questions settle on, with the theorem.rs-style
  guarded span lookup.
- **Phase 3 — End-to-end verification.** `q2 render` the fixture, inspect the
  emitted `callout-title-container`, record the invocation and output snippet
  per the repo's end-to-end rule.
- **Phase 4 — Docs**, if callout titles are documented under `docs/`.

## Open design questions for the user

1. **Markdown in the attribute value: match Q1, or plain text?** Q1 parses
   `title=` with `string_to_quarto_ast_inlines`, so `title="Use \`renv\`"`
   renders a code span. The helper to do this exists and is already used from a
   transform, so the faithful option is cheap — but it costs a full tree-sitter
   document parse per attribute-titled callout, and `theorem.rs` sets the
   opposite in-tree precedent (plain `Inline::Str` for `name=`). Full parity, or
   plain `Str` now with parity deferred to its own strand?
2. **If we parse: what to do with a `PandocBlocks` result?** A value containing
   block syntax (or a blank line) returns `PandocBlocks`, which a title slot
   cannot hold. Flatten to inlines (there is a `blocks_to_inlines` in
   `title_block.rs:161`), or fall back to literal text? Q1's helper effectively
   never produces this, so there is no Q1 answer to copy.
3. **Both `title=` and a leading heading — match Q1 exactly?** Confirmed by the
   repro (case 5) that q2 currently does the opposite of Q1 on *both* counts:
   the heading wins and is consumed. Q1 lets the attribute win and leaves the
   heading in the body, which renders what looks like a duplicate title. Mirror
   Q1 exactly, or take the attribute but also consume the heading (arguably
   nicer output, definite parity break)? And should q2 warn when both are
   present? Q1 emits none, but this repo's diagnostics culture might justify
   one.
4. **Adjacent divergence — heading level.** Q1's `resolveHeadingCaption` accepts
   *any* `Header`; q2 requires `level >= 2` (`callout.rs:202`), so an H1-led
   callout takes the title path in Q1 but not in q2. In scope here, out of
   scope, or file as a separate strand?
5. **Where should this land?** The skill ran in the main checkout on
   `docs/feature-porting-process`, which is a docs branch — a poor home for a
   callout fix. I have committed only the plan + fixture there. Recommend a
   worktree (`cargo xtask create-worktree bd-callout-custom-title-dropped-9qi1p7iw
   --base main`) for the implementation; per the skill I have not created one.

## Risks / tradeoffs (draft)

- **Low blast radius, high confidence.** The change is confined to the title
  slot's construction; `callout_resolve.rs` already implements the correct Q1
  shape and needs no edit. Everything downstream (the `screen-reader-only` span,
  `callout-titled`) starts working by consequence.
- **Snapshot churn.** Any existing callout snapshots for attribute-titled
  callouts will change (title text plus a new `screen-reader-only` span). Per
  the repo's snapshot rule, count and summarize these in the commit message.
- **The `AttrSourceInfo` alignment invariant is known-broken** in two parser
  paths (bd-3aolj, bd-1e6a5). Use the guarded theorem.rs pattern rather than
  indexing blind; do not treat those bugs as blockers.
- **Not verified end-to-end yet** — see the repro note above.
