---
title: 'Config paths: read markdown-parsed strings from their source span (GH #1152)'
date: 2026-11-10
description: 'Extension theme paths with two underscores lost them on pages that run code, because the path was recovered by flattening markdown inlines; read the source text instead.'
status: done  # Merged in PR #1164 (d84b9ad12), 2026-11-10; its span-reading code was reverted by #1180
braid:
  strand: bd-d50anjlg
---

Strand: `bd-d50anjlg`. Issue: <https://github.com/quarto-dev/q2/issues/1152>.

## Overview

**Current behaviour.** Before an engine runs, `EngineExecutionStage`
serializes the merged metadata to qmd front matter and re-parses the
engine's output. The qmd writer emits a `Path`-kind value (an extension's
rebased `theme:` entry, `_extensions/acme/demo/_colors.scss`) as a plain
YAML string, so the re-parse reads it as markdown: the text between the
two underscores becomes `Emph`. `ConfigValue::as_plain_text` flattens the
inlines and drops the delimiters, so `CompileThemeCssStage` looks for
`extensions/acme/demo/colors.scss` and fails with Q-14-4. Pages without
code cells never re-parse and keep the `Path` value.

**New behaviour.** A string that was parsed as markdown is read back from
the text its span covers (decoded as a YAML scalar, so quotes and escapes
come out right), not from its inlines. The inlines are the fallback only
when the span is not a single range in a file whose content is in the
`SourceContext`.

- `crates/quarto-core/src/config_paths.rs`: new `config_value_text`;
  `resolve_config_path` and `resolve_config_file_with` use it; new
  `literal_config_strings` turns a subtree's markdown values into plain
  strings.
- `crates/quarto-core/src/stage/stages/engine_execution.rs`: the
  engine intermediate's source slot keeps its markdown (it kept only the
  line index before), so the re-parsed spans have text to read.
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs`: hand
  `ThemeConfig::from_config_value` a metadata copy whose `theme:` values
  are literal, read through `doc.ast_context.source_context` (the
  context that holds the engine intermediates).

`PathCtx` consumers are built from `doc.source_context`, which does not
register engine intermediates, so on engine pages they still fall back
to the flattened inlines. Follow-up: `bd-8svy3p89`.

## Work items

- [x] Failing test: a `Path` value round-tripped through the qmd writer
      and reader resolves to the right file.
- [x] `config_value_text` + use in the resolvers.
- [x] Literal `theme:` values in `CompileThemeCssStage`.
- [x] Repro from the issue renders all pages.
- [x] Verification (nextest, clippy, xtask lint, notes render). The
      Windows cross clippy cannot build tree-sitter's C from macOS.
