# Inline-scope state collision: emphasis errors inside quotes misclassified as quote errors

## Summary

When an unescaped emphasis marker (`_`, `*`, `__`, `**`) appears inside a paired single-quote or double-quote span, the parser emits the wrong error code. It reports a quote-related diagnostic (`Q-2-10` Closed Quote Without Matching Open Quote, or `Q-2-11` Unclosed Double Quote) when it should report the emphasis-related diagnostic that actually explains the problem (`Q-2-5`, `Q-2-12`, `Q-2-13`, or `Q-2-15`).

The root cause is that several distinct parser contexts collapse to the same `(LR state, lookahead symbol)` key in the Merr-style error table, so the lookup cannot distinguish them.

## Examples

All inputs below trigger the wrong error code today.

### Single-quoted variants (should emit Q-2-5/12/13/15, currently emit Q-2-10)

```text
The '_blank' word.       # should be Q-2-5  (Unclosed Underscore Emphasis)
The '*blank' word.       # should be Q-2-12 (Unclosed Star Emphasis)
The '**blank' word.      # should be Q-2-13 (Unclosed Strong Star Emphasis)
The '__blank' word.      # should be Q-2-15 (Unclosed Strong Underscore Emphasis)
```

For each of these the user wrote a literal underscore or star inside a properly closed `'...'` span. The error message says "Closed Quote Without Matching Open Quote" pointing at the closing `'`, which is misleading; the real problem is the unescaped emphasis marker.

### Symmetric unclosed-double-quote-in-emphasis (should emit Q-2-11)

```text
*a" b.*                  # should be Q-2-11 (Unclosed Double Quote)
**a" b.**                # should be Q-2-11
_a" b._                  # should be Q-2-11
__a" b.__                # should be Q-2-11
```

Same shape, but with the apostrophe and emphasis markers swapped. Currently these emit `Q-2-10` (the apostrophe interpretation that earlier corpus changes pushed onto the colliding state).

### Multi-paragraph regression observed during fix attempts

```text
First apostrophe: a' b.

Second in bold: **c' d.**
```

The second-paragraph apostrophe was being correctly classified before any of the fix attempts. Some of the proposed fixes introduced a regression where the second paragraph's diagnostic degrades to a generic "Parse error" instead of `Q-2-10`. See attempt 4 below.

## What is the underlying problem

The error-reporting system uses Clinton Jeffery's Merr technique (TOPLAS 2003): each example error in the corpus is parsed to obtain a `(state, lookahead)` key, and at runtime the same key looks up the diagnostic. The technique relies on the parser reaching the same internal state for "the same kind of mistake."

The tree-sitter grammar used here aggressively minimises LR states. Item-set equivalence and symbol aliasing mean that, for example, all of the following inputs converge on the same `(state, lookahead)` pair at the moment of error:

```text
The '_blank' word.   # parser is inside single-quote, looking at unclosed `_`
_a' b._              # parser is inside `_`-emphasis, looking at unmatched `'`
```

Both errors happen at LR state `704` with lookahead `_whitespace`. The corpus can only carry one diagnostic per key, so whichever example was added last wins for both inputs. Identical collisions hold for the 690/712/759/705 quartet (the variations across `*`, `**`, `__`, and `"` flavours).

## Attempts made so far

### Attempt 1: corpus-only entries for the double-quoted variants

Add `Q-2-5/in-double-quote`, `Q-2-12/in-double-quote`, etc. cases to the corpus. The double-quoted shapes (`The "_blank" word.`) work because their state numbers do not collide with the analogous `Q-2-10` apostrophe-in-emphasis cases used by `qmd-syntax-helper`. The single-quoted shapes cannot be fixed this way: any new entry for `The '_blank' word.` overwrites the matching `Q-2-10` entry that `qmd-syntax-helper`'s `apostrophe-quotes` rule depends on. Shipped as the partial fix in commit `6e3ad158` for the double-quote shapes only.

### Attempt 2: grammar refactor with per-scope productions

Define separate `_inlines_in_single_quote`, `_inlines_in_double_quote`, `_inlines_in_emph_star`, etc. (16 scopes times 3 cascade levels = 48 productions) so the LR parser would carry the scope distinction in its state. Did not work: tree-sitter's LR generator minimises states based on item-set equivalence, regardless of production names, so the 48 productions collapsed back to the same shared states.

### Attempt 3: external scanner with scope-tagged whitespace tokens

Add an `InlineScope` enum plus a scope stack inside `scanner.c`, and emit scope-tagged tokens such as `_whitespace_in_single_quote`, `_whitespace_in_emph_star`, etc. The intent was to make the lookahead symbol carry the scope. Did not work: tree-sitter's `ts_symbol_map` and alias machinery collapse externals that share an alias, and removing the aliases still did not get the tagged tokens into `valid_symbols` at the relevant states. Multiple parser-table layers fought back.

### Attempt 4: extend the Merr key with an `outer_scope` column

Leave the grammar and scanner alone. Add a third dimension to the error-table key: `(state, sym, outer_scope)` where `outer_scope` is derived at lookup time by walking `consumed_tokens` (the LR stack) and identifying the outermost open inline scope (`single_quote`, `double_quote`, `emph_star`, ...). Same walker runs at corpus-build time so each existing entry picks up its natural `outer_scope` automatically. New `in-single-quote` cases can be added to `Q-2-5/12/13/15` without colliding with `Q-2-10`'s `emph_*` cases.

Works for single-paragraph inputs (all 17 emphasis-in-quote regression tests pass with this approach). Breaks for multi-paragraph inputs: the walker uses `parse.consumed_tokens` which is the LR stack at the end of parsing, not at the moment of each individual error. For input like the third example above, the row-0 paragraph leaves an unreduced `single_quote` opener on the stack (because row 0 itself errored), and the row-2 paragraph's `strong_emphasis_delimiter` gets reduced into a wrapper non-terminal (`document`). The walker then sees the leftover row-0 `single_quote` first and returns the wrong scope for the row-2 error.

A per-error stack snapshot taken at `detect_error` time would resolve this, but it requires changes to `TreeSitterLogObserver`'s public shape (snapshotting `Vec<ConsumedToken>` per error state, threading the snapshot through both `produce_diagnostic_messages` and `produce_error_message_json`). The fix is tractable, but the broader question is whether the Merr approach is the right tool for a problem that fundamentally needs scope-aware parser state.

## Open questions

- Is the right long-term fix to make the parser carry scope state (giving up some tree-sitter state minimisation), or to push more disambiguation into the error-reporting layer?
- If the latter, is `outer_scope` enough, or do we need full per-error stack snapshots to handle multi-block documents correctly?
- Are there other inline scope kinds (`span`, `image`, `inline_note`, super/subscript, strikeout, editorial marks) that will surface the same class of collision once these four are fixed?
