---
title: "Issue #1079: code blocks without a q2 grammar lose the div.sourceCode box"
date: 2026-11-06
description: "Triage of GH #1079. The HTML writer wraps a code block in div.sourceCode only when it has highlight spans; Quarto 1 wraps every block whose language Pandoc recognizes."
---

# Issue #1079: code blocks without a q2 grammar lose the `div.sourceCode` box

- **GitHub**: <https://github.com/quarto-dev/q2/issues/1079>
- **Reporter**: \@cscheid (Carlos Scheidegger)
- **Triage date**: 2026-11-06
- **Worktree**: `.worktrees/issue-1079` (branch `issue-1079`, based on `main` @ `08806ee4e`)
- **Braid strand**: bd-jxzjn4bs (bug)
- **Scope**: the `div.sourceCode` wrapper on HTML code *blocks*. Excludes inline `Code`, the placement of `code-with-copy` (see Open questions), and non-HTML writers.

## Summary

Reproduced. q2's HTML writer emits the Pandoc-style `div.sourceCode > pre.sourceCode > code.sourceCode` nesting only for blocks that carry `data-hl-spans`, which in turn exist only when q2 has a tree-sitter grammar for the block's language (15 built-in grammars plus user grammars). Every other language gets a bare `<pre>`, and since the theme paints the code-block box on `div.sourceCode`, those blocks render as plain text. Quarto 1 (Pandoc 3.10 + skylighting) wraps every block whose class skylighting resolves, by name or by file extension, even when it adds no colors (`default`).

**Decision (maintainer, 2026-11-06):** match Quarto 1 first, i.e. wrap exactly the blocks Quarto 1 wraps; consider broader rules (such as "wrap any block with a language class") later, once there is data from real-world usage.

## Reproduction

Fixture: `claude-notes/issue-reports/1079/repro/` (`_quarto.yml` + `index.qmd`). It extends the issue's example with `conf` (known only by file extension), `text` (unknown to Quarto 1), a `cell-output` block, a `{.cell-code .toml}` block (language not first), and a block with no language.

```bash
# Quarto 1 (quarto dev build, bundled pandoc 3.10)
cd <copy of repro> && quarto render
grep -o '<div class="sourceCode[^"]*"[^>]*>\|<pre class="[^"]*"' _site/index.html

# q2 (main @ 08806ee4e)
cargo run --bin q2 -- render claude-notes/issue-reports/1079/repro
```

Quarto 1's full page is saved as `q1-output.html`; q2's as `q2-output.html`.

| Block | Quarto 1 | q2 (`main`) |
|---|---|---|
| `ini` | `div.sourceCode` > `pre.sourceCode.ini.code-with-copy` | bare `pre.ini.code-with-copy` |
| `default` | `div.sourceCode` > `pre.sourceCode.default.code-with-copy` | bare `pre.default.code-with-copy` |
| `bash` | `div.sourceCode` > `pre.sourceCode.bash.code-with-copy` | `div.sourceCode.code-with-copy` > `pre.sourceCode.bash` |
| `conf` | `div.sourceCode` > `pre.sourceCode.conf.code-with-copy` | bare `pre.conf.code-with-copy` |
| `text` | bare `pre.text` (no copy class) | bare `pre.text.code-with-copy` (matches, apart from the copy class) |
| `{.cell-output .cell-output-stdout}` | bare `pre.cell-output.cell-output-stdout` | bare `pre.cell-output.cell-output-stdout.code-with-copy` (matches, apart from the copy class) |
| `{.cell-code .toml}` | `div.sourceCode.cell-code` > `pre.sourceCode.toml.code-with-copy` | bare `pre.cell-code.toml.code-with-copy` |
| no language | bare `pre` | bare `pre.code-with-copy` (matches, apart from the copy class) |

## Localization

- `crates/pampa/src/writers/html.rs:1530`: `Block::CodeBlock` arm. `highlighted = spans.is_some()` decides between `write_highlighted_codeblock` (wrapper) and a bare `<pre>`.
- `crates/pampa/src/writers/html.rs:655`, `read_hl_spans`: returns `None` for a missing *or empty* span list, so a known-grammar block whose text yields no captures also loses the wrapper.
- `crates/pampa/src/writers/html.rs:589`, `write_highlighted_codeblock`: already handles `spans: None` (writes escaped text), so it can emit a wrapped block without spans as-is. It treats the *first* class as the language.
- `crates/quarto-highlight/src/annotate.rs:191`, `annotate_attr`: picks the first class that resolves to a grammar (any position, like Pandoc) and writes `data-hl-spans`.
- `crates/quarto-highlight/src/registry.rs:152`, `BUILTIN_ALIASES`: q2's 15 grammars.
- `crates/quarto-core/src/stage/stages/code_highlight.rs`: the pipeline stage wrapping `annotate_pandoc`; shared by native and WASM (hub-client), so the fix applies to both.
- Existing test that encodes the current behaviour, which the fix must change: `unhighlighted_codeblock_has_no_div_wrapper` (`html.rs:3112`).

## Open questions — resolved during triage

**Which classes does Quarto 1 wrap?** Experiment: `quarto pandoc --list-highlight-languages` (Pandoc 3.10, 163 names) plus one-block probes through `quarto pandoc -t html`. Results:

- Name match is case-insensitive (`INI`, `Python` wrap).
- File extensions also resolve: `conf`, `cfg`, `txt`, `py`, `sh`, `js`, `md`, `yml`, `Dockerfile` all wrap. This explains `conf` and `txt` in the issue's tally.
- `text`, `plaintext`, `cell-output`, `nginx`, `k` do **not** wrap under 3.10. (`nginx` does wrap under Pandoc 3.12.1, which explains the issue's note that Quarto 1 leaves it bare: the language set depends on the Pandoc version.)
- Quarto 1 passes extra syntax definitions from `src/resources/pandoc/syntax-definitions/` (`k`, `q`, `r`, `markdown`, `scss`) during render.
- Pandoc scans all classes, not only the first: `{.cell-code .toml}` wraps, with `cell-code` moved to the div and `toml` kept on the `pre`.

So "Quarto 1's set" is: skylighting's syntax names and extension globs for the skylighting version in Pandoc 3.10, plus Quarto 1's five extra syntax definitions.

**Is "wrap any block with a language class" safe as an alternative?** No, not without more rules. Engine output blocks carry `cell-output*` classes, which Quarto 1 leaves bare; treating the first class as the language would box every stdout block. The maintainer chose the Quarto 1 set for now.

**Does `code-with-copy` land in the same place?** No, and that is a separate difference. Quarto 1 puts `code-with-copy` on the `pre` only for wrapped blocks. q2 adds it to every decorated block (`crates/quarto-core/src/transforms/code_block_generate.rs:258`), and for wrapped blocks the writer moves it to the `div` as a non-language class. Nothing in q2's CSS keys off the class today (per the comment there), so it does not affect the box. Not in scope for this fix.

## Outcome / recommended next step

Filed **bd-jxzjn4bs**. The fix landed on this branch following steps 1 to 3 below; see `claude-notes/plans/2026-11-06-sourcecode-wrapper-known-languages.md`. The language-ordering point in step 3 became its own strand, bd-hodntkwd. Recommended fix shape:

1. **Known-language table in `quarto-highlight`.** Add a static, generated list of the class names and file extensions that skylighting (Pandoc 3.10) resolves, plus `k`, `q`, `r`, `markdown`, `scss`, with case-insensitive lookup (e.g. `pub fn is_pandoc_known_language(class: &str) -> bool`). Generate it once from the skylighting-core syntax XMLs at the version Pandoc 3.10 pins (names from `name`/file stem, extensions from the `extensions` attribute), and record the source version in a comment. Probing `quarto pandoc` is a cross-check, not the source.
2. **Mark known-but-ungrammared blocks in the annotate step.** In `annotate_attr` (block path only), when no grammar resolves but some class is a known language, record that. The least invasive encoding is to write an *empty* span list into `data-hl-spans`, so no new attribute is needed. Keep "filter-authored annotations win".
3. **Writer:** in the `Block::CodeBlock` arm, wrap when `data-hl-spans` is present, even if empty. Split `read_hl_spans` so "present but empty" is distinct from "absent". This also fixes known-grammar blocks with zero captures. Make sure the language that `write_highlighted_codeblock` puts on `pre`/`code` is the *resolved* class, not blindly the first class, to match Quarto 1's `{.cell-code .toml}` output.
4. **Tests:** writer unit tests (empty spans → wrapper; absent → bare); annotate tests (`ini`, `default`, `INI`, `conf`, `{.cell-code .toml}` wrap; `text`, `cell-output`, no class stay bare); an integration test rendering this repro and checking the table above. The `wasm.md` rule does not apply (no `pampa/src/lua` files touched), but the stage also runs in hub-client, so check the preview there.

Out of scope, possible follow-ups: inline `Code` with a known language (Pandoc gives it `code.sourceCode`), `code-with-copy` placement, and the "wrap anything with a language class" rule the issue suggests, revisited with real-world data.

## Verification commands used

```bash
gh issue view 1079 --repo quarto-dev/q2 --json title,body,author,labels,comments
cargo xtask verify --skip-hub-build          # pre-flight on main @ 08806ee4e: Rust 17883 passed;
                                             # hub-client test:wasm failed (citations, highlighting smoke)
                                             # against a stale WASM bundle built Oct 31, 147 commits back
cd hub-client && npm run build:wasm && npm run test:wasm   # 465 passed (44 files): HEAD is green
quarto pandoc --version                      # pandoc 3.10
quarto pandoc --list-highlight-languages     # 163 names
for l in ini default conf txt text nginx k q Dockerfile; do
  printf '```%s\nx\n```\n' "$l" | quarto pandoc -f markdown -t html | head -1
done
quarto render                                # in a copy of repro/ → q1-output.html
cargo run --bin q2 -- render claude-notes/issue-reports/1079/repro   # → q2-output.html
```

## Cross-references

- bd-jxzjn4bs: the fix.
- `crates/pampa/src/writers/html.rs` tests around line 2994 ("div.sourceCode wrapper for highlighted code blocks").
- `external-sources/quarto-cli/src/resources/pandoc/syntax-definitions/`: Quarto 1's extra syntaxes.
- `external-sources/quarto-cli/configuration`: `PANDOC=3.10`.
