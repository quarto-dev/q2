# Repro output at HEAD 15bb0d54f (2026-10-09)

Run `q2 render` in `repro/`.

All 5 pages:

```
Warning [Q-12-10]: Re-parsing rendered listing `tbl` failed with 1 diagnostic(s); first: Unclosed Underscore Emphasis. Listing skipped.
Warning [Q-12-10]: Re-parsing rendered listing `dflt` failed with 1 diagnostic(s); first: Unclosed Underscore Emphasis. Listing skipped.
```

Without p/a.qmd and p/c.qmd (the `_scope` titles):

```
Warning [Q-12-10]: Re-parsing rendered listing `tbl` produced 1 diagnostic(s); first: HTML element converted to raw HTML
Warning [Q-12-10]: Re-parsing rendered listing `dflt` produced 1 diagnostic(s); first: HTML element converted to raw HTML

<a href="p/d.html" class="no-anchor no-external listing-title">About <anonymous> frames</a></h3>
<a href="p/b.html" class="no-anchor no-external listing-title">Plan for q2 preview and emph</a></h3>
<td><a href="p/d.html" class="no-external">About <anonymous> frames</a>
<td><a href="p/b.html" class="no-external">Plan for q2 preview and emph</a>
```

Each page renders its own title correctly (`<code>_scope</code>`, `<code>&lt;anonymous&gt;</code>`).

# Phase 0 probes (2026-10-09)

## Multi-paragraph description (D5)

`description: |` with two paragraphs → `PandocBlocks`, and `ConfigValue::as_plain_text` returns
`None` for blocks, so **the description is silently dropped**. A table listing with
`fields: [title, description]` shows an empty description cell, with no diagnostic. A single-paragraph
description renders, but flattened (`Single *para* desc` → `Single para desc`).

## `|` inside a code span in a pipe-table cell

Pandoc 3.11 and pampa agree:
- `` | `a|b` x | `` → `Code "a|b"`: the code span protects the pipe;
- `` | `a\|b` x | `` → `Code "a\\|b"`: the backslash is **kept** inside code;
- `| $a|b$ x |` → `Math InlineMath "a|b"`: math protects it too.

So today's `escape_table_cell` (a global `|` → `\|`) would corrupt a code span that contains a pipe.
The qmd writer already escapes `|` in `Str` text, so a cell written from inlines needs only line
breaks flattened (SoftBreak/LineBreak → Space before writing), not the global pipe escape.
