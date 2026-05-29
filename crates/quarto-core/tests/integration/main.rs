//! quarto-core integration test binary.
//! See bd-xvdop / claude-notes/plans/2026-05-28-integration-test-consolidation.md.

pub mod artifact_scoping_pipeline;
pub mod attribution_baseline_snapshot;
pub mod attribution_chain_resolution;
pub mod attribution_cli;
pub mod attribution_generate;
pub mod attribution_gitblame;
pub mod attribution_render;
pub mod attribution_types;
pub mod attribution_viewer;
pub mod attribution_wasm_invariant;
pub mod bootstrap_js_pipeline;
pub mod brand_render;
pub mod crossref_fixtures;
pub mod document_profile_pipeline;
pub mod engine_merge;
pub mod include_resolve_pipeline;
pub mod incremental_rebuild;
pub mod jupyter_integration;
pub mod link_rewriting_pipeline;
pub mod listing_pipeline;
pub mod math_mode_pipeline;
pub mod metadata_path_resolution;
pub mod navbar_footer_pipeline;
pub mod navigation_e2e;
pub mod navigation_merge;
pub mod page_navigation_pipeline;
pub mod project_pipeline;
pub mod project_resources;
pub mod render_page_in_project;
pub mod render_preserves_source_files;
pub mod render_to_html_user_grammars;
pub mod replay_engine;
pub mod sidebar_pipeline;
pub mod website_post_render;

fn main() {}
