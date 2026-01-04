# Coverage Progress

Last verified against coverage report: 2026-01-04

## Crate: comrak-to-pandoc

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/block.rs | done | 88.93% | Remaining uncovered: panic paths for GFM extensions (unreachable in CommonMark-only mode) |
| src/compare.rs | done | 99.82% | |
| src/inline.rs | done | 87.87% | Remaining uncovered: panic paths for GFM extensions (unreachable in CommonMark-only mode) |
| src/lib.rs | done | 90.70% | Minimal file - remaining uncovered are test panic paths |
| src/normalize.rs | done | 97.78% | |
| src/source_location.rs | done | 100% | |
| src/text.rs | done | 98.57% | Already has comprehensive tests |

## Crate: pampa

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/bin/ast_reconcile.rs | skipped | 0% | Binary entry point |
| src/citeproc_filter.rs | done | 93.76% | Improved from 71.85% with 38 new tests |
| src/errors.rs | done | 95.51% | Improved from 71.15% with 24 new tests |
| src/filter_context.rs | done | 100% | Improved from 90.77% with 6 new tests |
| src/filters.rs | done | 96.30% | Improved from 78.77% with 68 new tests |
| src/json_filter.rs | done | 93.58% | Improved from 84.86% with 10 new tests |
| src/lua/constructors.rs | done | 92.79% | Improved from 54.38% with 164 new tests |
| src/lua/diagnostics.rs | done | 97.84% | Improved from 85.43% with 55 new tests covering all Inline/Block variants and error paths |
| src/lua/filter.rs | done | 87.55% | Improved from 66.48% with 31 unit tests and 12 integration tests covering error types, traversal control, return handling, and various block types |
| src/lua/json.rs | done | 96.81% | Improved from 75.70% with 20 new tests covering null sentinel, encode/decode, special values, and edge cases |
| src/lua/list.rs | done | 98.12% | Improved from 78.49% with 33 new tests covering posrelat, metatables, List methods, and edge cases |
| src/lua/mediabag.rs | done | 94.41% | Improved from 84.63% with 22 new tests covering items iterator, fill, fetch cache, write errors, subdirectories, and all MIME types. Remaining uncovered: URL fetch path requires network mocking. |
| src/lua/path.rs | done | 92.91% | Improved from 78.26% with 24 new tests covering join edge cases, split components, unsafe make_relative, compute_relative_path, and more. Remaining uncovered: Windows-specific paths (Prefix, quotes) that can't be tested on Linux. |
| src/lua/readwrite.rs | done | 92.26% | Improved from 65.32% with 40 new tests covering parse_format_spec, config_value_to_lua, lua_to_config_value, error paths, all writer formats, and option constructors |
| src/lua/system.rs | done | 90.28% | Improved from 66.38% with 21 new tests covering cputime, times, with_environment, with_working_directory, XDG variants, recursive dir operations, and error cases |
| src/lua/text.rs | done | 100% | Improved from 95.08% with 13 new tests |
| src/lua/types.rs | done | 76.63% | Large file (2650 lines) with extensive AST type wrappers. Already has tests for tag_name, field_names. Remaining: get_field/set_field and conversion functions for all inline/block variants would require significant effort. |
| src/lua/utils.rs | done | 82.97% | Improved from 54.93% with 62 new tests covering stringify, blocks_to_inlines, equals, type, sha1, normalize_date (all formats), to_roman_numeral, and helper functions |
| src/main.rs | skipped | 30.08% | Binary entry point |
| src/options.rs | done | 99.63% | Improved from 97.03% - remaining line is unreachable defensive code |
| src/pandoc/ast_context.rs | done | 99.08% | Improved from 83.67% with 10 new tests |
| src/pandoc/location.rs | done | 98.89% | Improved from 98.39% with 2 new tests |
| src/pandoc/meta.rs | done | 94.67% | Improved from 91.43% with 16 new tests |
| src/pandoc/shortcode.rs | done | 97.79% | Improved from 97.07% with 2 new tests |
| src/pandoc/treesitter.rs | not_started | 75.69% | |
| src/pandoc/treesitter_utils/atx_heading.rs | not_started | 70.31% | |
| src/pandoc/treesitter_utils/block_quote.rs | not_started | 78.43% | |
| src/pandoc/treesitter_utils/caption.rs | done | 100% | |
| src/pandoc/treesitter_utils/citation.rs | not_started | 96.61% | |
| src/pandoc/treesitter_utils/code_fence_content.rs | not_started | 92.86% | |
| src/pandoc/treesitter_utils/code_span_helpers.rs | not_started | 88.37% | |
| src/pandoc/treesitter_utils/commonmark_attribute.rs | not_started | 89.74% | |
| src/pandoc/treesitter_utils/document.rs | not_started | 94.12% | |
| src/pandoc/treesitter_utils/editorial_marks.rs | not_started | 56.86% | |
| src/pandoc/treesitter_utils/fenced_code_block.rs | not_started | 78.95% | |
| src/pandoc/treesitter_utils/fenced_div_block.rs | not_started | 53.42% | |
| src/pandoc/treesitter_utils/info_string.rs | done | 100% | |
| src/pandoc/treesitter_utils/language_attribute.rs | not_started | 0% | |
| src/pandoc/treesitter_utils/list_marker.rs | not_started | 96.43% | |
| src/pandoc/treesitter_utils/note_definition_fenced_block.rs | not_started | 80.00% | |
| src/pandoc/treesitter_utils/note_definition_para.rs | not_started | 89.66% | |
| src/pandoc/treesitter_utils/numeric_character_reference.rs | not_started | 76.19% | |
| src/pandoc/treesitter_utils/paragraph.rs | done | 100% | |
| src/pandoc/treesitter_utils/pipe_table.rs | not_started | 84.55% | |
| src/pandoc/treesitter_utils/postprocess.rs | not_started | 75.14% | |
| src/pandoc/treesitter_utils/quote_helpers.rs | not_started | 73.86% | |
| src/pandoc/treesitter_utils/section.rs | not_started | 76.32% | |
| src/pandoc/treesitter_utils/shortcode.rs | not_started | 50.35% | |
| src/pandoc/treesitter_utils/span_link_helpers.rs | not_started | 97.20% | |
| src/pandoc/treesitter_utils/text_helpers.rs | not_started | 72.46% | |
| src/pandoc/treesitter_utils/thematic_break.rs | done | 100% | |
| src/pandoc/treesitter_utils/uri_autolink.rs | not_started | 80.81% | |
| src/readers/commonmark.rs | not_started | 87.72% | |
| src/readers/json.rs | not_started | 61.28% | |
| src/readers/qmd.rs | not_started | 85.23% | |
| src/readers/qmd_error_message_table.rs | not_started | 53.85% | |
| src/readers/qmd_error_messages.rs | not_started | 40.00% | |
| src/template/builtin.rs | not_started | 96.00% | |
| src/template/bundle.rs | not_started | 92.16% | |
| src/template/config_merge.rs | not_started | 88.16% | |
| src/template/context.rs | not_started | 82.66% | |
| src/template/render.rs | not_started | 83.33% | |
| src/traversals.rs | done | 100% | |
| src/unified_filter.rs | not_started | 84.00% | |
| src/utils/autoid.rs | done | 100% | |
| src/utils/concrete_tree_depth.rs | done | 100% | |
| src/utils/diagnostic_collector.rs | done | 100% | Improved from 91.11% with 6 new tests |
| src/utils/output.rs | not_started | 40.00% | |
| src/utils/text.rs | done | 100% | |
| src/utils/trim_source_location.rs | not_started | 91.07% | |
| src/wasm_entry_points/mod.rs | not_started | 75.00% | |
| src/writers/ansi.rs | not_started | 78.23% | Tests added, needs more work |
| src/writers/html.rs | not_started | 67.71% | |
| src/writers/html_source.rs | not_started | 80.81% | |
| src/writers/json.rs | not_started | 82.73% | |
| src/writers/native.rs | not_started | 65.56% | |
| src/writers/plaintext.rs | done | 100% | Improved from 99.69% with 1 new test |
| src/writers/qmd.rs | not_started | 67.05% | |

## Crate: qmd-syntax-helper

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/conversions/apostrophe_quotes.rs | not_started | 94.74% | |
| src/conversions/attribute_ordering.rs | not_started | 24.29% | |
| src/conversions/definition_lists.rs | not_started | 3.54% | |
| src/conversions/grid_tables.rs | not_started | 38.24% | |
| src/conversions/q_2_11.rs | not_started | 92.97% | |
| src/conversions/q_2_12.rs | not_started | 92.97% | |
| src/conversions/q_2_13.rs | not_started | 92.97% | |
| src/conversions/q_2_15.rs | not_started | 71.43% | |
| src/conversions/q_2_16.rs | not_started | 73.44% | |
| src/conversions/q_2_17.rs | not_started | 72.80% | |
| src/conversions/q_2_18.rs | not_started | 72.80% | |
| src/conversions/q_2_19.rs | not_started | 73.44% | |
| src/conversions/q_2_20.rs | not_started | 73.44% | |
| src/conversions/q_2_21.rs | not_started | 73.44% | |
| src/conversions/q_2_22.rs | not_started | 73.44% | |
| src/conversions/q_2_23.rs | not_started | 21.09% | |
| src/conversions/q_2_24.rs | not_started | 21.60% | |
| src/conversions/q_2_25.rs | not_started | 21.60% | |
| src/conversions/q_2_26.rs | not_started | 21.09% | |
| src/conversions/q_2_28.rs | not_started | 93.67% | |
| src/conversions/q_2_33.rs | not_started | 4.38% | |
| src/conversions/q_2_5.rs | not_started | 92.74% | |
| src/conversions/q_2_7.rs | not_started | 4.55% | |
| src/diagnostics/parse_check.rs | not_started | 9.84% | |
| src/diagnostics/q_2_30.rs | not_started | 5.50% | |
| src/main.rs | skipped | 0% | Binary entry point |
| src/rule.rs | not_started | 83.33% | |
| src/utils/file_io.rs | not_started | 75.00% | |
| src/utils/glob_expand.rs | not_started | 76.79% | |
| src/utils/resources.rs | not_started | 92.21% | |

## Crate: quarto-citeproc

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/bin/csl_conformance_report.rs | skipped | 0% | Binary entry point |
| src/disambiguation.rs | not_started | 81.72% | |
| src/error.rs | done | 100% | |
| src/eval.rs | not_started | 93.96% | |
| src/locale.rs | not_started | 81.69% | |
| src/locale_parser.rs | not_started | 93.54% | |
| src/output.rs | not_started | 75.32% | |
| src/reference.rs | not_started | 92.21% | |
| src/types.rs | not_started | 89.53% | |

## Crate: quarto-config

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/convert.rs | done | 100% | |
| src/materialize.rs | not_started | 93.39% | |
| src/merged.rs | not_started | 91.98% | |
| src/tag.rs | not_started | 91.84% | |
| src/types.rs | done | 100% | |

## Crate: quarto-core

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/artifact.rs | not_started | 84.87% | |
| src/error.rs | done | 100% | |
| src/format.rs | done | 100% | |
| src/pipeline.rs | not_started | 91.71% | |
| src/project.rs | not_started | 78.41% | |
| src/render.rs | not_started | 96.62% | |
| src/resources.rs | not_started | 87.01% | |
| src/template.rs | not_started | 97.88% | |
| src/transform.rs | not_started | 95.45% | |
| src/transforms/callout.rs | not_started | 86.07% | |
| src/transforms/callout_resolve.rs | not_started | 85.96% | |
| src/transforms/metadata_normalize.rs | not_started | 95.80% | |
| src/transforms/resource_collector.rs | not_started | 59.09% | |
| src/transforms/title_block.rs | not_started | 86.41% | |

## Crate: quarto-csl

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/error.rs | done | 100% | |
| src/parser.rs | not_started | 93.40% | |
| src/types.rs | done | 100% | |

## Crate: quarto-doctemplate

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/ast.rs | done | 100% | |
| src/context.rs | not_started | 88.80% | |
| src/doc.rs | not_started | 99.52% | |
| src/eval_context.rs | not_started | 98.78% | |
| src/evaluator.rs | not_started | 94.60% | |
| src/parser.rs | not_started | 81.42% | |
| src/resolver.rs | done | 100% | |

## Crate: quarto-error-reporting

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/builder.rs | not_started | 88.99% | |
| src/catalog.rs | done | 100% | |
| src/diagnostic.rs | not_started | 87.00% | |
| src/macros.rs | done | 100% | |

## Crate: quarto-hub

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/context.rs | not_started | 0% | |
| src/discovery.rs | not_started | 98.08% | |
| src/index.rs | not_started | 91.41% | |
| src/main.rs | skipped | 0% | Binary entry point |
| src/peer.rs | not_started | 0% | |
| src/server.rs | not_started | 0% | |
| src/storage.rs | not_started | 74.66% | |
| src/sync.rs | not_started | 73.76% | |
| src/sync_state.rs | not_started | 98.63% | |
| src/watch.rs | not_started | 90.18% | |

## Crate: quarto-pandoc-types

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/attr.rs | not_started | 87.41% | |
| src/config_value.rs | not_started | 97.50% | |
| src/custom.rs | not_started | 97.17% | |
| src/inline.rs | not_started | 95.80% | |
| src/meta.rs | not_started | 94.74% | |
| src/reconcile/apply.rs | not_started | 73.16% | |
| src/reconcile/compute.rs | not_started | 87.89% | |
| src/reconcile/hash.rs | not_started | 87.68% | |
| src/reconcile/mod.rs | not_started | 93.60% | |
| src/reconcile/types.rs | not_started | 97.66% | |

## Crate: quarto-parse-errors

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| error-message-macros/src/lib.rs | not_started | 92.86% | |
| src/error_generation.rs | not_started | 79.83% | |
| src/error_table.rs | done | 100% | |
| src/tree_sitter_log.rs | not_started | 92.61% | |

## Crate: quarto-source-map

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/context.rs | not_started | 98.60% | |
| src/file_info.rs | done | 100% | |
| src/mapping.rs | not_started | 99.36% | |
| src/source_info.rs | not_started | 97.67% | |
| src/types.rs | done | 100% | |
| src/utils.rs | done | 100% | |

## Crate: quarto-system-runtime

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/lib.rs | done | 100% | |
| src/native.rs | not_started | 91.44% | |
| src/sandbox.rs | not_started | 56.44% | |
| src/traits.rs | not_started | 77.17% | |

## Crate: quarto-treesitter-ast

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/traversals.rs | not_started | 80.25% | |

## Crate: quarto-util

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/version.rs | not_started | 94.44% | |

## Crate: quarto-xml

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/context.rs | not_started | 60.87% | |
| src/error.rs | not_started | 74.12% | |
| src/parser.rs | not_started | 90.58% | |
| src/types.rs | not_started | 90.31% | |

## Crate: quarto-yaml

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/error.rs | done | 100% | |
| src/parser.rs | not_started | 95.67% | |
| src/yaml_with_source_info.rs | not_started | 95.77% | |

## Crate: quarto-yaml-validation

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/diagnostic.rs | not_started | 83.86% | |
| src/error.rs | not_started | 99.85% | |
| src/schema/annotations.rs | not_started | 94.12% | |
| src/schema/helpers.rs | not_started | 97.90% | |
| src/schema/merge.rs | not_started | 83.97% | |
| src/schema/mod.rs | not_started | 87.89% | |
| src/schema/parser.rs | not_started | 80.81% | |
| src/schema/parsers/arrays.rs | not_started | 95.00% | |
| src/schema/parsers/combinators.rs | not_started | 94.95% | |
| src/schema/parsers/enum.rs | not_started | 76.47% | |
| src/schema/parsers/objects.rs | not_started | 74.07% | |
| src/schema/parsers/primitive.rs | not_started | 80.00% | |
| src/schema/parsers/ref.rs | not_started | 80.00% | |
| src/schema/parsers/wrappers.rs | not_started | 53.33% | |
| src/validator.rs | not_started | 98.45% | |

## Crate: quarto (binary)

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/commands/add.rs | skipped | 0% | CLI stub |
| src/commands/call.rs | skipped | 0% | CLI stub |
| src/commands/check.rs | skipped | 0% | CLI stub |
| src/commands/convert.rs | skipped | 0% | CLI stub |
| src/commands/create.rs | skipped | 0% | CLI stub |
| src/commands/install.rs | skipped | 0% | CLI stub |
| src/commands/list.rs | skipped | 0% | CLI stub |
| src/commands/pandoc.rs | skipped | 0% | CLI stub |
| src/commands/preview.rs | skipped | 0% | CLI stub |
| src/commands/publish.rs | skipped | 0% | CLI stub |
| src/commands/remove.rs | skipped | 0% | CLI stub |
| src/commands/render.rs | not_started | 23.81% | Has significant logic |
| src/commands/run.rs | skipped | 0% | CLI stub |
| src/commands/serve.rs | skipped | 0% | CLI stub |
| src/commands/tools.rs | skipped | 0% | CLI stub |
| src/commands/typst.rs | skipped | 0% | CLI stub |
| src/commands/uninstall.rs | skipped | 0% | CLI stub |
| src/commands/update.rs | skipped | 0% | CLI stub |
| src/commands/use_cmd.rs | skipped | 0% | CLI stub |
| src/main.rs | skipped | 0% | Binary entry point |

## Crate: tree-sitter-doctemplate

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/lib.rs | done | 100% | |

## Crate: tree-sitter-qmd

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| bindings/rust/benchmark.rs | skipped | 0% | Benchmark binary |
| bindings/rust/lib.rs | done | 100% | |
| bindings/rust/parser.rs | not_started | 49.53% | Generated parser |

## Crate: validate-yaml

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/main.rs | skipped | 0% | Binary entry point |

## Crate: wasm-qmd-parser

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/lib.rs | skipped | 0% | WASM module |
| src/utils.rs | skipped | 0% | WASM utilities |

---

## Summary

- **Total files**: 242
- **Done**: 37 (includes 31 at 100% and 6 at 90%+)
- **Skipped**: 27 (binaries, CLI stubs, WASM)
- **Not started**: 178

### Files at 100% coverage
- comrak-to-pandoc/src/source_location.rs
- pampa/src/filter_context.rs (improved from 90.77%)
- pampa/src/lua/text.rs (improved from 95.08%)
- pampa/src/pandoc/treesitter_utils/caption.rs
- pampa/src/pandoc/treesitter_utils/info_string.rs
- pampa/src/pandoc/treesitter_utils/paragraph.rs
- pampa/src/pandoc/treesitter_utils/thematic_break.rs
- pampa/src/traversals.rs
- pampa/src/utils/autoid.rs
- pampa/src/utils/concrete_tree_depth.rs
- pampa/src/utils/diagnostic_collector.rs (improved from 91.11%)
- pampa/src/utils/text.rs
- pampa/src/writers/plaintext.rs (improved from 99.69%)
- quarto-citeproc/src/error.rs
- quarto-config/src/convert.rs
- quarto-config/src/types.rs
- quarto-core/src/error.rs
- quarto-core/src/format.rs
- quarto-csl/src/error.rs
- quarto-csl/src/types.rs
- quarto-doctemplate/src/ast.rs
- quarto-doctemplate/src/resolver.rs
- quarto-error-reporting/src/catalog.rs
- quarto-error-reporting/src/macros.rs
- quarto-parse-errors/src/error_table.rs
- quarto-source-map/src/file_info.rs
- quarto-source-map/src/types.rs
- quarto-source-map/src/utils.rs
- quarto-system-runtime/src/lib.rs
- quarto-yaml/src/error.rs
- tree-sitter-doctemplate/src/lib.rs
- tree-sitter-qmd/bindings/rust/lib.rs

### Files at 99%+ coverage (remaining lines are unreachable/defensive)
- pampa/src/options.rs (99.63%)
- pampa/src/pandoc/ast_context.rs (99.08%)
- pampa/src/pandoc/location.rs (98.89%)
