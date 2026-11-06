# Issue #1081: built-in highlighting for qmd / markdown / md code blocks

- **GitHub**: https://github.com/quarto-dev/q2/issues/1081
- **Reporter**: @cscheid (Carlos Scheidegger), 2026-11-05
- **Triage date**: 2026-11-06
- **Worktree**: `.worktrees/issue-1081` (branch `issue-1081`, based on `main` @ `08806ee4e`)
- **Braid strand**: none new. This is a duplicate of bd-wf4swupu (markdown grammar) plus bd-v9pi6ty9 (injections), both under epic bd-98m98wg8. See Outcome.
- **Scope**: the whole issue: `qmd` highlighting, the `markdown`/`md` aliases, and embedded front matter and code cells. The user-grammar note at the end of the issue is recorded below but not filed.

## Summary

Fenced blocks with class `qmd`, `markdown` or `md` render as plain
`<pre class="<lang> code-with-copy">`. They get no `sourceCode` wrapper and no
highlight spans. Reproduced at `main` @ `08806ee4e`. This is a real gap, and it
is already tracked. Epic bd-98m98wg8 (self-documenting cells) split it into
plan 2, bd-wf4swupu ("Add a markdown grammar to quarto-highlight"), and plan 3,
bd-v9pi6ty9 ("quarto-highlight forbids syntax injections by design"). This
triage adds three findings that change the fix's shape:

1. **The editor already does the composite highlighting the issue asks for.**
   `quarto-lsp-core` highlights qmd in three zones: structural qmd, front-matter
   YAML, and code cells including `#|` options. It calls
   `quarto_highlight::highlight_captures` for the embedded layers. The render
   path can reuse this. It does not need a general injection engine.
2. **The render CSS would not style the qmd query's capture names.** The theme
   matches exact class names.
3. **`tree-sitter-qmd` is the right grammar.** bd-wf4swupu left open whether to
   use it or an upstream tree-sitter-markdown. Its `highlights.scm` is
   maintained and already tuned for exactly this zone split.

## Reproduction

Fixture: `claude-notes/issue-reports/1081/repro/index.qmd`. It contains the
issue's `qmd` and `markdown` blocks, a `md` block, and a `yaml` block as a
control.

`claude-notes/` is itself a q2 project that excludes this path, so render a
copy outside the repo:

```bash
cp claude-notes/issue-reports/1081/repro/index.qmd "$TMP"/ && cd "$TMP"
q2 render index.qmd
grep -o '<pre class="[^"]*"' index.html
grep -o 'class="hl-[^"]*"' index.html | sort | uniq -c
```

Observed:

```
<pre class="qmd code-with-copy"
<pre class="markdown code-with-copy"
<pre class="md code-with-copy"
<pre class="sourceCode yaml"
   1 class="hl-boolean"
   1 class="hl-property"
   1 class="hl-punctuation-delimiter"
```

Every span comes from the yaml control block. The three markdown-family blocks
get none.

Expected: `sourceCode` wrappers and spans for headings, emphasis, links, the
front matter (as YAML) and the nested `{python}` cell (as Python, with its `#|`
line highlighted as YAML).

## Localization

- **The missing registration.** `crates/quarto-highlight/src/registry.rs`:
  `BUILTIN_ALIASES` and `BUILTIN_BUILDERS` have no `qmd`, `markdown` or `md`
  entry. `Registry::resolve` returns `None`, so the annotate pass never adds
  `data-hl-spans`, and the HTML writer (`crates/pampa/src/writers/html.rs`,
  around line 617) leaves the block without `sourceCode`.
- **Injections are forbidden by design.** The `registry.rs` test
  `builtin_configs_have_no_injection_or_locals` fails if any built-in grammar
  carries an injection or locals query. That is because the resolver uses
  `Query.captures()`, not the `tree-sitter-highlight` event stream. This is the
  same blocker bd-v9pi6ty9 describes.
- **The working model to copy.** `crates/quarto-lsp-core/src/tokens.rs`:
  - zone 1: `structural_tokens`, which runs the `tree_sitter_qmd::HIGHLIGHT_QUERY`
    captures and excludes `metadata` and `code_fence_content` interiors;
  - zone 2: `frontmatter_tokens`, which synthesizes the `---` fences and
    highlights the body as YAML;
  - zone 3: `code_cell_tokens`, `cell_language`, `directive_tokens` (the `#|`
    run as YAML) and `embedded_body_tokens`.

  These zones are computed in byte offsets before the conversion to LSP
  line/UTF-16 positions. The zones are disjoint by construction, so composing
  them is just sort-by-start. That is an *explicit* injection. It is exactly
  the "handle injection queries explicitly" option that bd-v9pi6ty9 lists, and
  it has already been built and tested for one grammar.
- **The grammar and query.** `crates/tree-sitter-qmd/bindings/rust/lib.rs`
  exports `LANGUAGE` and `HIGHLIGHT_QUERY`.
  `tree-sitter-markdown/queries/highlights.scm` is the structural query. Its
  header states the "never cover `code_fence_content` / `metadata`" invariant
  that the zone composition relies on.
- **The capture-vocabulary gap.** `capture_to_class` (`html.rs:735`) only
  replaces `.` with `-`. `resources/scss/html/templates/highlight-default.scss`
  styles only `.hl-markup-heading`, `-bold`, `-italic`, `-strikethrough`,
  `-link`, `-link-url` and `-raw`, all as exact classes. The qmd query emits
  `markup.heading.1`…`6`, `markup.strong`, `markup.emphasis`,
  `markup.link.label`, `markup.shortcode`, `markup.math`, `markup.list`,
  `markup.quote`, `markup.comment`, `attribute.specifier`, and so on. Most of
  these would render as unstyled spans. The editor solves the same problem with
  a longest-prefix legend (`crates/quarto-lsp-core/src/types.rs` around line
  619, the `qmd.*` legend). The render path needs a qmd-to-`hl-*` normalization,
  either in the query or as a mapping step. Theme translation
  (`claude-notes/plans/2026-08-18-highlight-theme-translator.md`) should be
  checked against the chosen names.

## Open questions resolved during triage

- **Is this already tracked?** Yes. `braid search "markdown highlight"` finds
  bd-v9pi6ty9, whose parent epic bd-98m98wg8 also lists bd-wf4swupu as plan 2.
  bd-wf4swupu measured the same zero-span output for ```` ```markdown ````.
  Neither strand mentions the `qmd` or `md` class names, or the lsp-core
  composer.
- **tree-sitter-qmd or upstream tree-sitter-markdown?** This was left open in
  bd-wf4swupu. Recommend tree-sitter-qmd. It is already linked into both native
  and WASM builds through pampa, so it adds no new dependency or binary size.
  It parses Quarto constructs (shortcodes, divs, attribute specifiers, cells,
  editorial marks) that upstream does not. Its highlight query is maintained in
  this repo for the editor. Q1 uses Pandoc's generic markdown syntax for
  ```` ```markdown ````, so tree-sitter-qmd is a superset for parity purposes.
- **Why did the reporter's user-grammar attempt only produce spans for
  `**strong**` and punctuation?** Not reproduced. The WASM user-grammar build
  was not rebuilt during this triage. Part of the answer is by design: the
  query deliberately leaves front matter and cell interiors uncaptured (zones 2
  and 3), so those stay plain without the composer. Missing *heading* and
  *link* spans are not explained by CSS, because unstyled captures would still
  emit `hl-markup-heading-1` spans. That points at the user-grammar path (for
  example, a WASM build stale against the query), not at the built-in design.
  This is moot for the fix, because the built-in path doesn't go through the
  user-grammar loader. Separately, even with correct spans, the
  vocabulary gap above would leave most of them uncoloured.
- **The user-grammar loader can't register `qmd`.** Confirmed:
  `user_grammar.rs` resolves `tree_sitter_<file_stem>`, and has no alias
  config (around lines 307 and 342). Not filed. A built-in `qmd` makes it moot
  for this case. It would only matter for a third-party grammar whose exported
  symbol differs from the class name the user wants.

## Outcome and recommended next step

Duplicate of bd-wf4swupu and bd-v9pi6ty9. No new strand. Both strands are
updated with this triage's findings (braid notes). Recommended fix scope for
whoever picks up bd-wf4swupu:

1. Register `qmd` as a canonical built-in with aliases `markdown` and `md`.
   The language is `tree_sitter_qmd::LANGUAGE`; the query is its
   `HIGHLIGHT_QUERY`.
2. Move the byte-level zone composer (zones 1 to 3 plus the `#|` directive
   handling) from `quarto-lsp-core/src/tokens.rs` down into `quarto-highlight`
   as the `qmd` entry's extractor. `quarto-lsp-core` already depends on
   `quarto-highlight`, so lsp-core then consumes it and only does the
   line/UTF-16 conversion. One implementation then serves the editor and
   rendering, so they keep parity. This also delivers bd-v9pi6ty9's property 4
   for markdown without retiring `builtin_configs_have_no_injection_or_locals`:
   the qmd entry stays injection-free at the query level, and its composition
   is explicit code. The test's comment should be updated to say so.
3. Add a capture-name normalization for the render path, either in the query
   or as a mapping step: `markup.heading.N` to `markup-heading`, `markup.strong`
   to `markup-bold`, `markup.emphasis` to `markup-italic`, and so on. Add CSS
   rules for the qmd-specific roots (shortcode, math, attribute specifier).
4. Tests: a `registry.rs` unit test for each alias, an editor/render parity test
   for a qmd block (mirroring `code_cell_parity_with_render`), and a render
   smoke test using the fixture here.

Recommend responding on the GH issue that it is tracked as bd-wf4swupu and
bd-v9pi6ty9.

## Verification commands used

```bash
cargo xtask verify --skip-hub-build      # main @ 08806ee4e: all steps passed
# The worktree branch is the same commit, so the cold re-verify inside it was skipped.
gh issue view 1081 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
braid search "qmd highlight"; braid search "markdown highlight"; braid search 1081
braid show bd-v9pi6ty9; braid show bd-98m98wg8; braid show bd-wf4swupu
cargo build -p quarto && q2 render index.qmd   # on a copy of repro/index.qmd
grep -o '\.hl-[a-z0-9-]*' resources/scss/html/templates/highlight-default.scss | sort -u
```

## Cross-references

- bd-98m98wg8 (epic: self-documenting cells), bd-wf4swupu (plan 2: markdown
  grammar), bd-v9pi6ty9 (plan 3: injections)
- `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`
- `claude-notes/plans/2026-06-10-monaco-tree-sitter-highlighting.md` (the
  editor's zone design)
- `claude-notes/plans/2026-08-18-highlight-theme-translator.md`
