# Callout `title=` attribute is ignored; header shows the type name instead of the author's title (bd-callout-custom-title-dropped-9qi1p7iw)

**Date:** 2026-08-10
**Braid:** `bd-callout-custom-title-dropped-9qi1p7iw` (P1, bug, label `parity`)
**Origin strand:** `br-de85v0a8` — in the **connect-docs porting skein**, a
*different* braid project. It does not resolve against the q2 skein; the
q2-side strand above is the one to work.
**Branch:** `braid/callout-title-attribute`, off `origin/main` @ `b2b6100c`.
(Investigated on `docs/feature-porting-process`; cherry-picked across once main
was current.)
**Status:** Design settled — see "Decisions" below. Ready to implement,
starting with Phase 0 tests.

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

## Decisions (settled with the user, 2026-08-10)

1. **Full markdown parse of the attribute value.** Match Q1. Performance is
   explicitly deferred — titles are tiny documents, and the fixed cost of
   spinning up a tree-sitter parse is accepted for now.
2. **`PandocBlocks` result:** if it is a single block containing a single
   paragraph, take its inlines. Anything else → emit a **warning diagnostic**
   and ignore the attribute's content.
3. **Attribute + heading both present:** emit a **warning** and consume either
   one (which one does not matter — it is almost certainly an authoring
   mistake). This is a deliberate, warned divergence from Q1, which silently
   prefers the attribute and leaves the heading in the body.
4. **Heading level:** remove the `level >= 2` check at `callout.rs:202` so any
   `Header` can supply the title, matching Q1's `resolveHeadingCaption`.
5. **Branch:** `braid/callout-title-attribute`, off `origin/main`. No worktree —
   this checkout is not shared.

## Source-mapping design (the ancillary problem)

Re-parsing the attribute value is not just "call the parser." The value handed
to the parser is **not** the text its `SourceInfo` describes, so naive nesting
produces silently wrong locations.

### Why the obvious approach is wrong

`SourceInfo::substring(parent, start, end)` composes a **purely affine** map:
`resolve_byte_range` computes `parent_start + start_offset`
(`quarto-source-map-0.1.0/src/source_info.rs:194-200`, `:388-403`), with no
validation, no clamping, and no access to any text. The nested reader feeds it
byte offsets into the *inner* string
(`crates/pampa/src/pandoc/location.rs:213-218`).

But the two texts differ:

- `Attr.2["title"]` is stored **unescaped and unquoted** —
  `extract_quoted_text` strips the delimiters and `unescape_punctuation`
  collapses `\X` → `X` for ASCII punctuation
  (`crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:28-59`), called
  from `treesitter.rs:1207-1212`.
- `AttrSourceInfo.attributes[i].1` spans the **raw text including both
  quotes** — the grammar aliases a token containing the delimiters
  (`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:580-585`), and
  `commonmark_attribute.rs:39-51` records the full node range.

Measured on a real render (`observed-output.md` and the probe below): for
`title="Say \"hello\" now"` the stored value is 15 bytes while the span covers
19. Passing the span as `parent_source_info` shifts every inline by +1 (the
opening quote) and by one more byte per collapsed escape.

Worked example, `` title="Use `renv` today" `` with the value span starting at
byte 81: the `` `renv` `` code span sits at inner bytes 4..10, which maps to
85..91, while its true location is 86..92.

This is not merely a diagnostics concern: `SourceInfo::preimage_in` feeds the
incremental writer's verbatim-copy decision, and a drifted range there can copy
the wrong bytes.

### Precedent: the mechanism is solved, the mapping is not

- **Mechanism (fine).** `parse_config_string_as_markdown`
  (`crates/pampa/src/pandoc/meta.rs:34`) is sync, wasm-safe, and already called
  from an `AstTransform` at
  `crates/quarto-core/src/transforms/config_markdown.rs:164`.
- **Mapping (unsolved, tree-wide).** The YAML path has exactly this bug:
  `quarto-yaml`'s `compute_scalar_len` spans the quotes while the *decoded*
  scalar is handed to the nested parse, and nothing compensates. The design doc
  `claude-notes/plans/2026-07-20-ipynb-surface-syntax-design.md:73-92` states
  the constraint outright — `Substring`/`Concat` compose only affine maps, a
  `Transformed` variant once existed and was removed as unused, and the YAML
  analogue "currently punts."
- **The one exemplary case** is cell options
  (`crates/quarto-core/src/cell_options/mod.rs:33-43`), which stays exact
  precisely because it never unescapes — it reassembles a `SourceInfo::concat`
  of per-line substrings so every byte is a real source byte.

### What we will do

Detect drift **from lengths alone** — the span length is the raw length, and
the value's length is known, so no source text is required. Let
`span_len = end - start` of `attributes[i].1` and `n = value.len()`:

| condition | meaning | parent to pass |
|---|---|---|
| `span_len == n` | bare, unquoted value | `substring(src, 0, n)` — **exact** |
| `span_len == n + 2` | quoted, no escapes collapsed | `substring(src, 1, 1 + n)` — **exact** |
| otherwise | escapes were collapsed | whole-value span — approximate |

This needs no new infrastructure and no access to `SourceContext` (which is
`Option` on `RenderContext` and can be content-stripped by
`SourceContext::without_content()`, so depending on it would be fragile).

The fallback is **bounded and safe, not merely tolerable**: unescaping only ever
shrinks the string, so every mapped offset stays inside the attribute's raw
extent. The error is at most `1 + #escapes` bytes and can never point at a
neighbouring attribute. The exact path covers every case in our fixture and all
~25 affected Connect pages; only a title containing a backslash escape takes
the approximate path.

Exact non-affine mapping (an offset map through the unescape, emitting
`SourceInfo::concat` pieces around each escape) is deliberately **out of scope
here** and filed separately — it is the same infrastructure the YAML path and
the ipynb design both need.

## Phases

- **Phase 0 — Tests first (TDD).** Failing tests for: attribute only; heading
  only; both (warning + one consumed); markdown in the attribute (code span in
  the header); block-syntax value (warning, content ignored); empty attribute
  (falls back to heading); H1 heading as title; and the `screen-reader-only`
  span appearing for an attribute title. Plus a span assertion pinning the
  exact-mapping cases.
- **Phase 1 — Title source selection in `convert_div_to_callout`.**
  Attribute-first with heading fallback; move the header removal into the
  fallback branch; drop the `level >= 2` check; warn when both are present.
- **Phase 2 — Parse + source mapping.** `parse_config_string_as_markdown` with
  the length-derived parent above; `PandocInlines` straight through,
  single-paragraph `PandocBlocks` unwrapped, everything else warned and
  dropped. Use the guarded `theorem.rs:336-360` pattern for the index lookup.
- **Phase 3 — Diagnostics.** Register the new warning(s) in
  `quarto-error-catalog` if they need codes; confirm wording and spans against
  the fixture.
- **Phase 4 — End-to-end verification.** `q2 render` the fixture, inspect the
  emitted `callout-title-container`, record invocation + output snippet per the
  repo's end-to-end rule.
- **Phase 5 — Docs**, if callout titles are documented under `docs/`.

## Risks / tradeoffs

- **Low blast radius on the rendering side.** `callout_resolve.rs` already
  implements the correct Q1 shape and needs no edit; the
  `screen-reader-only` span and `callout-titled` start working by consequence.
- **Snapshot churn.** Attribute-titled callout snapshots will change (title
  text plus a new `screen-reader-only` span). Per the repo's snapshot rule,
  count and summarize them in the commit message and flag anything surprising.
- **Warning on attribute+heading is a deliberate parity break** (decision 3).
  Q1 is silent here. If the Connect docs turn out to use both together at any
  scale, revisit before shipping.
- **Removing the `level >= 2` check may change existing documents** where an H1
  inside a callout was previously left in the body. Grep the corpus and check
  snapshots.
- **`AttrSourceInfo` positional alignment is known-broken** on duplicate keys
  (bd-3aolj, bd-1e6a5) — use the guarded pattern, do not index blind.
- **`commonmark_attribute.rs:44-49` fabricates `Original { FileId(0) }`**,
  ignoring `parent_source_info` — so attribute spans produced *inside* a nested
  parse are wrong regardless. Only matters if a re-parsed title itself contains
  an attribute; noted, not addressed here.
- **`theorem.rs:317-319` documents the value span incorrectly** (claims it
  excludes the quotes; the grammar includes them). Worth correcting while we
  are here.
