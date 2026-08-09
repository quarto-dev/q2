//! quarto integration test binary.
//! See bd-xvdop / claude-notes/plans/2026-05-28-integration-test-consolidation.md.

pub mod attribution_cli_e2e;
pub mod bootstrap_sh;
pub mod coalesced_diagnostics;
pub mod create;
pub mod extension_config_spans;
pub mod get_config_cli;
pub mod json_errors;
pub mod preview_cli;
pub mod render_cli_e2e;
pub mod render_exit_codes;
pub mod render_integration;
pub mod render_scripts_cli;
pub mod revealjs_cli;
pub mod smoke_all;
pub mod strict_mode;
pub mod trace_cli;
pub mod unknown_project_type;
pub mod use_brand;
pub mod version_cli;

fn main() {}
