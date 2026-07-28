//! The single disk writer for [`FilePlan`]s.
//!
//! Semantics (originally Q1's `project-create.ts`, now shared by
//! `q2 create` and `q2 use`):
//!
//! - **preconditions first** — a plan whose "must not exist" paths are
//!   occupied fails before anything is written;
//! - writing into an existing directory is allowed (merge);
//! - individual files that already exist are **skipped, never
//!   overwritten**;
//! - edits are append-only ([`PlannedEdit`]), so no existing byte in a
//!   user's file is ever rewritten.
//!
//! Dry-run computes the identical result — including precondition
//! failures — without touching the filesystem. That is the whole reason
//! the plan is data rather than a closure: there is no second code path
//! to keep in sync.

use std::path::{Path, PathBuf};

use super::plan::{CommandFailure, FileContent, FilePlan, PlannedEdit};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Debug)]
pub struct ExecutedFile {
    /// Path relative to the plan root.
    pub path: PathBuf,
    pub action: FileAction,
}

fn io_failure(what: &str, path: &Path, e: std::io::Error) -> CommandFailure {
    CommandFailure::new(
        format!("Failed to {what}"),
        format!("{}: {e}", path.display()),
    )
}

pub fn execute_plan(plan: &FilePlan) -> Result<Vec<ExecutedFile>, CommandFailure> {
    for pre in &plan.preconditions {
        if plan.root.join(&pre.path).exists() {
            return Err(CommandFailure::new(pre.title.clone(), pre.problem.clone()));
        }
    }

    let mut results = Vec::with_capacity(plan.files.len() + plan.edits.len());

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
                    .map_err(|e| io_failure("create directory", parent, e))?;
            }
            let write_result = match &file.content {
                FileContent::Text(s) => std::fs::write(&target, s),
                FileContent::Binary(b) => std::fs::write(&target, b),
            };
            write_result.map_err(|e| io_failure("write file", &target, e))?;
        }
        results.push(ExecutedFile {
            path: file.path.clone(),
            action,
        });
    }

    for edit in &plan.edits {
        results.push(apply_edit(plan, edit)?);
    }

    Ok(results)
}

fn apply_edit(plan: &FilePlan, edit: &PlannedEdit) -> Result<ExecutedFile, CommandFailure> {
    match edit {
        PlannedEdit::EnsureLines { path, lines } => ensure_lines(plan, path, lines),
    }
}

/// Ensure every line in `lines` is present, appending the missing ones.
/// Creates the file when absent.
fn ensure_lines(
    plan: &FilePlan,
    rel: &PathBuf,
    lines: &[String],
) -> Result<ExecutedFile, CommandFailure> {
    let path = plan.root.join(rel);

    if !path.exists() {
        if !plan.dry_run {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| io_failure("create directory", parent, e))?;
            }
            let content = lines.join("\n") + "\n";
            std::fs::write(&path, content)
                .map_err(|e| io_failure(&format!("write {}", rel.display()), &path, e))?;
        }
        return Ok(ExecutedFile {
            path: rel.clone(),
            action: FileAction::Created,
        });
    }

    let existing = std::fs::read_to_string(&path)
        .map_err(|e| io_failure(&format!("read {}", rel.display()), &path, e))?;
    let missing: Vec<&String> = lines
        .iter()
        .filter(|entry| !existing.lines().any(|line| line.trim() == entry.as_str()))
        .collect();

    if missing.is_empty() {
        return Ok(ExecutedFile {
            path: rel.clone(),
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
        std::fs::write(&path, updated)
            .map_err(|e| io_failure(&format!("write {}", rel.display()), &path, e))?;
    }
    Ok(ExecutedFile {
        path: rel.clone(),
        action: FileAction::Updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::common::plan::{PlannedFile, Precondition};
    use tempfile::TempDir;

    fn plan_in(dir: &TempDir, dry_run: bool) -> FilePlan {
        FilePlan::new(
            dir.path().to_path_buf(),
            dir.path().display().to_string(),
            Vec::new(),
            dry_run,
        )
    }

    #[test]
    fn precondition_failure_blocks_all_writes() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_brand.yml"), "logo:\n").unwrap();

        let plan = FilePlan {
            files: vec![PlannedFile {
                path: PathBuf::from("new.txt"),
                content: FileContent::Text("x".into()),
            }],
            ..plan_in(&dir, false)
        }
        .with_preconditions(vec![Precondition {
            path: PathBuf::from("_brand.yml"),
            title: "Brand already configured".into(),
            problem: "remove it first".into(),
        }]);

        let err = execute_plan(&plan).unwrap_err();
        assert!(err.0.to_text(None).contains("Brand already configured"));
        assert!(
            !dir.path().join("new.txt").exists(),
            "no file may be written once a precondition fails"
        );
    }

    #[test]
    fn precondition_is_checked_under_dry_run_too() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_brand.yml"), "logo:\n").unwrap();
        let plan = plan_in(&dir, true).with_preconditions(vec![Precondition {
            path: PathBuf::from("_brand.yml"),
            title: "Brand already configured".into(),
            problem: "remove it first".into(),
        }]);
        assert!(execute_plan(&plan).is_err());
    }

    #[test]
    fn dry_run_writes_nothing() {
        let dir = TempDir::new().unwrap();

        let plan = FilePlan {
            files: vec![PlannedFile {
                path: PathBuf::from("_brand.yml"),
                content: FileContent::Text("color:\n".into()),
            }],
            ..plan_in(&dir, true)
        }
        .with_edits(vec![PlannedEdit::EnsureLines {
            path: PathBuf::from(".gitignore"),
            lines: vec!["/.quarto/".into()],
        }]);
        let executed = execute_plan(&plan).unwrap();

        // The plan is reported in full...
        assert_eq!(executed.len(), 2);
        assert_eq!(executed[0].action, FileAction::Created);
        assert_eq!(executed[1].action, FileAction::Created);
        // ...and nothing was written.
        assert!(!dir.path().join("_brand.yml").exists());
        assert!(!dir.path().join(".gitignore").exists());
    }

    #[test]
    fn ensure_lines_appends_only_missing_entries() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "/.quarto/\n").unwrap();

        let plan = plan_in(&dir, false).with_edits(vec![PlannedEdit::EnsureLines {
            path: PathBuf::from(".gitignore"),
            lines: vec!["/.quarto/".into(), "/_site/".into()],
        }]);
        let executed = execute_plan(&plan).unwrap();

        assert_eq!(executed[0].action, FileAction::Updated);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
            "/.quarto/\n/_site/\n"
        );
    }

    #[test]
    fn ensure_lines_is_idempotent() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "/.quarto/\n").unwrap();
        let plan = plan_in(&dir, false).with_edits(vec![PlannedEdit::EnsureLines {
            path: PathBuf::from(".gitignore"),
            lines: vec!["/.quarto/".into()],
        }]);
        let executed = execute_plan(&plan).unwrap();
        assert_eq!(executed[0].action, FileAction::SkippedExisting);
    }

    #[test]
    fn existing_files_are_skipped_never_overwritten() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.qmd"), "MINE\n").unwrap();

        let plan = FilePlan {
            files: vec![PlannedFile {
                path: PathBuf::from("index.qmd"),
                content: FileContent::Text("SCAFFOLD\n".into()),
            }],
            ..plan_in(&dir, false)
        };
        let executed = execute_plan(&plan).unwrap();

        assert_eq!(executed[0].action, FileAction::SkippedExisting);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("index.qmd")).unwrap(),
            "MINE\n"
        );
    }
}
