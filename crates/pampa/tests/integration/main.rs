//! Pampa integration test binary.
//!
//! All integration tests live as `pub mod` submodules of this binary
//! rather than as separate per-file binaries. See bd-xvdop and
//! claude-notes/plans/2026-05-28-integration-test-consolidation.md
//! for the rationale (matklad / posit-dev/ark#1240 pattern: one
//! binary statically links pampa's dependency closure once, rather
//! than ~57 times).
//!
//! The harness handles `#[test]` discovery across all modules, and
//! nextest still runs each test in its own process for isolation.

pub mod attribution_html_coalescing_test;
pub mod attribution_json_wire_test;
pub mod incremental_writer_investigation;
pub mod incremental_writer_tests;
pub mod inline_span_investigation;
pub mod inline_splice_integration_tests;
pub mod inline_splice_property_tests;
pub mod inline_splice_safety_tests;
pub mod json_location_test;
pub mod json_reader_smoke_tests;
pub mod lua_conformance;
pub mod lua_differential;
pub mod nesting_cursor_roundtrip_tests;
pub mod node_edit_tests;
pub mod qmd_writer_source_info;
pub mod regenerate_nested_buffers_tests;
pub mod table_caption_provenance;
pub mod test;
pub mod test_ansi_writer;
pub mod test_attr_source_parsing;
pub mod test_attr_source_structure;
pub mod test_bare_lt_str;
pub mod test_blockquote_multiline_attrs;
pub mod test_citeproc_integration;
pub mod test_cli_input_arg;
pub mod test_code_block_attributes;
pub mod test_code_block_writer_roundtrip;
pub mod test_code_span;
pub mod test_diagnostic_determinism;
pub mod test_diagnostic_path_normalization;
pub mod test_editorial_mark_spacing;
pub mod test_email_autolink;
pub mod test_emphasis_opening_mark;
pub mod test_error_corpus;
pub mod test_figure_figcaption_synthesis;
pub mod test_grid_table_error;
pub mod test_hard_soft_break;
pub mod test_heading_auto_id;
pub mod test_html_attr_handling;
pub mod test_indented_continuation;
pub mod test_inline_locations;
pub mod test_json_div_transforms;
pub mod test_json_errors;
pub mod test_json_roundtrip;
pub mod test_link_destination_linebreak;
pub mod test_location_health;
pub mod test_lua_attr_mutation;
pub mod test_lua_constructors;
pub mod test_lua_content_mutation;
pub mod test_lua_list;
pub mod test_lua_utils;
pub mod test_lua_walk;
pub mod test_math_attr;
pub mod test_meta;
pub mod test_metadata_source_tracking;
pub mod test_nested_yaml_serialization;
pub mod test_ordered_list_formatting;
pub mod test_raw_json_roundtrip;
pub mod test_rawblock_to_config_value;
pub mod test_section_divs;
pub mod test_shortcode;
pub mod test_smart_typography_positions;
pub mod test_task_list;
pub mod test_template_integration;
pub mod test_trailing_linebreak_commonmark;
pub mod test_treesitter_coverage;
pub mod test_treesitter_refactoring;
pub mod test_unclosed_attr_specifier;
pub mod test_unicode_error_offsets;
pub mod test_unicode_whitespace;
pub mod test_warnings;
pub mod test_wasm_entrypoints;
pub mod test_whitespace_re_compile_once;
pub mod test_yaml_tag_regression;
pub mod test_yaml_to_config_value;
pub mod tiling_phase3_tests;

fn main() {}
