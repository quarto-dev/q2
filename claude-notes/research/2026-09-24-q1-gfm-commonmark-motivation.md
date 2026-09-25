# Q1's gfm → `commonmark+<8 extensions>` writer string: motivation archaeology

**Date:** 2026-09-24
**Context:** Plan decision **D7** in
`claude-notes/plans/2026-09-24-pandoc-hybrid-long-tail-formats.md` — should q2
send bare `-t gfm` to pandoc, or reproduce Q1's exact writer string?
**Conclusion:** no principled reason for the `commonmark` base survives
inspection; bare `-t gfm` is correct on modern pandoc. Gordon approved bare
gfm the same day.

## The question

Q1 (`external-sources/quarto-cli/src/format/markdown/format-markdown.ts:32`,
`format-markdown-consts.ts`) renders `gfm` as:

```
-t commonmark+autolink_bare_uris+emoji+footnotes+gfm_auto_identifiers
  +pipe_tables+strikeout+task_lists+tex_math_dollars
```

That is odd on its face: pandoc's `gfm` writer exists precisely to bundle
GitHub-Flavored-Markdown defaults. Why spell out an extension list against
`commonmark` instead?

## Timeline (git history via `gh api repos/quarto-dev/quarto-cli/commits?path=...`

— the local `external-sources/quarto-cli` clone is shallow, so all archaeology
ran against the GitHub API):

| Date | Commit | Change |
| --- | --- | --- |
| 2021-01-01 | (initial commit) | Plain `--to gfm`. |
| 2021-10-04 | `eee09d8f` | Adds `+footnotes`. |
| 2022-05-20 | `acbe475f` | Now `gfm+footnotes+tex_math_dollars-yaml_metadata_block`. |
| 2022-09-25 | `1ad7b46a` | Format-registry refactor; base switches `gfm` → `commonmark`. |
| 2022-09-29 | `342ff1ee` | The frozen 8-extension list above lands on the `commonmark` base. |
| 2022-09-29 | `c6feffaa` / `8ad60b0c` | Variant-forwarding attempted and reverted the same day. |

**No commit message or linked issue states a principled reason for the
`commonmark` base.** Searched commit messages and issues around all six
touches.

## Why each piece was (probably) there

Cross-referenced against pandoc's changelog:

- **`+footnotes` (2021-10-04)** — added three weeks *before* pandoc 2.15
  (2021-10-23) enabled footnotes in gfm's defaults. A workaround for the
  bundled pandoc of the time.
- **`+tex_math_dollars` (2022-05-20)** — added three months *before* pandoc
  2.19 (2022-08-03) enabled it in gfm's defaults. Same story.
- **`-yaml_metadata_block` (2022-05-20)** — the one **deliberate** deviation:
  Quarto parses and manages front-matter YAML itself and does not want pandoc
  consuming/emitting YAML metadata blocks.
- **The `commonmark` base (2022-09-25/29)** — landed mid-refactor; both
  commits and the reverted variant-forwarding experiment suggest churn around
  how writer strings/variants were represented, not a semantic decision.
  Nothing documents why `commonmark` replaced `gfm` as the base.

## pandoc 3.11 gfm defaults vs Q1's frozen list

`src/Text/Pandoc/Extensions.hs:395-409` — `getDefaultExtensions "gfm"`:

> pipe_tables, raw_html, auto_identifiers, gfm_auto_identifiers,
> autolink_bare_uris, strikeout, task_lists, emoji, yaml_metadata_block,
> footnotes, tex_math_dollars, tex_math_gfm, alerts

(`getDefaultExtensions "commonmark"` = `[raw_html]` only; the changelog
anchors for the late additions are 2.13 yaml_metadata_block, 3.1.9
tex_math_gfm, 3.1.10 alerts.)

Q1's string = `commonmark` + its 8 extensions = the same set **minus**
`yaml_metadata_block`, `auto_identifiers`, `tex_math_gfm`, `alerts`.

So on pandoc 3.11, **bare `-t gfm` is a strict superset of Q1's string**. The
delta is the deliberate YAML subtraction plus three extensions Q1's frozen
list predates.

## Conclusion

Q1's string is (a) workarounds for pandoc versions 2.15/2.19+ have since
obsoleted, plus (b) one deliberate `-yaml_metadata_block`, frozen in
September 2022 and never revisited. Reproducing it in q2 would freeze 2022
GitHub and re-implement pandoc's own packaging for no benefit. Bare `-t gfm`
tracks pandoc's evolving GFM defaults instead — accepted tradeoff (snapshot
churn on pandoc upgrades; YAML metadata blocks now possible in gfm output,
which Phase 6 documents as a deviation).
