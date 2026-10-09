---
title: 'doctemplate CRLF line-ending preservation (bd-1d3e / #157)'
date: 2026-06-23
---

## Policy decision

**Preserve the input line-ending convention end-to-end.** CRLF input renders
CRLF output; LF input renders LF output. No ingress normalization, no silent
byte rewriting. (Chris, 2026-06-23. Matches Pandoc's parser-aware doctemplates;
`--eol` writer override is a separate, future concern.)

This sets precedent for pampa output, the JSON/native writers, and future
tree-sitter grammars: line-ending conventions are preserved, not normalized.

## Root cause

`normalize_multiline_directives` (`crates/quarto-doctemplate/src/parser.rs`)
detects a "directive on its own line" and swallows the surrounding newlines, to
match Pandoc. Its three helper functions hardcode `'\n'`:

- `first_node_is_newline_literal` / `is_first_child_newline_literal` — detection
  via `lit.text.starts_with('\n')` (parser.rs:1093-1098).
- `strip_leading_newline_from_node` — `starts_with('\n')` then `text[1..]`
  strips a single byte (parser.rs:1115-1120).

For CRLF input the body Literal starts with `\r\n`, so detection returns false,
`is_multiline` stays false, and the newline-consumption branch never runs →
doubled blank lines around every multiline `$if$` / `$for$` / `$else$`.

The bug is in the engine and reproduces with in-memory `Template::compile` —
**not** a file-read concern. (An earlier ingress-normalize attempt was reverted:
it was the approach #157 explicitly rules out, and it masked this engine bug.)

## Fix

Make the three helpers treat `\r\n`, `\r`, and `\n` each as one leading
line-ending unit:

- detection: body's first Literal starts with any of `\n` / `\r\n` / `\r`.
- strip: remove the full leading sequence (2 bytes for `\r\n`, else 1).

Single chokepoint — all detection/stripping routes through these helpers — so
both `$if$` and `$for$` (and nested/else) are fixed at once. CRLF bytes in the
surrounding literals are preserved, so output keeps the input convention.

## tree-sitter-doctemplate audit

The buggy output already contained `\r\n` inside the body Literal, so the
grammar tokenizes CRLF into literal text fine; the Rust pass alone should
suffice. Confirm no grammar change is needed (no `\r`-specific tokenization gap).

## Test fixtures

`tests/pandoc-equiv/*.template` and `test-fixtures/*.template` are checked out
CRLF on Windows (`core.autocrlf=true`). Their tests assert LF output. Pin these
fixtures to LF via a scoped `.gitattributes` (`*.template eol=lf`) + renormalize,
so they are LF on every platform and the existing LF assertions verify
LF-preservation. The CRLF path is covered by new in-process tests.

## Work items

- [x] TDD: in-process CRLF regression tests — `Template::compile(crlf_source)`
      for `$if$`, `$for$`, nested, `$else$`; assert CRLF-preserved output (no
      doubled lines). Confirmed they FAILED on current engine (doubled `\r\n`).
      Added `crlf_preservation` module in `src/parser.rs` (+ `lf_behavior_unchanged`).
- [x] Fix the three helpers to be line-ending-agnostic via new
      `leading_line_ending_len` (`\r\n`→2, `\r`/`\n`→1).
- [x] New CRLF tests pass; `lf_behavior_unchanged` confirms LF path unchanged.
- [x] Pin `.template` fixtures to LF via `crates/quarto-doctemplate/.gitattributes`
      + renormalize (delete + checkout to apply `eol=lf`).
- [x] The 8 originally-failing fixture tests pass; full crate suite 200/200.
- [x] Audited `tree-sitter-doctemplate`: `text: /[^$]+/` captures CRLF verbatim,
      so Rust-only fix is enough. No grammar change. (Only `\n`-specific rule is
      `comment`, which chomps CRLF comment lines correctly; lone-CR comment
      terminator is a no-consumer edge.)
- [ ] Full workspace verification (Chris runs): `cargo nextest run --workspace`
      + `cargo xtask verify --skip-hub-build`.
