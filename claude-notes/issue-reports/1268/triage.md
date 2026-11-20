# Issue #1268 — rich-text italic around existing bold writes `***`

- **GitHub**: https://github.com/quarto-dev/q2/issues/1268
- **Reporter**: cscheid (Carlos Scheidegger), 2026-11-19
- **Triage date**: 2026-11-20
- **Worktree**: `.worktrees/issue-1268` (branch `issue-1268`, based on `main` @ `4ca24c110`)
- **Braid**: bd-szy5r3wk (the bug, P1); bd-3l05bz1g (related enhancement, P3)
- **Scope**: the single report in the issue ("add italics to a paragraph with a bold word inside").

## Summary

Italicising a span that contains an existing bold word in the q2 preview's
rich-text editor writes `This is *a* ***Quarto*** *website*.`, which is not
valid qmd. Reproduced in Chrome and natively. The bug is in the **incremental
qmd writer**, not in the TypeScript rich-text serializer. When reconciliation
keeps an original `Strong` and recurses into its changed children, the writer
copies the original `**` delimiters and writes the new inner `Emph` with an
empty emphasis stack. The inner `Emph` therefore picks `*` instead of `_`.

## Reproduction

Fixture: `claude-notes/issue-reports/1268/repro-site/` (one paragraph:
`This is a Quarto website.`).

```bash
q2 preview --allow-edit --ui editor claude-notes/issue-reports/1268/repro-site
```

The bug needs **two separate commits**:

1. Open the paragraph in the preview, select "Quarto", press Cmd-B, then
   commit (Cmd-Enter, or click away). The file on disk now reads
   `This is a **Quarto** website.`
2. Reopen the paragraph, select "a Quarto website", press Cmd-I, and commit.
   The file on disk now reads `This is *a* ***Quarto*** *website*.`

If you apply both marks in **one** editor session, the result is valid:
`This is *a* **_Quarto_** *website*.` In that case the source has no
`**Quarto**` for the reconciler to keep, so the writer emits the whole
paragraph fresh.

In both cases the editor DOM is identical:
`This is <em>a </em><strong><em>Quarto</em></strong><em> website</em>.`
The TS serializer turns it into `This is _a_ **_Quarto_** _website_.` (checked
with a scratch vitest against `docToMarkdown`). That text is valid.

Native reproduction, now pinned as two `#[ignore]`d tests in
`crates/pampa/tests/integration/incremental_writer_tests.rs`:

| test | original | edited (parsed as new AST) | incremental result |
|---|---|---|---|
| `new_emph_inside_original_strong_does_not_triple_delimit` | `This is a **Quarto** website.` | `This is _a_ **_Quarto_** _website_.` | `This is *a* ***Quarto*** *website*.` |
| `new_strong_inside_original_emph_does_not_triple_delimit` | `This is *Quarto*.` | `This is *__Quarto__*.` | `This is ***Quarto***.` |

The first row matches the browser output byte for byte.

## Localization

The text-channel commit path is:

1. `RichTextEditor.tsx` `commit` → `docToMarkdown` (TS serializer) →
   `commitTextEdit` (`ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx:1538`).
2. `apply_node_edit` (`crates/pampa/src/apply_node_edit.rs:113`) re-parses the
   text, splices it into the untransformed AST, and calls
   `compute_reconciliation` and then `incremental_write`.
3. The reconciler aligns the original `Strong[Str "Quarto"]` with the new
   `Strong[Emph[Str "Quarto"]]` as `RecurseIntoContainer`.
4. `assemble_recursed_container` (`crates/pampa/src/writers/incremental.rs:883`)
   copies the opening and closing delimiters from the source (`**`, `**`) and
   then calls `assemble_inline_content` on the children.
5. A changed child (`UseAfter`) is written by `write_inline_to_string`
   (`incremental.rs:1065`) → `qmd::write_single_inline`, using a **fresh**
   `QmdWriterContext`. Its `emphasis_stack` is empty, so `choose_emph_delimiter`
   (`crates/pampa/src/writers/qmd.rs:158`) returns `*`.

The full writer already avoids this case. `write_emph` and `write_strong`
(`qmd.rs:2227`, `qmd.rs:2248`) push their delimiter onto `emphasis_stack`, and
the next nested level switches to `_` after a `*` parent. The incremental path
discards that context at the splice boundary.

The fix needs the delimiters of the kept containers that enclose a spliced
child. For example, `assemble_recursed_container` could pass the stack of
enclosing delimiters (derived from the `opening` bytes it already has) down to
a context-aware variant of `write_inline_to_string`. A `UseAfter` child would
then see the same stack the full writer would see.

A related edge case belongs in the same fix: a kept `*x*` sibling directly
next to a fresh `*y*` sibling. Check that delimiter runs at sibling boundaries
cannot fuse.

## Open questions — resolved during triage

- **Does the `***` come from the TS serializer?** No. Its italic mark is
  explicitly `_` (`serializer.ts`, `marks.italic`) to avoid `***`. A scratch
  test serialized the editor's mark structure to the valid
  `_a_ **_Quarto_** _website_`. The `*` in the final file comes from the Rust
  qmd writer's default delimiter.
- **Why did the first browser attempt (both marks in one session) not
  reproduce?** In that session the original source had no `Strong`, so there
  was nothing to recurse into. The new paragraph was written entirely by the
  full writer, which tracks the emphasis stack. The issue needs a kept
  emphasis container whose children change.
- **Is the "shallow" output (`_a_ **_Quarto_** _website_` instead of
  `_a **Quarto** website_`) the same bug?** No. prosemirror-markdown opens
  marks in schema-rank order (bold before italic), so italic has to close and
  reopen around the bold word. The output is valid and equivalent when
  rendered, but it splits one `Emph` into three. This is filed separately as
  bd-3l05bz1g (P3). Fixing it alone would hide this repro, but bd-szy5r3wk
  could still be hit, for example by the second test row or by edits in the
  plain-text editor.
- **Is the installed `q2` stale?** The browser repro used `~/.local/bin/q2`
  (built 2026-11-13). The native tests run against `main` @ `4ca24c110` and
  give the same output, so the bug is present at HEAD.

## Outcome / recommended next step

Filed **bd-szy5r3wk** (bug, P1). Fix in the incremental writer. To start, run
the ignored tests:

```bash
cargo nextest run -p pampa --run-ignored only -E 'test(triple_delimit)'
```

Then remove the `#[ignore]` attributes once they pass. Also filed
**bd-3l05bz1g** (feature, P3, related) for nesting-aware mark serialization in
the rich-text editor.

## Verification commands used

```bash
gh issue view 1268 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
q2 preview --allow-edit --ui editor --no-browser --port 47268 <site>   # Chrome via DevTools MCP
cargo nextest run -p pampa --run-ignored only -E 'test(triple_delimit)'
```

## Cross-references

- bd-szy5r3wk, bd-3l05bz1g
- `crates/pampa/src/writers/incremental.rs` (`assemble_recursed_container`, `write_inline_to_string`)
- `crates/pampa/src/writers/qmd.rs` (`choose_emph_delimiter`, `choose_strong_delimiter`)
- `ts-packages/preview-renderer/src/q2-preview/richtext/serializer.ts`
