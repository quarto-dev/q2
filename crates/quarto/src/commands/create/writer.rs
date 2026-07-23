//! Disk writer for create plans.
//!
//! Q1-parity directory semantics (`project-create.ts`):
//! - creating into an existing directory is allowed (merge);
//! - a directory that already contains `_quarto.yml`/`_quarto.yaml`
//!   is a hard error, before anything is written;
//! - individual files that already exist are skipped, never
//!   overwritten;
//! - `.gitignore` entries are ensured: file created when absent,
//!   appended when present-but-missing-entries, untouched otherwise.
//!
//! Dry-run computes the identical plan (including the hard error)
//! without touching the filesystem.

use std::path::{Path, PathBuf};

use super::artifact::{CreateFailure, CreatePlan, FileContent};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Created,
    SkippedExisting,
    Updated,
}

impl FileAction {
    /// Wire string for JSON results (and stable vocabulary for the
    /// human output).
    pub fn as_str(self) -> &'static str {
        match self {
            FileAction::Created => "created",
            FileAction::SkippedExisting => "skipped-existing",
            FileAction::Updated => "updated",
        }
    }
}

pub struct ExecutedFile {
    /// Path relative to the plan root.
    pub path: PathBuf,
    pub action: FileAction,
}

fn io_failure(what: &str, path: &Path, e: std::io::Error) -> CreateFailure {
    CreateFailure::new(
        format!("Failed to {what}"),
        format!("{}: {e}", path.display()),
    )
}

pub fn execute_plan(plan: &CreatePlan) -> Result<Vec<ExecutedFile>, CreateFailure> {
    for config in ["_quarto.yml", "_quarto.yaml"] {
        if plan.root.join(config).exists() {
            return Err(CreateFailure::new(
                format!(
                    "Directory '{}' already contains a Quarto project",
                    plan.root_display
                ),
                format!("Found {config}. Choose a different directory."),
            ));
        }
    }

    let mut results = Vec::with_capacity(plan.files.len() + 1);

    for file in &plan.files {
        let target = plan.root.join(&file.path);
        let action = if target.exists() {
            FileAction::SkippedExisting
        } else {
            FileAction::Created
        };
        if action == FileAction::Created && !plan.dry_run {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| io_failure("create project directory", parent, e))?;
            }
            let write_result = match &file.content {
                FileContent::Text(s) => std::fs::write(&target, s),
                FileContent::Binary(b) => std::fs::write(&target, b),
            };
            write_result.map_err(|e| io_failure("write project file", &target, e))?;
        }
        results.push(ExecutedFile {
            path: file.path.clone(),
            action,
        });
    }

    if !plan.gitignore_entries.is_empty() {
        results.push(ensure_gitignore(plan)?);
    }

    Ok(results)
}

fn ensure_gitignore(plan: &CreatePlan) -> Result<ExecutedFile, CreateFailure> {
    let path = plan.root.join(".gitignore");
    let rel = PathBuf::from(".gitignore");

    if !path.exists() {
        if !plan.dry_run {
            std::fs::create_dir_all(&plan.root)
                .map_err(|e| io_failure("create project directory", &plan.root, e))?;
            let content = plan.gitignore_entries.join("\n") + "\n";
            std::fs::write(&path, content).map_err(|e| io_failure("write .gitignore", &path, e))?;
        }
        return Ok(ExecutedFile {
            path: rel,
            action: FileAction::Created,
        });
    }

    let existing =
        std::fs::read_to_string(&path).map_err(|e| io_failure("read .gitignore", &path, e))?;
    let missing: Vec<&str> = plan
        .gitignore_entries
        .iter()
        .copied()
        .filter(|entry| !existing.lines().any(|line| line.trim() == *entry))
        .collect();

    if missing.is_empty() {
        return Ok(ExecutedFile {
            path: rel,
            action: FileAction::SkippedExisting,
        });
    }

    if !plan.dry_run {
        let mut updated = existing;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        for entry in &missing {
            updated.push_str(entry);
            updated.push('\n');
        }
        std::fs::write(&path, updated).map_err(|e| io_failure("write .gitignore", &path, e))?;
    }
    Ok(ExecutedFile {
        path: rel,
        action: FileAction::Updated,
    })
}
