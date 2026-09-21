# Probes from the 2026-09-21 docx / quarto-math research session

Throwaway tools that produced the evidence cited in
`claude-notes/plans/2026-09-21-quarto-math-and-native-docx.md`. They are
kept for reproducibility, not as project code; nothing builds them in CI.

- `extract_python_docx_oxml.py` — statically extracts python-docx's
  declarative element model (`_tag_seq`, child/attribute declarations, XML
  template methods) from `external-sources/python-docx/src/docx/oxml` into
  JSON, using only the stdlib `ast` module (no lxml needed).
  `python3 extract_python_docx_oxml.py external-sources/python-docx out.json`
- `mitex-probe/` — a scratch binary against `external-sources/mitex` that
  prints the rowan parse tree for each argument and, with a second lexer
  pass, recovers the original byte span of every leaf token (including
  macro-expanded ones). Requires `git submodule update --init` inside
  `external-sources/mitex` (prebuilt spec artifact) and is built outside the
  q2 workspace (`[workspace]` stanza in its Cargo.toml).
- `spec-dump/` — dumps mitex's prebuilt command spec (`default.rkyv`) to
  the sorted JSON committed at
  `crates/quarto-math/spec/upstream/mitex-default-spec.json`. Same
  submodule requirement and out-of-workspace build as `mitex-probe/`:
  `cd spec-dump && cargo run --release > ../../../../crates/quarto-math/spec/upstream/mitex-default-spec.json`.
