---
title: "Q-2-61: unquoted attribute value containing a comma"
date: 2026-11-25
description: "Give `widths=1,3` a coded diagnostic and stop the follow-on errors at later fences (GH 1339)."
---

Strand: bd-e8vm6sf2. Issue: <https://github.com/quarto-dev/q2/issues/1339>.

## Overview

Current behaviour: an unquoted attribute value containing a comma, such
as `::: {.list-table widths=1,3}`, gives a bare uncoded "Parse error".
Recovery consumes the div, so the closing `:::` and every later fence
report more parse errors.

All attribute hosts (div, span, image, inline code, heading) fail in one
LR state, `(2950, pandoc_str_token1)`. Every other character the
attribute lexer rejects in a value (`;`, `(`, `<`, ...) fails in the
same state.

New behaviour: a new error-corpus entry,
`crates/pampa/resources/error-corpus/Q-2-61.json`, codes the error as
Q-2-61 with the message "Attribute values containing `,` must be quoted".
It is marked `desynchronizes`, so the follow-on fence errors are
dropped. Its cases are guarded on a comma in the value (`^[^\s}]*,`),
so the other rejected characters keep the generic error and don't get a
message about commas.

## Work items

- [x] `Q-2-61.json` with div, span and heading cases (guarded, desynchronizes)
- [x] Regenerate `_autogen-table.json` and case files with `build_error_table.ts`
- [x] Register Q-2-61 in `error_catalog.json`, add `docs/errors/markdown/Q-2-61.qmd` and the sidebar entry
- [x] Integration test `tests/integration/test_attr_unquoted_comma_value.rs`
