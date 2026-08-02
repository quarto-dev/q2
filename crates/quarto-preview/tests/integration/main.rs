//! quarto-preview integration test binary.
//! See bd-xvdop / claude-notes/plans/2026-05-28-integration-test-consolidation.md.

pub mod boot;
pub mod cache_hit;
pub mod config_endpoint;
pub mod diagnostics_capture_failure;
pub mod diagnostics_endpoint;
pub mod eager_capture;
pub mod render_scripts_boot;
pub mod smoke;
pub mod staleness;

fn main() {}
