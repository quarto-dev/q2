# Writing notes and plans that render under q2

`claude-notes/` is a Quarto 2 website (`claude-notes/_quarto.yml`, strand
bd-uk8zgkha). Every `.md` under it is a qmd document: plans, research,
instructions, this file. Write them in qmd, not in the Markdown you would
write for GitHub or Pandoc.

The user-facing list of differences is the docs page "Migrating Markdown to
Quarto 2" (`docs/guides/authoring/migrating-markdown.qmd`;
`q2 agents-info guides/authoring/migrating-markdown.md` prints it). Read it once.
The habits that most often break a note here, in order of frequency:

1. An apostrophe right after a code span: `` `a.rs`'s `` is an unclosed quote.
   Write `` `a.rs`\'s ``, or reword ("the `a.rs` function").
2. A plural possessive: `the engines' state` is a close quote without an open.
   Write `engines\'`.
3. A `$` that is not math: `$5 and $10`, `$ cargo run`. Write `\$`, or put the
   command in a code span or fence.
4. Underscore filenames in prose: `_quarto.yml and _brand.yml` pairs into
   emphasis. Write `\_quarto.yml`, or use code spans (better: they are filenames).
5. Braces in prose: `{ a, b }`, `{python}`. Code span or `\{ \}`.
6. A code block indented four spaces. Use a fence; q2 does not support indented
   code (Q-2-35).
7. A stray backtick, or a code span that contains a backtick with single-backtick
   delimiters. Use double backticks around a span that contains one.
8. Bare brackets as editorial marks: `[REVISED]`, `[sic]`, `**[Q-1]**`,
   `[[a-ref]]`. They become spans with a warning (Q-2-49); write `\[REVISED\]`.
   A code expression (`argv[1]`, `[2, 4]`, `["mermaid"]`) goes in a code span
   instead. A checkbox state q2 does not know, `- [~]` or `- [-]`, is the same
   warning; write `- \[\~\]`. And `- [x]**bold**` with no space after `]` is not
   a task item at all.
9. Reference-style links `[text][1]`. Only inline links work.
10. A heading underlined with `===` or `---`. It silently becomes a paragraph;
    use `#`.
11. A multi-paragraph footnote written Pandoc-style (`[^1]: First.` and an
    indented second paragraph). Write it as a block footnote: `::: ^1` on its
    own line, the paragraphs, `:::`.
12. Continuation lines of a `- [x]` item indented six spaces to sit under the
    text. The item's content column is 2 (`[x]` is content), so a paragraph after
    a blank line, a fenced block or a sub-list at column 6 is an *indented code
    block*. Indent continuations two spaces. `scripts/q2-dedent-task-items.py`
    re-indents an existing file.
13. A fenced block that contains another fence (a Rust raw string holding a
    ```` ```{r} ```` cell, a Markdown example with its own fences). Use four
    backticks for the outer fence. One diagnostic looks through it on purpose:
    an inner ```` ```{{r}} ```` opener (the Quarto 1 doubled-brace form) still
    reports Q-2-50, so a note that documents that idiom opts out in its front
    matter, `diagnostics: {Q-2-50: {level: off, reason: "..."}}`.
14. A shortcode quoted in a code span or a fenced code block. Shortcodes
    expand there on purpose (textual inclusion of snippets), so a quoted
    `{{< include "x.qmd" >}}` reports Q-17-2 and `{{{< meta title >}}}` reports
    Q-16-5 and renders as `?meta:title`. In a span or in prose write it with
    triple braces, `{{{< meta title >}}}` (renders with double); for a block,
    open the fence as `` ```{.yaml shortcodes="false"} `` and leave its content
    alone. A span that must show the triple-brace form takes the same
    attribute: `` `{{{< x >}}}`{shortcodes="false"} ``{shortcodes="false"}.
15. Angle brackets in prose (Q-2-9): q2 reads `<x>` as an HTML tag and passes it
    through, so the reader sees nothing. A generic type goes in a code span
    (`` `Vec<String>` ``, the whole expression). A placeholder escapes the `<`:
    `"Authenticated as \<email>"`. HTML you actually want rendered is a raw
    inline, `` `<br>`{=html} `` (with a space on each side when it sits between
    two code spans), or a tag-only line becomes a ```` ```{=html} ```` block.

Whitespace-flanked `*`, `~16`, `x^2` and a bare `@` (`` `main` @ `sha` ``) are
literal text and need nothing. `snake_case` is fine.

A backslash before any syntax character is always safe (`\A` is a literal `A`),
so when in doubt, escape.

## Front matter on plans

Every plan in `claude-notes/plans/` starts with front matter giving at least
`title:`, `date:` and `description:` (one sentence on what the work is, shown
in the plans table); the plans listing (`plans/index.md`) is built from them.
The title block renders the title, so do not repeat it as a `#` heading.

```yaml
---
title: 'Fix `_scope`: lexical regression (bd-XXXX)'
date: 2026-10-09
description: 'Restores `_scope: lexical` handling, which broke when metadata strings began parsing as markdown.'
---
```

Single-quote the title (write a `'` inside it as `''`): in single quotes a
backslash escape such as `\@` reaches the markdown parser unchanged, while in
double quotes YAML rejects it. The title is markdown, so code spans work.
`scripts/claude-notes-plan-frontmatter.py` adds both keys to a plan that has
only a `#` heading and a dated filename.

## Checking

Render the file you wrote:

```sh
q2 render claude-notes/plans/2026-10-07-my-plan.md
```

It reports each problem with a location, and for "unclosed" errors the opening
delimiter. For the whole tree, `scripts/q2-render-tally.py claude-notes` tallies
diagnostics by code (`--list Q-2-7` shows each occurrence with its source line).
`scripts/claude-notes-escape-fixpoint.sh` applies the reviewed mechanical escapes
for the classes above; run it after merging a branch that added notes.

## A stubborn block

When a block fails and the cause is not obvious, let Pandoc read it and write it
back in qmd:

```sh
pandoc -f commonmark_x -t json block.md | cargo run -q --bin pampa -- -f json -t qmd
```

Pandoc treats every incomplete delimiter as literal text and the pampa writer
escapes it the way q2 wants. Splice the smallest block that still contains what
it references (a footnote definition, say), and diff before accepting: the
output is normalized (`__x__` becomes `**x**`, `-` list markers become `*`,
tables are re-padded, `—` becomes `---`). Use `commonmark_x`, not `markdown`:
Pandoc's `smart` extension rewrites "e.g. " into a non-breaking space.
