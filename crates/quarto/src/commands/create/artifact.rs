//! Artifact-provider seam for `q2 create`.
//!
//! Rust port of Quarto 1's `ArtifactCreator` interface
//! (`external-sources/quarto-cli/src/command/create/cmd-types.ts`),
//! expressed as three resolution paths over one plan/writer engine:
//! positional CLI args, a JSON directive payload, and interactive
//! gap-filling prompts (bd-hh1erpfx). Each artifact type (project
//! today; extension later) implements [`ArtifactProvider`]; the
//! command layer stays agnostic to what is being created.
//!
//! The plan/writer/prompter types themselves live in
//! [`crate::commands::common`], shared with `q2 use` (bd-1vlw8).

use std::path::Path;

use serde::Serialize;

use crate::commands::common::plan::{CommandFailure, ResolvedPlan};
use crate::commands::common::prompter::Prompter;

/// One choice row for `--list`. Kept to the fields downstream tools
/// need to populate a picker; the `id` is what the directive's
/// `choice` field accepts.
#[derive(Serialize)]
pub struct ChoiceListing {
    pub id: String,
    pub name: String,
    pub description: String,
    pub implemented: bool,
}

pub trait ArtifactProvider {
    /// CLI token and JSON-directive `artifact` tag (e.g. `"project"`).
    fn type_id(&self) -> &'static str;

    /// Human-facing name for listings.
    fn display_name(&self) -> &'static str;

    /// Resolve positional CLI arguments (everything after the artifact
    /// type token) into a create plan.
    fn resolve_cli(
        &self,
        args: &[String],
        cwd: &Path,
        dry_run: bool,
    ) -> Result<ResolvedPlan, CommandFailure>;

    /// Resolve a JSON directive payload (the directive object minus
    /// its `artifact` tag). Implementations must reject unknown
    /// fields so tooling typos fail loudly.
    fn resolve_json(
        &self,
        payload: serde_json::Value,
        cwd: &Path,
        dry_run: bool,
    ) -> Result<ResolvedPlan, CommandFailure>;

    /// Resolve with interactive gap-filling: consume whatever
    /// positional args were provided and prompt (via `prompter`) only
    /// for what is missing. With a complete argument list this must
    /// behave identically to [`Self::resolve_cli`] — except that a
    /// prompted-and-accepted default title is explicit consent, so no
    /// defaulted-title warning is emitted on this path.
    fn resolve_interactive(
        &self,
        args: &[String],
        cwd: &Path,
        dry_run: bool,
        prompter: &mut dyn Prompter,
    ) -> Result<ResolvedPlan, CommandFailure>;

    /// Choices offered by this artifact type, for `--list`.
    fn choices(&self) -> Vec<ChoiceListing>;
}

/// The artifact registry. Order is the display order in `--list`.
pub fn providers() -> Vec<Box<dyn ArtifactProvider>> {
    vec![Box::new(super::project::ProjectProvider)]
}

pub fn find_provider<'a>(
    providers: &'a [Box<dyn ArtifactProvider>],
    type_id: &str,
) -> Option<&'a dyn ArtifactProvider> {
    providers
        .iter()
        .find(|p| p.type_id() == type_id)
        .map(|p| p.as_ref())
}

/// Comma-separated list of registered artifact type ids, for error
/// messages and the missing-type hint.
pub fn type_ids(providers: &[Box<dyn ArtifactProvider>]) -> String {
    providers
        .iter()
        .map(|p| p.type_id())
        .collect::<Vec<_>>()
        .join(", ")
}
