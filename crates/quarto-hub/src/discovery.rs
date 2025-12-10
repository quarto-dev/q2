//! Project file discovery
//!
//! Walks the project directory to find `.qmd` files and other editable documents.

use std::path::{Path, PathBuf};

use tracing::debug;
use walkdir::WalkDir;

/// Discovered files in a Quarto project.
#[derive(Debug, Default)]
pub struct ProjectFiles {
    /// All discovered `.qmd` files (paths relative to project root)
    pub qmd_files: Vec<PathBuf>,
}

impl ProjectFiles {
    /// Discover all editable files in a Quarto project.
    ///
    /// Walks the project directory, skipping:
    /// - Hidden directories (starting with `.`)
    /// - `node_modules`
    /// - `_site`, `_book`, and other output directories
    pub fn discover(project_root: &Path) -> Self {
        let mut files = ProjectFiles::default();

        let walker = WalkDir::new(project_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored(e));

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "qmd" {
                        // Store path relative to project root
                        if let Ok(relative) = path.strip_prefix(project_root) {
                            debug!(?relative, "Discovered .qmd file");
                            files.qmd_files.push(relative.to_path_buf());
                        }
                    }
                }
            }
        }

        // Sort for deterministic ordering
        files.qmd_files.sort();

        files
    }

    /// Returns the total number of discovered files.
    pub fn total_count(&self) -> usize {
        self.qmd_files.len()
    }
}

/// Check if a directory entry should be ignored during traversal.
fn is_ignored(entry: &walkdir::DirEntry) -> bool {
    // Never filter the root directory (depth 0)
    if entry.depth() == 0 {
        return false;
    }

    let name = entry.file_name().to_string_lossy();

    // Skip hidden directories (but not the root, which we already handled)
    if name.starts_with('.') && entry.file_type().is_dir() {
        return true;
    }

    // Skip common non-source directories
    matches!(
        name.as_ref(),
        "node_modules" | "_site" | "_book" | "_freeze" | "renv" | "venv" | "__pycache__" | "target"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_discover_qmd_files() {
        let temp = TempDir::new().unwrap();

        // Create some .qmd files
        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();
        fs::write(temp.path().join("about.qmd"), "# About").unwrap();

        // Create a subdirectory with more files
        fs::create_dir(temp.path().join("chapters")).unwrap();
        fs::write(temp.path().join("chapters/intro.qmd"), "# Intro").unwrap();

        // Create files that should be ignored
        fs::create_dir(temp.path().join(".quarto")).unwrap();
        fs::write(temp.path().join(".quarto/hidden.qmd"), "hidden").unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert_eq!(files.qmd_files.len(), 3);
        assert!(files.qmd_files.contains(&PathBuf::from("index.qmd")));
        assert!(files.qmd_files.contains(&PathBuf::from("about.qmd")));
        assert!(
            files
                .qmd_files
                .contains(&PathBuf::from("chapters/intro.qmd"))
        );
    }

    #[test]
    fn test_ignores_node_modules() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();
        fs::create_dir(temp.path().join("node_modules")).unwrap();
        fs::write(
            temp.path().join("node_modules/package.qmd"),
            "should be ignored",
        )
        .unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert_eq!(files.qmd_files.len(), 1);
        assert!(files.qmd_files.contains(&PathBuf::from("index.qmd")));
    }
}
