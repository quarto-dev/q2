# Shortcodes in footnote definitions and `lst-cap` (bd-xjg7vl6c)

Strand: `bd-xjg7vl6c`. Blocks `claude-notes/plans/2026-10-03-disable-lua-shortcodes.md`
(bd-2uva9urq): that plan's T2 must not land until this is fixed.

## Problem

Rust's `shortcode_resolve` does not expand `{{{< meta x >}}}` in three qmd-reachable contexts where
Quarto 1 does:

1. `NoteDefinitionPara` (`[^1]: Note {{{< meta author >}}}.`).
2. `NoteDefinitionFencedBlock` (q2's fenced form `::: ^id`).
3. The `lst-cap` attribute on a listing code block (`{#lst-a lst-cap="… {{{< meta author >}}}"}`).

Cause of 1 and 2: both are "leaf" arms in the two body walkers (`resolve_block`, `stamp_block`).
`FootnotesTransform` runs after `ShortcodeResolveTransform` and moves the definition content into
`Inline::Note` unchanged, so nothing resolves it later. Inline notes (`Inline::Note`) were already walked.

Cause of 3: `CodeBlock` attribute values are not expanded (matching Q1), but
`FloatRefTargetSugarTransform` runs after the shortcode pass and parses the `lst-cap` string into the
caption, so a literal `{{{< … >}}}` becomes an unresolved `Inline::Shortcode` that nothing resolves.
Q1 expands it ("Listing 1: LC Ann").

The pandoc path hides 1 and 2 today only because the vendored Lua shortcodes pass catches them.

## Evidence (Q1 parity, 2026-10-04)

Read from Q1's `shortcodes.lua` (the vendored copy is byte-identical) and confirmed with renders of
a Q1 dev build:

| Context | Q1 expands? |
|---|---|
| Footnote definition `[^1]: …` | yes |
| `lst-cap` | yes |
| Link/image title, CodeBlock/Code attr values, Table attr values, Cite prefix/suffix | **no** (leaks a placeholder, drops the value, or fails to parse as a citation) |
| Div/Span/Header/Link/Image attrs, link URL, image src | yes (q2 already matches) |

So only items 1-3 are regressions. The others match Q1 and stay unexpanded.

Out of scope: the `::: ^id` fenced footnote syntax is a deliberate q2 difference (indented
multi-paragraph footnotes are a code block in q2, Q-2-35). It is not changed; the fix only makes its
body expand like the single-paragraph form.

## Detection test

`crates/quarto-core/tests/integration/shortcode_all_contexts.rs` (one parameterized test, ~65 qmd
contexts rendered to native HTML with `author: Ann`). Cases carry `known_gap`; a failing case without a
marker, or a marked case that passes, fails the test.

## Checklist

- [x] Parameterized detection test written and green with the gaps marked (`known_gap`).
- [x] Q1 parity check for every gap the test found.
- [x] Walk `NoteDefinitionPara` / `NoteDefinitionFencedBlock` in `resolve_block` (`resolve_inlines` /
      `resolve_blocks`) and in `stamp_block` (provenance), and fix the "don't contain inlines" comment.
- [x] Expand shortcodes in the `lst-cap` attribute value of `CodeBlock` in `resolve_block`. Only that
      key; other code-block attrs stay literal (Q1 parity). Not gated by the code-text opt-out
      (`shortcodes="false"` / `.cell-code`), since the caption is prose, not code text.
- [x] Retag the test: footnote and `lst-cap` cases lose `known_gap`; the Q1-parity cases (titles,
      CodeBlock/Code/Table attrs, cite prefix/suffix) become "expected unexpanded (matches Q1)" assertions
      that fail if they start expanding (marker `Q1_LITERAL`). A `#| lst-cap:` cell-option case was
      not added: it needs an executed cell (engine), which a native-HTML test cannot assume.
- [x] Unknown shortcode / unknown meta key in a footnote definition gives `?name` / `?meta:key` plus
      Q-16-3 / Q-16-5 (the strand text said Q-16-5 for an unknown shortcode name; the real code is Q-16-3).
- [x] Gates (workspace: 15733 run, 15733 passed, 202 skipped; identical to the HEAD-plus-test baseline): `cargo clippy -p quarto-core --all-targets -- -D warnings`,
      `cargo nextest run -p quarto-core`, then one `cargo nextest run --workspace` against a baseline
      measured at HEAD.
- [x] Commit (explicit paths only), then comment on and close `bd-xjg7vl6c`, noting the corrected
      Q-code and the Q1-parity decisions; update the disable-Lua plan's prerequisite note.
