# Plan 7b Phase 0 — research note

**Session:** impl-7b, 2026-09-24. Covers the two Phase 0 research items: pinning
the knitr spin oracle version, and the tree-sitter-r `matchable` spike.

## knitr version pin

**Installed oracle: knitr 1.50** (`Rscript -e 'packageVersion("knitr")'`), R
4.3.2 (aarch64-apple-darwin20). This is the version Phase 4's golden corpus
will be generated with; record here so a future regeneration can diff against
a known-good baseline.

Relevant `NEWS.md` churn for the `spin()` grammar (from `~/src/knitr/NEWS.md`,
all already included in 1.50 since each landed at ≤1.46):

| knitr version | change |
|---|---|
| 1.46 | `spin()` recognizes `# %%` as a chunk delimiter (#2307) |
| 1.46 | `spin()` recognizes `#\|` (pipe options) as starting a chunk (#2320) |
| 1.46 | `spin()` dropped `#-` as a delimiter token — use `#+`/`# %%`/`#\|` (breaking) |
| 1.46 | fixed a regression (#1605) where unparseable input broke `spin()` (#1773) |
| 1.44 | added the `qmd` (Quarto) output format to `spin()` (#2284) — **the branch this plan targets** |
| 1.21 | fixed roxygen-comments-as-string-literals / >3-backtick bug (#1605/#1611) — this is the `matchable` case the tree-sitter-r approach must get right |
| 1.19 | added `-- ---- label ----` chunk delimiter form for external SQL/Haskell spin (not in scope — q2 spin is qmd/Rmd-branch-only, R-only) |
| 0.9 | `## @knitr` chunk-option form added (`#+`/`#-` already existed) |

The plan's grammar table (`2026-07-08-plan7b-native-content-processors.md`
§"spin") already encodes the *current* (1.50) grammar, including the `#-`
removal and the `#|`/`# %%` additions — no further reconciliation needed
before Phase 4 starts.

## tree-sitter-r `matchable` spike

**Goal:** confirm `tree_sitter::Parser` + `tree_sitter_r::LANGUAGE` can
reproduce knitr's `matchable` predicate (`spin.R:73`) — a `#'`/`{{ }}` line is
a real marker only if it is not inside a string literal, and an unparseable
file falls back to "everything matchable."

**Method:** a throwaway example (`crates/quarto-highlight/examples/
spin_matchable_spike.rs`, since deleted — not a deliverable) parsed three
fixtures and walked the tree to classify each line:

1. A well-formed roxygen header + chunk marker (`#' ---` × 3, then `#+
   chunk1`) — every `#'`-prefixed line matchable, the `#+`/code lines not
   (they aren't the pattern under test, just controls).
2. A `#'`-prefixed line **inside** a multi-line string literal (`x <- "a\n#'
   fake marker inside string\nc"`), followed by a real `#' real marker` line
   after the string closes.
3. Text that isn't valid R (`this is not { valid R (((`) preceded by a `#'`
   line.

**Result — confirmed, both invariants hold:**
- Case 2: byte-position lookup against `root.has_error() == false`'s tree
  correctly excludes the string-embedded `#'` line (classified non-matchable)
  and includes the real marker after the string closes.
- Case 3: `root_node().has_error()` is `true` for the malformed R, and the
  fallback path (treat every line as matchable, knitr's own behavior at
  `spin.R:73`) fires correctly.

**Driving pattern** (validated, reusable for Phase 4's real implementation):
`tree_sitter::Parser::new()` → `set_language(&tree_sitter_r::LANGUAGE.into())`
→ `parser.parse(src, None)`. Check `tree.root_node().has_error()` first (parse
failure ⇒ all-matchable, no further walk). Otherwise walk the tree looking
for a `node.kind() == "string"` span containing the candidate byte offset —
`~/src/air/crates/air_r_parser/src/parse.rs`/`treesitter.rs` is confirmed
(read in full this session) to use exactly this walk shape (`has_error()`
check, `node.kind()` string match against tree-sitter-r's native node-kind
names: `"program"`, `"string"`, `"string_content"`, `"comment"`, etc.) for its
own error-resilience fallback — the spike's approach and air's precedent
agree, no fork/vendor needed. `tree-sitter-r = "1.2"` is already a workspace
dependency via `quarto-highlight` (`crates/quarto-highlight/Cargo.toml:27`),
native + wasm32 alike, at zero incremental cost.

**Not yet built:** the real Phase 4 implementation needs to find the
*top-level token start* of a candidate line (not just "is this byte inside a
string somewhere in the tree") — the spike's linear ancestor-walk is a
proof-of-concept, not the final algorithm; Phase 4 should walk from the root
looking for the smallest containing node and check whether that node (or an
ancestor) is a top-level child of `program`, mirroring air's `Preorder`
walker rather than the spike's descend-by-byte-range loop.
