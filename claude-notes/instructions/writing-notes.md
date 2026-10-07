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
8. Bare brackets as editorial marks: `[REVISED]`, `[sic]`. They become spans
   with a warning; write `\[REVISED\]`.
9. Reference-style links `[text][1]`. Only inline links work.
10. A heading underlined with `===` or `---`. It silently becomes a paragraph;
    use `#`.

Whitespace-flanked `*`, `~16`, `x^2` and a bare `@` (`` `main` @ `sha` ``) are
literal text and need nothing. `snake_case` is fine.

A backslash before any syntax character is always safe (`\A` is a literal `A`),
so when in doubt, escape.

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
