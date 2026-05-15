# CommonMark Spec — Reference Scaffolding

The CommonMark specification (v0.31.2) is checked in at
`external-sources/commonmark/spec.txt` — 9811 lines, ~200 KB. Reading
it in full is wasteful for almost any concrete question. This directory
provides indexes and small lookup scripts so an agent (or human) can
fetch just the relevant section or example.

**Caveat on conformance.** This project does *not* promise CommonMark
compliance. Quarto Markdown (qmd) is its own dialect. Use this spec as
a reference for "expected behavior unless we have a documented reason
to differ" — not as a contract. When qmd intentionally diverges (e.g.
no reference-style links), that divergence is the source of truth, not
the spec.

## Contents

- `index.md` — section table of contents with line ranges. Skim this
  first to find the right area.
- `examples-index.md` — every numbered example (655 total) with its
  section and line range. Search this when chasing a specific behavior.
- `scripts/spec-section.sh` — print one section's text.
- `scripts/spec-example.sh` — print one numbered example.

## Typical commands

From the repo root (or anywhere — the scripts resolve their own paths):

```bash
# Look up by section title (case-insensitive substring)
claude-notes/research/commonmark-spec/scripts/spec-section.sh "fenced code blocks"

# Look up by line number (prints the enclosing section)
claude-notes/research/commonmark-spec/scripts/spec-section.sh 1934

# Print a specific numbered example (input + expected HTML, with fences)
claude-notes/research/commonmark-spec/scripts/spec-example.sh 95
```

For ad-hoc reads, the `Read` tool with an explicit `offset`/`limit`
from `index.md` is also fine — but prefer the scripts for whole-section
lookups since they handle the next-heading boundary correctly.

## Workflow for diagnosing a parser-behavior question

1. Identify the construct (e.g. "list item lazy continuation").
2. Open `index.md`, find the section (e.g. §5.2 List items, lines
   4119–5237).
3. Run `spec-section.sh "list items"` to read just that section.
4. Grep `examples-index.md` for related examples and pull them with
   `spec-example.sh <N>`.
5. Cross-check qmd behavior against the examples; document any
   intentional divergence in the relevant `claude-notes/` plan or
   code comment.

## Maintenance

- Do not edit `external-sources/commonmark/spec.txt` (it is an
  external reference).
- If the spec is bumped to a new version, regenerate `index.md` and
  `examples-index.md`. The awk one-liners that produced them are in
  git history for this directory.
- The scripts assume the heading style and 32-backtick fence
  convention. Both have been stable across CommonMark versions; if a
  future spec breaks them, fix the script rather than working around
  it.
