# Issue #182 — Reader drops whitespace between `pandoc_code_span` and `html_element` siblings

- **GitHub**: https://github.com/quarto-dev/q2/issues/182
- **Reporter**: @rundel (Colin Rundel), 2026-05-11
- **Triage date**: 2026-05-12
- **Worktree**: `.worktrees/issue-182` (branch `issue-182`, based on `main` @ `16a8d67c`)
- **Beads issue**: bd-nkx4
- **Scope**: covers the *whitespace-loss* part of the report (reporter's follow-up comment). Explicitly does **not** attempt to fix the underlying ambiguity of two adjacent Code/RawInline spans with no separator in the AST (cscheid's first reply). That case has no unambiguous qmd surface form and isn't the user's actual complaint.

## Summary

The reporter's qmd-web sources contain inputs like:

```
See `func()` <a href="x">link</a> done.
```

with a real space between the closing backtick of the inline code span and the `<` of the bare HTML. After parsing, the AST has `Code` immediately followed by `RawInline` with no intervening `Space`, so the qmd writer emits `` `func()``<a href="x">`{=html}…``` and the result is unparseable. The bug is in the **reader**: the space is being dropped during tree-sitter → AST conversion. The writer is innocent.

The fix is small and local: the `"html_element"` branch in `crates/pampa/src/pandoc/treesitter.rs` already handles leading/trailing whitespace correctly for the anchor-shorthand case (it splits the whitespace out into adjacent `Space` inlines). The same handling is missing from the sibling RawInline case in the same `match` arm.

## Reproduction

Fixtures live at:

- `claude-notes/issue-reports/182/repro-with-space.qmd` — `See \`func()\` <a href="x">link</a> done.\n`
- `claude-notes/issue-reports/182/repro-no-space.qmd`   — `See \`func()\`<a href="x">link</a> done.\n`

### Observed (pampa @ `16a8d67c`)

Both inputs produce the **same** AST — the space is gone:

```
$ cargo run --bin pampa -- < repro-with-space.qmd
[ Para [ Str "See", Space, Code ( "" , [] , [] ) "func()"
      , RawInline (Format "html") "<a href=\"x\">"
      , Str "link", RawInline (Format "html") "</a>"
      , Space, Str "done." ] ]
```

(Identical when run on `repro-no-space.qmd`.)

The writer then collapses `Code` against `RawInline`, and the resulting qmd fails to re-parse:

```
$ cargo run --bin pampa -- -t qmd < repro-with-space.qmd
See `func()``<a href="x">`{=html}link`</a>`{=html} done.

$ cargo run --bin pampa -- -t qmd < repro-with-space.qmd | cargo run --bin pampa --
Error: Parse error (offset 13, near ``)
```

### Pandoc reference behaviour

Pandoc *does* distinguish the two inputs and inserts a `Space` between `Code` and the `RawInline` when one was in the source:

```
$ pandoc -f markdown -t native < repro-with-space.qmd
[ Para [ Str "See", Space, Code ( "" , [] , [] ) "func()"
      , Space                 ← present
      , RawInline (Format "html") "<a href=\"x\">"
      , Str "link", RawInline (Format "html") "</a>"
      , Space, Str "done." ] ]

$ pandoc -f markdown -t native < repro-no-space.qmd
[ Para [ Str "See", Space, Code "func()"
      , RawInline (Format "html") "<a href=\"x\">"   ← no Space
      , Str "link", RawInline (Format "html") "</a>"
      , Space, Str "done." ] ]
```

So fixing the reader to preserve the space brings us into line with Pandoc on the "with space" form. The "no space" form (cscheid's original reply) remains ambiguous and is out of scope here.

## Localization

The bug lives in **`crates/pampa/src/pandoc/treesitter.rs`**, in the `"html_element"` arm of the inline visitor (≈ lines 1076–1187 on `main` @ `16a8d67c`).

### Tree-sitter behaviour observed (with `-v`)

```
pandoc_code_span: {Node pandoc_code_span (0, 3) - (0, 12)}
  code_span_delimiter: (0, 3) - (0, 5)     ← includes the leading space at col 3
  code_span_delimiter: (0,11) - (0,12)
html_element:      {Node html_element (0,12) - (0,25)}   ← includes leading space at col 12
html_element:      {Node html_element (0,29) - (0,33)}
```

Both adjacent inline nodes claim a piece of the surrounding whitespace. The code-span helper handles its half cleanly (see below); the html_element handler doesn't handle its half at all in the RawInline path.

### How the analogous (working) cases handle it

1. **`pandoc_code_span` opening delimiter** — `crates/pampa/src/pandoc/treesitter_utils/code_span_helpers.rs:43-57, 109-113`. The helper inspects the opening `code_span_delimiter`; if it starts with ASCII whitespace it emits an explicit `Space` inline *before* the `Code` node. (This is why `See ` ends up as `Str "See", Space, Code …` and not `Str "See", Code …`.) Comment on line 45 confirms: *"The closing delimiter never includes trailing space in the grammar"* — so the trailing-side whitespace is always owned by the following sibling node.

2. **`html_element` → anchor shorthand `<#id>`** — `crates/pampa/src/pandoc/treesitter.rs:1084-1162`. Computes `leading_ws` and `trailing_ws` from `raw_text.len() - raw_text.trim_start().len()` (resp. `trim_end`), pushes a `Space` inline before the synthesized `Link` if `leading_ws > 0`, and pushes another after if `trailing_ws > 0`. This is exactly the pattern needed for the RawInline branch.

### The buggy branch

`crates/pampa/src/pandoc/treesitter.rs:1076-1187`:

```rust
"html_element" => {
    let raw_text = node.utf8_text(input_bytes).unwrap();
    let text = raw_text.trim().to_string();                       // ← silently drops both edges
    if let Some(anchor_id) = parse_anchor_shorthand(&text) {
        // anchor branch: correctly splits whitespace into Space inlines
        …
    } else {
        // RawInline branch: emits a single RawInline, leading/trailing whitespace gone
        PandocNativeIntermediate::IntermediateInline(Inline::RawInline(RawInline {
            format: "html".to_string(),
            text,
            source_info: node_source_info_with_context(node, context),
        }))
    }
}
```

The RawInline branch needs the same leading_ws/trailing_ws → `IntermediateInlines(…)` splitting that the anchor branch already does.

## Open questions — resolved during triage

**Q1.** Is the root cause in the writer (failing to add a separating space) or in the reader (dropping an existing space)?

*Experiment.* Compared the AST emitted by `cargo run --bin pampa --` on the with-space and no-space fixtures. Both ASTs are byte-identical and contain no `Space` between `Code` and `RawInline`. The reader is collapsing the two inputs into one AST; the writer cannot recover information that isn't there.

*Conclusion.* Root cause is in the reader. The writer is doing the best it can with the AST it gets.

**Q2.** Does this require redesigning whitespace handling across the parser, or is there a local fix?

*Experiment.* Read the visitor for `"html_element"` and the helper for `"pandoc_code_span"`. Found that:

- The code-span helper already cleanly handles its half (leading whitespace via the opening delimiter; closing delimiter never includes trailing space, per the comment in code_span_helpers.rs:45).
- The anchor-shorthand branch of the html_element visitor already implements the leading_ws/trailing_ws → adjacent-`Space` pattern.

*Conclusion.* Local fix is feasible: mirror the anchor branch's leading_ws/trailing_ws handling into the RawInline branch of the same `match` arm. No cross-parser whitespace redesign is required.

**Q3.** Does fixing this regress the "no separator" ambiguity case (cscheid's original reply)?

*Experiment / reasoning.* The fix only adds `Space` inlines when the *html_element node text* actually contained leading/trailing whitespace in the source. The pathological case (`` `foo``<a> ``: two adjacent inline nodes with no whitespace between them) still emits an AST with no `Space`, which is the correct representation and still has no unambiguous qmd surface form. Reporter agrees the "no space" case is a separate, harder problem and out of scope (issue #182 thread, 2026-05-12).

*Conclusion.* No regression for the "truly adjacent" case. Round-trip there remains a known-impossible problem.

## Outcome / recommended next step

**Filed as bd-nkx4** for the reader-side fix. Scope (recommended for the implementer, not part of triage):

1. **Test first** (per `crates/pampa/CLAUDE.md` TDD rule):
   - Add `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/code_space_html_inline.qmd` (or similar) containing `` See `func()` <a href="x">link</a> done. `` to lock in the round-trip.
   - Add a focused AST test asserting that `Code` followed by an `html_element` separated by whitespace produces `Code, Space, RawInline` (mirror of an existing anchor-branch test).
   - Verify both fail before any code change.
2. **Fix** in `crates/pampa/src/pandoc/treesitter.rs` `"html_element"` arm: in the RawInline (non-anchor) branch, compute `leading_ws` / `trailing_ws` from `raw_text` and return `IntermediateInlines(vec![Space?, RawInline, Space?])` instead of a single `IntermediateInline`. The anchor branch above (lines 1084–1162) is the model — adapt the `Space` source-info construction directly.
3. **Verify**:
   - `cargo nextest run -p pampa` (and workspace, per CLAUDE.md step 5).
   - `cargo xtask verify --skip-hub-build` (Rust-only change; no `quarto-core`/`pandoc-types` touched).
   - End-to-end check: both reproduction fixtures here should now AST → qmd → AST round-trip cleanly and the qmd intermediate should contain the separating space.
4. **Watch for snapshot churn.** Any existing snapshot test whose qmd contained `<tag>` adjacent to whitespace will likely have its AST change from `RawInline` to `Space, RawInline` (or `RawInline, Space`). Each change needs to be inspected — if a snapshot starts emitting an *extra* `Space` that wasn't in the source, that is the fix going too far.

I will respond on the GH issue once the beads issue is filed, linking it and confirming we accept the reporter's framing of the bug.

## Verification commands used

```bash
# Issue body + comments
gh issue view 182 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments

# Pre-flight (green at 16a8d67c)
cargo xtask verify --skip-hub-build

# Reproduction
cargo run --bin pampa -- < claude-notes/issue-reports/182/repro-with-space.qmd
cargo run --bin pampa -- -t qmd < claude-notes/issue-reports/182/repro-with-space.qmd
cargo run --bin pampa -- -t qmd < claude-notes/issue-reports/182/repro-with-space.qmd \
  | cargo run --bin pampa --

# Pandoc reference
pandoc -f markdown -t native < claude-notes/issue-reports/182/repro-with-space.qmd
pandoc -f markdown -t native < claude-notes/issue-reports/182/repro-no-space.qmd

# Tree-sitter CST
cargo run --bin pampa -- -v < claude-notes/issue-reports/182/repro-with-space.qmd 2>&1 \
  | grep -E "(html_element|code_span)"
```

## Cross-references

- `crates/pampa/src/pandoc/treesitter.rs:1076` — `"html_element"` arm (bug site).
- `crates/pampa/src/pandoc/treesitter.rs:1084-1162` — anchor branch (working model to copy).
- `crates/pampa/src/pandoc/treesitter_utils/code_span_helpers.rs:43-57, 109-113` — code span side, also a working model; note line 45 comment confirming the closing delimiter never carries trailing whitespace (so the html_element is the right place to emit the post-Code space).
- `crates/pampa/CLAUDE.md` — TDD-first rule for parser fixes.
