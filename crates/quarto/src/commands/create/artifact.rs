//! Artifact-provider seam for `q2 create`.
//!
//! Rust port of Quarto 1's `ArtifactCreator` interface
//! (`external-sources/quarto-cli/src/command/create/cmd-types.ts`),
//! expressed as three resolution paths over one plan/writer engine:
//! positional CLI args, a JSON directive payload, and interactive
//! gap-filling prompts (bd-hh1erpfx). Each artifact type (project
//! today; extension later) implements [`ArtifactProvider`]; the
//! command layer stays agnostic to what is being created.

use std::path::{Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use serde::Serialize;

/// Content of a planned scaffold file.
#[derive(Debug)]
pub enum FileContent {
    Text(String),
    Binary(Vec<u8>),
}

/// One file the create operation intends to write.
#[derive(Debug)]
pub struct PlannedFile {
    /// Path relative to the target directory.
    pub path: PathBuf,
    pub content: FileContent,
}

/// The resolved intent of a create invocation: where to write, what
/// to write, and whether to actually write it.
#[derive(Debug)]
pub struct CreatePlan {
    /// Absolute target directory.
    pub root: PathBuf,
    /// The directory as the user wrote it, for messages and hints.
    pub root_display: String,
    pub files: Vec<PlannedFile>,
    /// `.gitignore` entries to ensure in the target directory.
    pub gitignore_entries: Vec<&'static str>,
    /// When set, the writer computes the full file plan (including the
    /// existing-project error) but writes nothing.
    pub dry_run: bool,
}

/// A resolved create plus any non-fatal diagnostics produced while
/// resolving (e.g. a defaulted title).
#[derive(Debug)]
pub struct ResolvedCreate {
    pub plan: CreatePlan,
    pub warnings: Vec<DiagnosticMessage>,
}

/// A create failure, carried as a structured diagnostic so the human
/// path (pretty text) and the JSON path (wire shape) render the same
/// content.
#[derive(Debug)]
pub struct CreateFailure(pub DiagnosticMessage);

impl CreateFailure {
    pub fn new(title: impl Into<String>, problem: impl Into<String>) -> Self {
        Self(
            DiagnosticMessageBuilder::error(title)
                .problem(problem.into())
                .build(),
        )
    }

    /// The user cancelled an interactive prompt (Esc / Ctrl-C).
    pub fn cancelled() -> Self {
        Self::new("Create cancelled", "No files were written.")
    }
}

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
    ) -> Result<ResolvedCreate, CreateFailure>;

    /// Resolve a JSON directive payload (the directive object minus
    /// its `artifact` tag). Implementations must reject unknown
    /// fields so tooling typos fail loudly.
    fn resolve_json(
        &self,
        payload: serde_json::Value,
        cwd: &Path,
        dry_run: bool,
    ) -> Result<ResolvedCreate, CreateFailure>;

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
        prompter: &mut dyn super::prompter::Prompter,
    ) -> Result<ResolvedCreate, CreateFailure>;

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
