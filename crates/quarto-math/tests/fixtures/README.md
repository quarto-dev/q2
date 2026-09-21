# quarto-math fixture corpus

One TeX math construct per file. The loader and the guard test that keep
this directory well-formed live in `tests/integration/fixture_corpus.rs`;
every snapshot test in the crate iterates the same corpus through it.

## Conventions

- `<group>/<name>.tex` is inline math; `<group>/<name>.display.tex` is
  display math. Nothing else about the file name carries meaning.
- The file's bytes are the math **text** exactly as pampa hands it to
  `quarto-math` (no `$` delimiters). One trailing newline is stripped by the
  loader so files may end in a newline. Multi-line inline fixtures are
  legitimate: pampa keeps the literal `\n` for `$…$` spanning lines.
- Names are kebab-case and unique within their group.
- `errors/` holds inputs that must produce a diagnostic and must never panic.
  A snapshot there records the diagnostic, not a "correct" rendering.

## Groups

| Group | What it covers |
| --- | --- |
| `basic` | words, numbers, relations, primes, comments, Unicode input, brace groups |
| `fractions` | `\frac` family, `\over`, `\binom`, `\cfrac`, single-char and command arguments |
| `radicals` | `\sqrt` with and without index, nested, bare argument |
| `scripts` | `_` / `^` in every order and nesting, scripts on groups, fractions, parens |
| `operators` | big operators with limits (inline and display), `\limits`/`\nolimits`, function names, `\operatorname` |
| `delimiters` | `\left…\right` (incl. `.` and `\middle`), `\big…\Bigg`, bare and floor/ceil delimiters |
| `text-and-styles` | `\text` family, `\mathrm`/`\mathbf`/`\mathbb`/…, `\boldsymbol`, style switches, color |
| `accents` | hats, bars, arrows, braces, `\overset`/`\underset`, `\not`, `\cancel` |
| `environments` | `aligned`, `cases`, matrices, `array` (incl. `\hline`), `gathered`, `split`, `align*`, ragged and empty cells, nesting |
| `macros` | `\newcommand` / `\renewcommand` / `\def` with 0, 1, 2 and optional args; multi-line; unused |
| `spacing` | `\,` `\;` `\:` `\!` `\quad` `\qquad` `\ ` `~` `\hspace` `\phantom` `\mkern` |
| `symbols` | arrows, relations, binary operators, Greek, dots, set/logic symbols, escaped specials |
| `corpus` | expressions copied verbatim from `docs/`, `external-sources/quarto-cli/tests/docs` and quarto-web |
| `errors` | unknown commands/environments, unbalanced braces and delimiters, double scripts, macro misuse, empty input |

## Provenance of `corpus/`

Mined on 2026-09-21 with a regex sweep over `.qmd` files (fenced code
excluded): 69 expressions in `docs/`, 136 in the Quarto 1 test corpus, 21 in
quarto-web, plus the in-tree pampa and smoke-all fixtures. Commands seen, by
frequency: `\frac` 49, `\mathrm` 37, `\partial` 24, Greek letters, `\left`/
`\right` 8, `\int` 8, `\sqrt` 5, `\vec` 5, `\lim` 3, `\hat` 2, `\cancel` 2;
the only environment in the wild was `aligned`. Each `corpus/` file is one of
those expressions, unedited apart from whitespace at the ends. When a real
document surfaces a construct the corpus lacks, add it here first.
