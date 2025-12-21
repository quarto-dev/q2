# Lua Filter Pipeline Analysis

**Purpose**: Document the quarto-cli Lua filter pipeline to inform the Rust port design.

**Goals**:
1. Identify which stages are pure vs side-effectful
2. Identify which stages use Pandoc Lua API (runtime calls, not AST construction)
3. Understand stage dependencies and data flow
4. Inform design of WASM-compatible "fast preview" bypass

**Analysis Status Legend**:
- [ ] Not started
- [~] Partial (reading code)
- [x] Complete

---

## Pipeline Overview

The filter pipeline is defined in `main.lua` and executes via `run_as_extended_ast()`.

### Execution Order

```
1. pre-ast              (user entry point)
2. quarto_init_filters  (4 stages)
3. quarto_normalize_filters (6 stages, includes quarto_ast_pipeline)
4. post-ast             (user entry point)
5. pre-quarto           (user entry point)
6. quarto_pre_filters   (~17 stages)
7. quarto_crossref_filters (6 stages, conditional on enable-crossref)
8. post-quarto          (user entry point)
9. pre-render           (user entry point)
10. quarto_layout_filters (9 stages)
11. quarto_post_filters  (~29 stages)
12. post-render         (user entry point)
13. pre-finalize        (user entry point)
14. quarto_finalize_filters (7 stages)
15. post-finalize       (user entry point)
```

Total: ~78 internal stages + 8 user entry points

---

## Side Effect Categories

For each stage, we categorize side effects:

| Category | Symbol | Description |
|----------|--------|-------------|
| Pure | `P` | Only reads document AST/Meta, transforms in memory |
| File Read | `FR` | Reads files from filesystem |
| File Write | `FW` | Writes files to filesystem |
| Network | `N` | HTTP/network requests |
| Subprocess | `S` | Spawns external processes |
| Pandoc API | `PA` | Uses pandoc.read/write/pipe/system/path |
| Project Resource | `PR` | Reads project config/resources (cacheable for WASM) |

---

## Analysis Documents

| Group | Stages | Document | Status |
|-------|--------|----------|--------|
| Init | 4 | [01-init-filters.md](./01-init-filters.md) | [x] |
| Normalize | 6 | [02-normalize-filters.md](./02-normalize-filters.md) | [x] |
| Pre | ~17 | [03-pre-filters.md](./03-pre-filters.md) | [x] |
| Crossref | 6 | [04-crossref-filters.md](./04-crossref-filters.md) | [x] |
| Layout | 9 | [05-layout-filters.md](./05-layout-filters.md) | [x] |
| Post | ~29 | [06-post-filters.md](./06-post-filters.md) | [x] |
| Finalize | 7 | [07-finalize-filters.md](./07-finalize-filters.md) | [x] |

---

## Summary Tables (populated as analysis progresses)

### Side Effect Summary by Group

| Group | Pure | File R | File W | Network | Subprocess | Pandoc API |
|-------|------|--------|--------|---------|------------|------------|
| Init | 3 | 1 (includes) | 0 | 0 | 0 | 0 |
| Normalize | 3 | 1 (Typst) | 1 (Typst) | 0 | 1 (Typst) | 3 (`pandoc.read`) |
| Pre | 14 | 1 (shortcodes)* | 1 (results) | 0 | 1 (Shiny) | 1 (`pandoc.utils.references`) |
| Crossref | 5 | 0 | 1 (index) | 0 | 0 | 1 (`pandoc.write`) |
| Layout | 6 | 1 (manuscripts) | 0 | 0 | 0 | 1 (lightbox) |
| Post | ~22 | 2 (email, book) | 3 (cites, email) | 0 | 1 (rsvg) | ~8 (`pandoc.write`) |
| Finalize | 4 | 0 | 3 (mediabag, cites, deps) | 0 | 0 | 0 |
| **Total** | **~57** | **6** | **9** | **0** | **3** | **~14** |

*Shortcode file loading happens at init, env shortcode reads `os.getenv()`

### WASM Compatibility Summary

| Group | WASM-Safe | Needs VFS | Blocked (non-HTML) |
|-------|-----------|-----------|-------------------|
| Init | 3 | 1 (include files) | 0 |
| Normalize | 3 | 0 | 1 (Typst juice.ts)* |
| Pre | 14 | 1 (results file) | 1 (Shiny subprocess)* |
| Crossref | 5 | 1 (index file) | 0 |
| Layout | 6 | 1 (manuscripts) | 0 |
| Post | ~22 | 0 | 2 (pdf-images, email)* |
| Finalize | 4 | 3 (mediabag, cites, deps) | 0 |
| **Total** | **~57** | **7** | **4*** |

*These blockers only apply to non-HTML output formats. **For HTML live preview, blocked = 0**.

---

## Key Observations

### WASM Feasibility

1. **~73% of stages are pure** (~57 of ~78 stages). These can run directly in WASM.

2. **4 stages have subprocess calls, but none apply to HTML output**:
   - Typst juice.ts → Typst only (also: JS callback possible via NPM implementation)
   - Shiny Python → Shiny documents only
   - PDF image conversion (rsvg-convert) → PDF only
   - Email rendering → Email format only, and only via `quarto render`

3. **~7 stages need VFS**: File reads/writes that could be redirected to virtual filesystem:
   - Include files, manuscripts notebooks, results/index files, mediabag, cites

4. **~14 stages use Pandoc API** (`pandoc.read`, `pandoc.write`, `pandoc.pipe`):
   - These would need either Pandoc WASM or Rust-native replacements
   - Most are for format-specific output generation (LaTeX, Typst, etc.)
   - For HTML output, many of these can be skipped or replaced with pampa

### Pandoc API Usage Patterns

1. **`pandoc.read()`**: Used for parsing embedded content (HTML tables, markdown in attributes)
   - Could be replaced by pampa/native parsing in Rust

2. **`pandoc.write()`**: Used for format conversion (AST → LaTeX/HTML/etc.)
   - Could be replaced by quarto-doctemplate in Rust

3. **`pandoc.pipe()`**: Used for external tool invocation
   - Blocks WASM, requires external binaries

4. **`pandoc.utils.references()`**: Bibliography extraction
   - Would need citeproc in Rust or pre-extraction

### Rust Port Implications

1. **Pure stages** can be directly ported as Rust filters
2. **File I/O stages** need abstraction layer for WASM compatibility
3. **Pandoc API stages** need native Rust implementations or skip conditions
4. **Subprocess stages** must be disabled in WASM or pre-processed

### WASM "Fast Preview" Strategy

**Target**: HTML output only (live preview use case)

For a WASM-based quarto preview:
1. **Pre-process**: Run side-effectful stages before WASM (results, index files)
2. **VFS**: Load project resources into virtual filesystem
3. **Replace**: Use native Rust parsers instead of `pandoc.read/write`

**WASM Blockers Don't Apply for HTML Preview**:

| Blocker | Why It Doesn't Apply |
|---------|---------------------|
| juice.ts (Typst) | Only runs for Typst output, not HTML |
| rsvg-convert (PDF) | Only runs for PDF output, not HTML |
| Email rendering | Only for email format, and only via `quarto render` |
| Shiny Python | Specialized interactive document type |

**Note on juice.ts**: Even for Typst output, juice has an NPM implementation. In WASM, we could potentially call back into JS for this API rather than spawning a subprocess. This is a viable strategy for any Node/Deno libraries that have browser-compatible implementations.

**Result**: For HTML live preview, **all WASM blockers are avoided**. The only considerations are:
- File I/O stages need VFS or in-memory handling
- Pandoc API calls need Rust-native replacements

This makes ~95%+ of the HTML pipeline WASM-compatible.

---

## References

- Main filter orchestration: `external-sources/quarto-cli/src/resources/filters/main.lua`
- Custom node system: `external-sources/quarto-cli/src/resources/filters/ast/`
- Filter modules: `external-sources/quarto-cli/src/resources/filters/modules/`
- Common utilities: `external-sources/quarto-cli/src/resources/filters/common/`
