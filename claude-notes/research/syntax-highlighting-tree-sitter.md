# Tree-sitter Syntax Highlighting: Standards and Integration Guide

## Overview

Tree-sitter's syntax highlighting ecosystem is driven by the `tree-sitter-highlight` Rust crate, which provides a standardized system for syntax highlighting based on tree queries. The system is production-grade (used on GitHub.com) and supports language injections, local variable tracking, and hierarchical capture naming conventions derived from TextMate and Sublime Text traditions.

## A. Standard Capture Names and Hierarchy

The canonical capture names are defined in the `tree-sitter-highlight` crate at `crates/highlight/src/highlight.rs:30-87` in the `STANDARD_CAPTURE_NAMES` LazyLock set. The complete list includes:

**Core groups:** `attribute`, `boolean`, `carriage-return`, `comment`, `constant`, `constructor`, `embedded`, `error`, `escape`, `function`, `keyword`, `module`, `number`, `operator`, `property`, `punctuation`, `string`, `tag`, `type`, `variable`

**Hierarchical extensions:** `comment.documentation`, `constant.builtin`, `constructor.builtin`, `function.builtin`, `markup.*` (bold, italic, link, list, quote, raw, strikethrough), `property.builtin`, `punctuation.bracket`, `punctuation.delimiter`, `punctuation.special`, `string.escape`, `string.regexp`, `string.special`, `string.special.symbol`, `type.builtin`, `variable.builtin`, `variable.member`, `variable.parameter`

**Naming convention:** Captures use dot-separated hierarchies following TextMate conventions. The system is not strictly hierarchical—any capture name can be used as long as it follows the dotted format. However, the standard list represents de facto conventions shared across tree-sitter grammars and adopted by Helix, nvim-treesitter, and GitHub.

**Longest-match resolution:** When a capture like `function.builtin.static` is emitted, consumers match against the longest theme key available. For example, if a theme defines `function` and `function.builtin`, the `function.builtin.static` capture will match `function.builtin` rather than `function` (tree-sitter docs, section 3-syntax-highlighting.md).

Non-standard captures (those not in `STANDARD_CAPTURE_NAMES`) are flagged by the `nonconformant_capture_names()` method, useful for validation. Private captures starting with `_` are exempt from this check.

## B. tree-sitter-highlight Rust API

The three core types are:

- **`Highlight`** (line 91): A newtype wrapping `usize`, representing an index into the configured highlight names list. Not a string; consumer code maps it to colors.

- **`HighlightConfiguration`** (line 115): Immutable, thread-shareable struct holding a `Language`, a combined `Query` (merging highlights + injections + locals), and parsed metadata about capture indices for special names like `@injection.content`, `@local.scope`, etc. Created via `HighlightConfiguration::new(language, name, highlights_query, injections_query, locals_query)` (line 353).

- **`Highlighter`** (line 137): Thread-local wrapper around a `Parser` and reusable `QueryCursor` pool. One per thread. Call `highlight(config, source, encoding, injection_callback)` (line 295) to get an iterator of `HighlightEvent`.

**The workflow:**
1. Create a `Highlighter` and `HighlightConfiguration` (or reuse across calls).
2. Call `config.configure(&highlight_names)` to map capture names to indices via dot-hierarchy longest-match (line 472).
3. Call `highlighter.highlight(...)` with a closure that returns `HighlightConfiguration` for injected language names.
4. Iterate over `HighlightEvent` enum variants:
   - `Source {start, end}` — unhighlighted text range
   - `HighlightStart(Highlight)` — begin a highlight (pushed onto a stack)
   - `HighlightEnd` — pop a highlight

**Locals tracking:** The `HighlightConfiguration` parses the locals query to find indices for `@local.scope`, `@local.definition`, `@local.reference`. During highlighting, scopes and definitions are tracked in `LocalScope` / `LocalDef` structures (line 152-163). When a reference is encountered, the system searches enclosing scopes for matching definitions, ensuring all uses of a variable get consistent highlighting.

## C. Language Injections

Injections are detected via `@injection.content` and `@injection.language` captures in the injections query. The `injection_callback` closure passed to `highlight()` is called with a language name string and must return `Option<&HighlightConfiguration>` for that language.

**Injection properties** (tree-sitter docs, 3-syntax-highlighting.md):
- `injection.language` — hardcode language name via `#set!` predicate
- `injection.combined` — parse disjoint ranges as a single document
- `injection.include-children` — default: exclude child nodes; set to reparse entire subtree
- `injection.self` and `injection.parent` — inherit language from current or parent layer

Implementations can disable injections by returning `None` from the callback. Injections create nested "layers" (line 519-699 in highlight.rs), with proper range intersection to avoid parsing outside parent boundaries.

## D. Overlap and Nesting in Event Streams

Events are emitted in byte-order with careful depth-based sorting (line 798-817). Nesting is represented via a stack:

- `HighlightStart(h1)` at byte 0
- `HighlightStart(h2)` at byte 5 (nested inside h1)
- `Source {5, 10}`
- `HighlightEnd` (pops h2)
- `HighlightEnd` (pops h1)

The `HtmlRenderer` (line 1142) maintains a highlights stack and outputs nested `<span>` tags. At line boundaries, spans are closed and reopened to preserve nesting across newlines (line 1294-1303).

## E. Per-Grammar highlights.scm Override

The query paths are configurable via `tree-sitter.json`:
```json
{
  "grammars": [{
    "highlights": "queries/highlights.scm",
    "locals": "queries/locals.scm",
    "injections": "queries/injections.scm"
  }]
}
```

Users can override by specifying alternate paths in `tree-sitter.json` (docs/src/3-syntax-highlighting.md, "Query Paths" section). The tree-sitter CLI also accepts `--query-paths <PATHS>` (cli/highlight.md). In the library API, users pass query strings directly to `HighlightConfiguration::new()`, so overrides are application-specific.

## F. Performance

Tree-sitter queries operate on pre-parsed trees. For a 100-line code block (~3–5 KB):
- Parsing: typically 0.5–2 ms (tree-sitter's C parser is fast)
- Querying (highlights.scm): depends on query complexity; simple grammars ~1–5 ms
- Total: ~2–10 ms for typical grammars (concrete benchmarks not published, but the system is used in GitHub's hot path)

**Caching strategy:** Reuse `Highlighter` and `HighlightConfiguration` across documents; the parser pools cursors internally. For large files, consider processing in chunks.

## References

- `tree-sitter-highlight` crate: `crates/highlight/src/highlight.rs`, lines 30–1378
- Tree-sitter docs: `external-sources/tree-sitter/docs/src/3-syntax-highlighting.md`
- CLI docs: `external-sources/tree-sitter/docs/src/cli/highlight.md`, `init-config.md`
- Helix themes: https://docs.helix-editor.com/themes.html (longest-match hierarchies)
