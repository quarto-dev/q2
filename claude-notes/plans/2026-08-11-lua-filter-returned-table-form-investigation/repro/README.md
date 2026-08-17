# Lua filters written in the returned-table form are silently ignored

**Observed with:** q2 0.16.0; re-verified unchanged on 0.17.0.
**Repro:** `q2 render` in this directory.

> **Provenance.** Copied verbatim into q2 on 2026-08-11 from the porting
> session that found it (`q2-connect-docs/llms-info/repros/lua-filter-table-form-ignored/`,
> a local-only repo), so the repro survives independently of that checkout.
> What was actually observed re-running it at `main` @ `808215fc` is in
> `OBSERVED-AT-HEAD.md` next to this file; pandoc's reference semantics for
> every return shape are pinned in `../pandoc-probes/README.md`.

## Expected (Pandoc / Quarto 1)

A Lua filter may be written either way, and Pandoc runs both:

```lua
-- returned-table form (the standard idiom)
return {
  Str = function(el) ... end,
}
```

```lua
-- top-level function form
function Str(el) ... end
```

The returned-table form is the more common of the two in real filters,
because it is the only way to express an *ordered list* of passes
(`return { {Str = ...}, {Para = ...} }`) and because it keeps handlers
out of the global namespace.

## Actual (q2 0.16.0)

Only the top-level function form runs. The returned-table form is
silently ignored: the filter file is read, no error or warning is
emitted, and none of its handlers ever fire.

| file | form | result |
|---|---|---|
| `index.qmd` + `table-form.lua` | `return { Str = … }` | `MARKER` unchanged — **filter did not run** |
| `list-form.qmd` + `list-form.lua` | `return { {Str=…}, {Str=…} }` | `MARKER` unchanged — **filter did not run** |
| `control.qmd` + `function-form.lua` | `function Str(el)` | `FUNCTION-FORM-RAN` — works |
| `walk-form.qmd` + `walk-form.lua` | global `Pandoc` + `doc:walk{Str=…}` | `WALK-TABLE-RAN` — works |

All four filters contain the identical handler body and are declared the
same way. Verified for `filters:` in project metadata (`_quarto.yml`),
in document frontmatter, and on a standalone single-file render — all
three behave the same.

q2's own documentation (`docs/guides/authoring/lua-filters.qmd`) shows
only the top-level function form, so this may be a deliberate subset
rather than a bug — but if so it needs a diagnostic, because the
failure mode today is a filter that quietly does nothing.

## Root cause

`crates/pampa/src/lua/filter.rs`. `apply_lua_filter` (~line 221) loads
the filter chunk with

```rust
lua.load(&filter_source).set_name(…).exec_async().await?;
```

`exec_async` **discards the chunk's return value**, so a `return { … }`
at the top of a filter goes nowhere. The handler table is then rebuilt
from scratch by `get_filter_table` (~line 323), which only ever reads
globals — it walks a hardcoded list of ~50 element names and copies any
same-named global function into a fresh table.

`get_filter_table`'s own doc comment states the intended contract:

```rust
// Pandoc filters can either:
// 1. Return a table with filter functions
// 2. Define filter functions as globals
// We'll support both by creating a table that checks globals
```

Case 1 was never implemented — only case 2 exists, and the comment
describes an intent the code does not fulfil.

Nothing downstream needs to change. `apply_full_filter` already takes a
handler table as an argument, and `walk-form.lua` in this repro proves
the table-consuming path works end to end: a handler table handed to
`doc:walk` fires correctly. The bug is confined to the entry point that
throws the table away.

## Suggested fix direction

Load the chunk with `eval_async::<Value>()` instead of `exec_async()`
and inspect the result:

- a table of handlers → use it directly (no name whitelist, matching
  Pandoc, so filters can define handlers q2's hardcoded list omits);
- a *sequence* of handler tables → apply them as successive passes, in
  order. `list-form.lua` covers this shape; it is the reason authors
  reach for the returned-table form in the first place, so a fix that
  handles only the single-table case is incomplete;
- `nil` / anything else → fall back to the existing globals scan.

`get_walking_order` (~line 88) already reads `traverse` off a filter
table, so per-table traversal mode comes along for free.

If instead the returned-table form is to stay unsupported, it needs a
diagnostic at load time: a chunk that returns a table and defines no
recognised globals is unambiguously this mistake, and today it is
silent.

## Impact on the Connect docs

The `mermaid-zoom` extension (`_extensions/posit-dev/mermaid-zoom/`,
declared in `_quarto.yml` as `filters: [mermaid-zoom]`) is written in
the returned-table form, so it never runs under q2 — the pan/zoom
overlay for large diagrams is silently absent from all 14 diagram
pages, with no warning that a declared filter did nothing.

That extension needs a second change too, unrelated to this bug: see
the note on filter ordering below.

## Adjacent finding: user filters run *before* the mermaid transform

Probing with a filter that records what it sees:

```
PROBE codeblock=true rawblock=false
```

Under q2, a mermaid block is still a `CodeBlock` with class `mermaid`
when user filters run; `transforms/mermaid.rs` converts it to a
`RawBlock` of HTML later, at `TransformPhase::Finalization`. Quarto 1
is the other way round — `mermaid-zoom.lua`'s own comment says *"By the
time user filters run, Quarto has already turned a `{mermaid}` cell
into a RawBlock of HTML"* — which is why it matches `RawBlock`.

So a ported filter that wants to react to diagrams must match
`CodeBlock` + class `mermaid` under q2, not `RawBlock`.

`quarto.doc.add_html_dependency` itself works fine (assets land under
`_files/<name>/`), with one caveat: it warns `Q-11-1: field 'version'
is not yet supported and will be ignored`.
