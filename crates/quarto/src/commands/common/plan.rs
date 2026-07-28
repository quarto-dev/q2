//! The declarative plan shared by `q2 create` and `q2 use`.
//!
//! A command's resolution step turns arguments (and, for `q2 use brand`,
//! a fetched source) into a [`FilePlan`]: the preconditions that must
//! hold, the files to write, and the in-place edits to make. Nothing in
//! here touches the filesystem — [`super::writer`] does that, once, for
//! both commands.

use std::path::PathBuf;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

/// Content of a planned file.
#[derive(Debug)]
pub enum FileContent {
    Text(String),
    Binary(Vec<u8>),
}

/// One file the command intends to write.
#[derive(Debug)]
pub struct PlannedFile {
    /// Path relative to the plan root.
    pub path: PathBuf,
    pub content: FileContent,
}

/// An in-place modification of a file that may already exist.
///
/// Every variant is append-only by construction. That is not a
/// limitation we ran into and worked around — it is the property that
/// makes editing a user's hand-written file safe: no existing byte is
/// ever rewritten, so comments, ordering, and formatting above the
/// insertion point survive untouched.
#[derive(Debug)]
pub enum PlannedEdit {
    /// Ensure each line in `lines` appears somewhere in the file,
    /// appending the ones that do not. Creates the file when absent.
    /// Order-insensitive and idempotent — used for `.gitignore`.
    EnsureLines {
        /// Path relative to the plan root.
        path: PathBuf,
        lines: Vec<String>,
    },
}

/// A path that must **not** exist for the plan to be executable,
/// carrying the message to show when it does.
///
/// Checked before anything is written — including under `--dry-run`, so
/// a dry run reports the same refusal a real run would.
#[derive(Debug)]
pub struct Precondition {
    /// Path relative to the plan root.
    pub path: PathBuf,
    pub title: String,
    pub problem: String,
}

/// The resolved intent of a command invocation: where to write, what
/// must not already be there, what to write, and whether to actually
/// write it.
#[derive(Debug)]
pub struct FilePlan {
    /// Absolute root directory that every relative path is joined to.
    pub root: PathBuf,
    /// The root as the user wrote it, for messages and hints.
    pub root_display: String,
    /// Paths that must not exist. Checked first, before any write.
    pub preconditions: Vec<Precondition>,
    pub files: Vec<PlannedFile>,
    pub edits: Vec<PlannedEdit>,
    /// When set, the writer computes the full plan (including
    /// precondition failures) but writes nothing.
    pub dry_run: bool,
}

impl FilePlan {
    /// A plan with no preconditions and no edits — the common shape.
    pub fn new(
        root: PathBuf,
        root_display: String,
        files: Vec<PlannedFile>,
        dry_run: bool,
    ) -> Self {
        Self {
            root,
            root_display,
            preconditions: Vec::new(),
            files,
            edits: Vec::new(),
            dry_run,
        }
    }

    pub fn with_preconditions(mut self, preconditions: Vec<Precondition>) -> Self {
        self.preconditions = preconditions;
        self
    }

    pub fn with_edits(mut self, edits: Vec<PlannedEdit>) -> Self {
        self.edits = edits;
        self
    }
}

/// A resolved plan plus any non-fatal diagnostics produced while
/// resolving (e.g. a defaulted title).
#[derive(Debug)]
pub struct ResolvedPlan {
    pub plan: FilePlan,
    pub warnings: Vec<DiagnosticMessage>,
}

/// A command failure, carried as a structured diagnostic so the human
/// path (pretty text) and the JSON path (wire shape) render the same
/// content.
#[derive(Debug)]
pub struct CommandFailure(pub DiagnosticMessage);

impl CommandFailure {
    pub fn new(title: impl Into<String>, problem: impl Into<String>) -> Self {
        Self(
            DiagnosticMessageBuilder::error(title)
                .problem(problem.into())
                .build(),
        )
    }

    /// The user cancelled an interactive prompt (Esc / Ctrl-C).
    pub fn cancelled() -> Self {
        Self::new("Cancelled", "No files were written.")
    }
}
