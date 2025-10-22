# Performance Analysis: qmd-syntax-helper div-whitespace Rule

**Date:** 2025-10-21
**Profiling Tool:** samply + flamegraph
**Build Mode:** Release with debug symbols (`CARGO_PROFILE_RELEASE_DEBUG=true`)
**Test Corpus:** `external-sites/**/*.qmd` (509 files)
**Total Runtime:** ~12 seconds
**Total Samples:** 7,501

## Executive Summary

The div-whitespace rule's performance is dominated by **parsing time** (tree-sitter), not by the offset calculation algorithm. The O(N²) bug fixed earlier only manifests when files have actual div-whitespace errors to process. Since the test corpus has zero files with div-whitespace issues, the performance is almost entirely spent in the parsing phase.

**Key Finding:** ~95% of execution time is spent in `tree_sitter` parsing (`ts_parser_parse`), not in the div-whitespace detection logic itself.

## Performance Breakdown

### Top Functions by Sample Count

| Samples | % | Function | Component |
|---------|---|----------|-----------|
| 7,498 | 99.96% | `qmd_syntax_helper::main` | Main program |
| 7,480 | 99.72% | `DivWhitespaceConverter::check` | Rule check entry |
| 7,440 | 99.19% | `get_parse_errors` | Parser invocation |
| 7,194 | 95.91% | `quarto_markdown_pandoc::readers::qmd::read` | QMD parser |
| 5,680 | 75.72% | `tree_sitter_qmd::parser::MarkdownParser::parse` | Tree-sitter wrapper |
| 5,627 | 75.01% | `ts_parser_parse_with_options` | Tree-sitter core |
| 5,611 | 74.80% | `ts_parser_parse` | Tree-sitter parsing |
| 3,707 | 49.41% | `TreeSitterLogObserver::log` | Debug logging |
| 927 | 12.36% | `snprintf` | String formatting |
| 615 | 8.20% | `HashMap::insert` | Hash table ops |
| 611 | 8.15% | `treesitter_to_pandoc` | AST conversion |
| 274 | 3.65% | `ts_lex` | Lexical analysis |
| 272 | 3.63% | `_nanov2_free` | Memory deallocation |

### Time Distribution by Phase

1. **Parsing (tree-sitter):** ~75% of total time
   - Tree-sitter core parsing: 5,611 samples (75%)
   - Logging overhead: 3,707 samples (49%)
   - Lexing: 274 samples (4%)

2. **AST Conversion:** ~8% of total time
   - `treesitter_to_pandoc`: 611 samples (8%)
   - HashMap operations: 615 samples (8%)

3. **String Formatting (logging):** ~12% of total time
   - `snprintf`/`vsnprintf`: 927+892 = 1,819 samples (24%)
   - Note: This is part of tree-sitter logging overhead

4. **Memory Management:** ~4% of total time
   - `_nanov2_free`: 272+204 = 476 samples (6%)
   - Various malloc/realloc: ~200 samples (3%)

5. **Div-Whitespace Logic:** <1% of total time
   - `find_div_whitespace_errors`: Negligible (not in top 50)
   - The O(N²) bug we fixed doesn't show up because there are no errors

## Analysis

### Why Parsing Dominates

The `div-whitespace` rule works by:
1. **Parse the file** with `quarto_markdown_pandoc::readers::qmd::read` (~95% of time)
2. **Extract errors** from parsing diagnostics
3. **Find div fence patterns** in the errors (< 1% of time)
4. **Calculate byte offsets** for fixes (< 1% of time)

Since step #1 dominates, the overall performance appears slow. However, this is expected behavior - tree-sitter parsing is expensive, especially with:
- Debug logging enabled (TreeSitterLogObserver)
- 509 files being processed sequentially
- Each file spawning a new parser instance

### Logging Overhead

The `TreeSitterLogObserver::log` function accounts for ~49% of samples (3,707/7,501). This is significant overhead that could be eliminated in production by:
- Disabling tree-sitter logging when not in verbose mode
- Using conditional compilation to remove logging code entirely

**Potential speedup:** 2x if logging is disabled

### The O(N²) Fix Impact

The O(N²) bug we fixed (pre-computing line start offsets) doesn't show up in this profile because:
1. The test corpus has **zero files with div-whitespace errors**
2. The `find_div_whitespace_errors` function is only called on files with parse errors
3. When called, it processes the error list (usually small) rather than the full file

**When would the fix matter?**
- Files with many div-whitespace errors (e.g., 100+ `:::{` patterns)
- Large files (10,000+ lines) with errors throughout
- In those cases, the fix changes O(N²) → O(N), potentially 100x+ speedup

### Memory Allocation Patterns

Memory operations (`_nanov2_free`, `malloc`, etc.) account for ~6-9% of time. This is reasonable for a program that:
- Builds ASTs for 509 files
- Creates diagnostic message objects
- Manages hash maps for symbol resolution

No obvious memory allocation bottlenecks.

## Recommendations

### Immediate Optimizations (If Needed)

1. **Disable tree-sitter logging in non-verbose mode**
   - Remove ~49% overhead
   - Change `quarto_markdown_pandoc::readers::qmd::read` to accept logging flag
   - Expected speedup: ~2x

2. **Parse files in parallel**
   - Currently sequential: 509 files × ~24ms/file = 12s
   - With 8-core parallelism: 509 files / 8 × 24ms = ~1.5s
   - Expected speedup: ~8x (on 8-core machine)

3. **Cache parse results**
   - If files are checked multiple times, cache the parse tree
   - Only re-parse on file modification
   - Expected speedup: ∞ (amortized)

### Long-Term Optimizations

1. **Incremental parsing**
   - Tree-sitter supports incremental re-parsing
   - Only reparse changed sections of files
   - Requires more complex state management

2. **Lazy parsing**
   - Only parse files that likely have div-whitespace errors
   - Use regex pre-screening: `:::[{]` pattern
   - Trade-off: might miss some edge cases

3. **Reduce AST conversion overhead**
   - `treesitter_to_pandoc` accounts for 8% of time
   - Could avoid full AST conversion if only checking for errors
   - Just extract error nodes instead of building full Pandoc AST

## Conclusion

The `div-whitespace` rule's performance is **not** limited by the algorithm we optimized (line offset calculation). The real bottleneck is:

1. **Tree-sitter parsing** (75% of time)
2. **Debug logging overhead** (49% of time)
3. **Sequential file processing** (no parallelism)

The O(N²) fix we implemented is still valuable and correct - it just doesn't show up in this benchmark because the test corpus has no div-whitespace errors.

### Performance in Context

**Current:** 12 seconds for 509 files = ~24ms/file
**With logging disabled:** ~6 seconds (est.)
**With 8-core parallelism:** ~0.75 seconds (est.)
**With both optimizations:** ~0.4 seconds (est.) = **30x speedup**

The performance is reasonable for a syntax checking tool. Most users won't notice 24ms per file unless processing thousands of files.

## Appendix: Profiling Commands Used

```bash
# Install samply
cargo install samply

# Profile with samply (JSON output)
samply record --save-only --output /tmp/samply-profile.json \
  cargo run --release --bin qmd-syntax-helper -- \
  check --rule div-whitespace 'external-sites/**/*.qmd'

# Install flamegraph
cargo install flamegraph

# Generate flamegraph with debug symbols
CARGO_PROFILE_RELEASE_DEBUG=true \
  cargo flamegraph --bin qmd-syntax-helper -o /tmp/flamegraph-debug.svg -- \
  check --rule div-whitespace '../../external-sites/**/*.qmd'

# Extract top functions
grep -o '<title>[^<]*samples[^<]*' /tmp/flamegraph-debug.svg | \
  sed 's/<title>//' | \
  awk -F'[()]' '{n=split($2, a, " "); print a[1] "\t" $1}' | \
  sort -rn | head -50
```

## Appendix: Test Corpus Stats

- **Total files:** 509
- **Files with div-whitespace issues:** 0
- **Total lines:** Unknown (not measured)
- **Average file size:** Unknown (not measured)

The test corpus is from `external-sites/quarto-web/docs/**/*.qmd`, which consists of Quarto documentation files. These files are generally well-formed and don't contain div-whitespace errors, which is why the error-processing code path is never executed in this benchmark.
