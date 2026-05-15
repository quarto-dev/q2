# Issue #173 — qmd writer drops trailing blank line inside a code block

- **GitHub**: https://github.com/quarto-dev/q2/issues/173
- **Reporter**: @rundel (Colin Rundel), 2026-05-11
- **Triage date**: 2026-05-11
- **Worktree**: `.worktrees/issue-173` (branch `issue-173`, based on `main` @ `37f78170`)
- **Beads issue**: bd-v1qc
- **Scope**: the fenced-code-block round-trip bug only. The three quarto-web links in the issue body are example sites of the same bug; they will be fixed by the same change and don't need separate triage.

## Summary

Real bug, fix is small and lives in the writer. A `CodeBlock` whose AST `.text` ends with one or more `\n` round-trips with the final `\n` removed, because `write_codeblock` in `crates/pampa/src/writers/qmd.rs` only emits a separator newline before the closing fence when the content does not already end in `\n`. The reader's AST shape matches Pandoc exactly (verified) so the AST contract is not the problem — only the writer is wrong. Reproduced verbatim against the reporter's example and on three additional edge cases (empty content, content of only blank lines, content with two trailing blank lines).

## Reproduction

Input fixture: `claude-notes/issue-reports/173/repro.qmd` (bytes: `` ```markdown\nfoo\n\n```\n ``).
Driver: `claude-notes/issue-reports/173/repro.sh` (runs all three stages and dumps bytes).

Observed:

```
$ ./claude-notes/issue-reports/173/repro.sh
=== first parse (text should be "foo\n") ===
[ CodeBlock ( "" , ["markdown"] , [] ) "foo\n" ]

=== qmd writer output (closing fence glued to 'foo\n', BUG) ===
0000000    `   `   `   m   a   r   k   d   o   w   n  \n   f   o   o  \n
0000020    `   `   `  \n

=== round-trip parse (text is now "foo", BUG — trailing \n lost) ===
[ CodeBlock ( "" , ["markdown"] , [] ) "foo" ]
```

Expected: the writer emits `` ```markdown\nfoo\n\n```\n ``, so the re-parse yields `"foo\n"`.

## Reader vs writer — where to fix

The user explicitly asked which side this should be fixed on. Decision: **writer-only.** The reasoning:

### Pandoc parity check (the deciding evidence)

The pampa reader currently joins content lines with `\n` and emits **no trailing `\n`** for the last content line. That is *not* what a literal reading of the CommonMark spec produces (the spec's HTML examples always show a trailing newline before `</code>`), but it is **exactly** what Pandoc 3.9 does. Six side-by-side cases — pandoc 3.9.0.2 vs pampa, native parse, identical inputs:

| input between fences | pandoc `CodeBlock` text | pampa `CodeBlock` text |
|----------------------|-------------------------|------------------------|
| `foo`                | `"foo"`                 | `"foo"` |
| `foo\n` (1 trailing blank) | `"foo\n"`         | `"foo\n"` |
| `foo\n\n` (2 trailing blanks) | `"foo\n\n"`    | `"foo\n\n"` |
| (empty)              | `""`                    | `""` |
| `\n` (one blank)     | `""`                    | `""` |
| `\n\n` (two blanks)  | `"\n"`                  | `"\n"` |
| `\n  ` (CommonMark spec ex. 100 input) | `"\n  "` | `"\n  "` |

So:

1. Pampa's reader matches Pandoc 1:1 on every case I checked.
2. Pandoc itself diverges from a literal reading of the CommonMark spec in the same way (and has done so for many years). For example, CommonMark spec example 100 (`external-sources/commonmark/spec.txt:2107`) expects the rendered HTML to be `<pre><code>\n  \n</code></pre>` (two newlines inside `<code>`), but `pandoc -f markdown -t html5` on the same input emits `<pre><code>\n  </code></pre>` (one newline). Pampa is following Pandoc here, not the literal spec.
3. This project does not promise CommonMark compliance, and matching Pandoc is the dominant convention in this codebase.

Changing the reader to be strictly spec-compliant would (a) diverge from Pandoc, (b) change the AST shape for every code block in the corpus, (c) invalidate a large fraction of `crates/pampa/snapshots/`, and (d) gain no functionality. So the reader stays as is.

### The writer's contract under that reader

Given the Pandoc-matching reader, the writer's job is the inverse: it must emit text such that re-parsing yields the same `.text` it was given. The rule that falls out of the table above is:

- Non-empty `.text` C → write `C` followed by exactly one `\n`, then the closing fence on its own line.
- Empty `.text` → write nothing between the fences (or one blank line — both round-trip to `""`).

The current writer instead writes `C` then conditionally adds `\n` only if `C` didn't already end with one. That's correct for `"foo"` but wrong for any C ending in `\n`, which is exactly the bug.

## Localization

`crates/pampa/src/writers/qmd.rs:628-634`:

```rust
// Write the code content
write!(buf, "{}", codeblock.text)?;

// Ensure we end on a newline
if !codeblock.text.ends_with('\n') {
    writeln!(buf)?;
}
```

The conditional is the bug. The minimal fix is to remove the `if` and always emit a newline after non-empty content. A clean form:

```rust
write!(buf, "{}", codeblock.text)?;
if !codeblock.text.is_empty() {
    writeln!(buf)?;
}
```

This makes `write_codeblock` round-trip correctly on all seven cases in the table above, including the previously-broken cases where `.text` ends with `\n`.

Note: this fix also silently corrects a secondary round-trip defect that the issue did not call out. The current writer renders empty content (`.text = ""`) as `` ```\n\n```\n `` (one blank content line), which happens to re-parse to `""` because the reader collapses one blank content line to `""`. With the proposed fix, empty content renders as `` ```\n```\n `` (zero content lines) — also re-parses to `""`, and is the canonical form. Either is correct under round-trip, but the proposed form is the canonical one and matches what a human writes.

## Open questions — resolved during triage

**Q1: Should we fix the reader to be CommonMark-spec-compliant (always include trailing `\n`) instead?**
Resolved: no. See § "Reader vs writer". Pampa's reader matches Pandoc exactly, and Pandoc itself diverges from a literal reading of the CommonMark spec. Changing the reader would break parity with Pandoc, invalidate snapshots, and offer no user-visible benefit.

**Q2: What does Pandoc actually emit as the writer output? Could pampa also "fix" round-trip by stripping the trailing `\n` in the reader?**
Resolved: not investigated for the *writer* side because the choice is already constrained — pampa's reader output is what user filters/tests inspect, and that already matches Pandoc. Stripping `\n` in the reader would change the AST contract; doing it only on round-trip is worse. The clean fix is writer-only.

**Q3: How many existing round-trip / snapshot tests could break with this change?**
Resolved (sample): only three fixtures under `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/` mention fenced code (`codeblock_with_attrs.qmd`, `rawblock_latex.qmd`, plus the note-definition family). None has trailing blank lines inside a code block, so the new writer behavior would produce identical bytes on existing inputs. The empty-code-block case (see Localization note) might affect a handful of snapshots that include `` ```\n\n```\n ``; the fix can keep the current empty-block output (`` ```\n\n```\n ``) if those snapshots are too numerous to update, since both forms are correct.

## Outcome / recommended next step

Filed bd-v1qc with the fix scope below.

1. Write a TDD round-trip test under `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/` covering: content with one trailing `\n`, content with two trailing `\n`, content of only blank lines, and an empty code block. Verify it fails on the trailing-`\n` cases.
2. Apply the writer change at `crates/pampa/src/writers/qmd.rs:628-634` as shown.
3. Re-run `cargo nextest run -p pampa` and update any snapshots that change. Document any non-fence-trailing snapshot changes in the commit message per `CLAUDE.md` § Snapshot Test Changes.
4. End-to-end verify per the project rule: run `cargo run --bin pampa -- <fixture>.qmd -t qmd | cargo run --bin pampa --` on the four cases above and confirm `.text` round-trips.

Also: durable CommonMark spec lookup scaffolding produced during this triage was landed separately on `main` under `claude-notes/research/commonmark-spec/` — it's small, self-contained, and was useful for resolving Q1.

## Verification commands used

```bash
gh issue view 173 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments

# Reader behavior on five inputs (showed pampa matches pandoc)
for input in '```markdown\nfoo\n\n```\n' '```markdown\nfoo\n```\n' \
             '```markdown\nfoo\n\n\n```\n' '```\n```\n' '```\n\n\n```\n'; do
  printf -- "$input" | cargo run --quiet --bin pampa --
  printf -- "$input" | pandoc -f markdown -t json | jq -c '.blocks'
done

# CommonMark spec example 100 (showed pandoc diverges from spec, pampa follows pandoc)
printf -- '```\n\n  \n```\n' | pandoc -f markdown -t html5
printf -- '```\n\n  \n```\n' | cargo run --quiet --bin pampa --

# Round-trip + writer output bytes
./claude-notes/issue-reports/173/repro.sh

# Existing fenced-code coverage in the round-trip suite
ls crates/pampa/tests/roundtrip_tests/qmd-json-qmd/ | grep -i -E 'code|fence'
```

## Cross-references

- `crates/pampa/src/writers/qmd.rs:628-634` — the buggy conditional.
- `crates/pampa/CLAUDE.md` — TDD round-trip workflow that the fix should follow.
- `claude-notes/research/commonmark-spec/` (on `main`) — CommonMark spec lookup scaffolding produced during this triage (index, examples-index, and two helper scripts).
- `external-sources/commonmark/spec.txt:1934-2359` — Fenced code blocks section consulted for the AST-shape question.
