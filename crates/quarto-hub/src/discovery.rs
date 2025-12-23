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

    /// Config files (e.g., `_quarto.yml`, paths relative to project root)
    pub config_files: Vec<PathBuf>,
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
                // Check for config files first (by name)
                if let Some(file_name) = path.file_name() {
                    if file_name == "_quarto.yml" || file_name == "_quarto.yaml" {
                        if let Ok(relative) = path.strip_prefix(project_root) {
                            debug!(?relative, "Discovered config file");
                            files.config_files.push(relative.to_path_buf());
                        }
                        continue;
                    }
                }

                // Check for .qmd files (by extension)
                if let Some(ext) = path.extension() {
                    if ext == "qmd" {
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
        files.config_files.sort();

        files
    }

    /// Returns the total number of discovered files.
    pub fn total_count(&self) -> usize {
        self.qmd_files.len() + self.config_files.len()
    }

    /// Returns an iterator over all discovered file paths.
    pub fn all_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.config_files.iter().chain(self.qmd_files.iter())
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

    #[test]
    fn test_discover_config_files() {
        let temp = TempDir::new().unwrap();

        // Create _quarto.yml at project root
        fs::write(temp.path().join("_quarto.yml"), "project:\n  type: website").unwrap();
        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();

        // Create a subdirectory with _quarto.yaml (alternate extension)
        fs::create_dir(temp.path().join("subproject")).unwrap();
        fs::write(
            temp.path().join("subproject/_quarto.yaml"),
            "project:\n  type: book",
        )
        .unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert_eq!(files.config_files.len(), 2);
        assert!(files.config_files.contains(&PathBuf::from("_quarto.yml")));
        assert!(files.config_files.contains(&PathBuf::from("subproject/_quarto.yaml")));
        assert_eq!(files.qmd_files.len(), 1);
        assert_eq!(files.total_count(), 3);
    }

    #[test]
    fn test_all_files_iterator() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("_quarto.yml"), "project:\n  type: website").unwrap();
        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();
        fs::write(temp.path().join("about.qmd"), "# About").unwrap();

        let files = ProjectFiles::discover(temp.path());
        let all: Vec<_> = files.all_files().collect();

        assert_eq!(all.len(), 3);
        // Config files come first
        assert_eq!(all[0], &PathBuf::from("_quarto.yml"));
    }
}
