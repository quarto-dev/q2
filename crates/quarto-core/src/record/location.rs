//! Where a document's `.quarto` execution record lives (RIU-2).
//!
//! One function derives the path, and both the writer (`EngineExecutionStage`, for
//! `execute: {record: true}`) and the `q2` record source read it, so the reader cannot drift
//! from the writer.

use std::path::{Path, PathBuf};

/// The document's path relative to the project directory, with forward slashes (the record's
/// `quarto_source`; a standalone document is relative to its own directory). A path outside the
/// project keeps its file name.
pub fn project_relative_source(project_dir: &Path, path: &Path) -> String {
    let rel = path
        .strip_prefix(project_dir)
        .unwrap_or_else(|_| path.file_name().map_or(path, Path::new));
    quarto_util::to_forward_slashes(rel)
}

/// `<project_dir>/.quarto/execution-records/<document path with the extension replaced by
/// .ipynb>`: project-relative, or beside a standalone document.
pub fn execution_record_path(project_dir: &Path, doc_path: &Path) -> PathBuf {
    let rel = PathBuf::from(project_relative_source(project_dir, doc_path)).with_extension("ipynb");
    project_dir
        .join(".quarto")
        .join("execution-records")
        .join(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_in_the_project_root() {
        let dir = Path::new("proj");
        assert_eq!(
            execution_record_path(dir, &dir.join("doc.qmd")),
            dir.join(".quarto")
                .join("execution-records")
                .join("doc.ipynb")
        );
    }

    #[test]
    fn a_nested_document_keeps_its_directories() {
        let dir = Path::new("proj");
        assert_eq!(
            execution_record_path(dir, &dir.join("posts").join("a.qmd")),
            dir.join(".quarto")
                .join("execution-records")
                .join("posts")
                .join("a.ipynb")
        );
        assert_eq!(
            project_relative_source(dir, &dir.join("posts").join("a.qmd")),
            "posts/a.qmd"
        );
    }

    /// A path outside the project keeps only its file name.
    #[test]
    fn a_document_outside_the_project_keeps_its_file_name() {
        assert_eq!(
            execution_record_path(
                Path::new("proj"),
                Path::new("elsewhere").join("x.qmd").as_path()
            ),
            Path::new("proj")
                .join(".quarto")
                .join("execution-records")
                .join("x.ipynb")
        );
    }
}
