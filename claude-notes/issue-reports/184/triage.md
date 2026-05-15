# Triage: issue #184 — Indented (4-space) code blocks parsed as paragraphs

- **GH:** https://github.com/quarto-dev/q2/issues/184
- **Reporter:** @rundel (2026-05-11)
- **Branch:** `issue-184`
- **Verdict:** Real bug. Project policy (per @cscheid's comment on the issue) is that Quarto Markdown does **not** support 4-space indented code blocks; the parser must reject them with a high-quality error message rather than silently rewriting them.
- **Plan:** `claude-notes/plans/2026-05-14-q-2-35-indented-code-block-error.md`
- **Beads:** [bd-7l1u](.beads) — _Q-2-35: Reject 4-space indented code blocks with a custom parse error (issue #184)_

## What the user reported

CommonMark indented code blocks (lines starting with four spaces) are not recognized by pampa's parser. The indented content is parsed as a series of `Para` blocks (with the indentation **stripped**), so the file silently changes shape on a qmd → qmd round-trip.

Reporter's example, reproduced verbatim by `claude-notes/issue-reports/184/repro.qmd`:

```
Before.

    categories:
      - A
      - B

After.
```

## Reproduction at HEAD (`main` of issue-184 worktree, 2026-05-14)

```
$ cargo run --bin pampa -- claude-notes/issue-reports/184/repro.qmd
[ Para [Str "Before."]
, Para [Str "categories:"]
, Para [Str "-", Space, Str "A"]
, Para [Str "-", Space, Str "B"]
, Para [Str "After."]
]
```

Round-trip through the qmd writer:

```
$ cargo run --bin pampa -- claude-notes/issue-reports/184/repro.qmd -t qmd
Before.

categories:

- A

- B

After.
```

The indentation is gone; a second pass would now read this as a real `BulletList`. Confirms the issue exactly.

## Diagnosis

The block-level scanner already tracks per-line leading whitespace as `s->indentation` (and `s->column` for tab expansion). After the per-block matchers consume their share (e.g. `list_item_indentation(block)` for a list-item context, the `>` plus optional space for a blockquote), a non-zero `s->indentation` is what's "leftover" — the indentation that the user wrote *beyond* what the surrounding container required.

Every block-start emitter in `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` gates on `s->indentation <= 3` (e.g. `THEMATIC_BREAK` at line 742, ATX heading at line 868, the various list-marker emitters at 888 / 933 / 1007 / 1081, fenced code at line 639). When `s->indentation >= 4` and *none* of those emitters match, control falls through and the line is consumed as paragraph content. That fall-through is the trap.

There is **no existing diagnostic** for this case. The scanner silently consumes the whitespace, and the leading spaces never reach the AST or the writer — which is why round-tripping rewrites the file.

## Resolution direction (decided with the user)

Adopt the **same scheme** the parser already uses for Q-2-32 (`***` triple-star emphasis):

1. Detect the disallowed construct in `scanner.c`.
2. Emit an external token that is declared in `grammar.js`'s `externals` list **but never consumed by any rule body**.
3. The grammar then has nowhere to shift the token, so tree-sitter raises a parse error at exactly that point.
4. The Merr-style error table in `crates/quarto-parse-errors/` maps the resulting `(state, sym)` pair to a templated user-facing message rendered through `quarto-error-reporting`.

Confirmed wiring of the existing Q-2-32 example:

| Stage | File / line |
| --- | --- |
| Token enum                      | `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:132` (`TRIPLE_STAR`) |
| Detection logic                 | `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:733-735` |
| Externals declaration (unused)  | `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:1052` (`$._triple_star_error`) |
| Error corpus                    | `crates/pampa/resources/error-corpus/Q-2-32.json` |
| Error table generation script   | `crates/pampa/scripts/build_error_table.ts` |
| Generated lookup table          | `crates/pampa/resources/error-corpus/_autogen-table.json` |
| `(state, sym)` → message        | `crates/quarto-parse-errors/src/error_table.rs:65-94` (`lookup_error_entry`) |
| Diagnostic rendering            | `crates/quarto-parse-errors/src/error_generation.rs:30-200` |
| QMD reader integration          | `crates/pampa/src/readers/qmd_error_messages.rs:24-36` (called from `qmd.rs:124`) |
| Snapshot tests                  | `crates/pampa/tests/test_error_corpus.rs` (`crates/pampa/snapshots/error-corpus/text/`) |

The infrastructure is in place; this is a small additive change.

## Scope decisions (confirmed with @cscheid in this triage)

1. **List-item context:** Error fires whenever the **leftover** `s->indentation` (after all container matchers have consumed their share) is `>= 4`. So a continuation paragraph properly indented to a list item's level is unaffected, but a list-item continuation that itself has 4 *additional* leading spaces is also rejected.
2. **Error code & wording:** `Q-2-35` (next free in the Q-2 sequence). Title: *"Indented code blocks are not supported"*. Message: *"Quarto Markdown does not support 4-space indented code blocks. Use a fenced code block (` ``` `) instead, or remove the leading indentation."*
3. **Documentation:** Add a Known Limitations entry in `crates/tree-sitter-qmd/tree-sitter-markdown/CONTRIBUTING.md` (and a comment in `grammar.js` near the new external) following the Q-2-32 precedent. User docs in `docs/` updated only if there is an obvious place.

## Outcome

- One beads issue filed (the implementation work).
- This triage doc + `repro.qmd` committed on branch `issue-184`.
- Plan document (TDD-shaped) committed alongside, referenced from the beads issue.

No incidental sub-issues discovered during triage.
