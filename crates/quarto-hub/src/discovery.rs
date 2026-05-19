//! Project file discovery
//!
//! Walks the project directory to find `.qmd` files, config files, and binary resources.

use std::path::{Path, PathBuf};

use tracing::debug;
use walkdir::WalkDir;

use crate::resource::is_binary_extension;

/// Discovered files in a Quarto project.
#[derive(Debug, Default)]
pub struct ProjectFiles {
    /// All discovered `.qmd` files (paths relative to project root)
    pub qmd_files: Vec<PathBuf>,

    /// Config files (e.g., `_quarto.yml`, paths relative to project root)
    pub config_files: Vec<PathBuf>,

    /// Binary resource files (images, PDFs, etc., paths relative to project root)
    pub binary_files: Vec<PathBuf>,

    /// All text files discovered inside `_extensions/` directories (paths relative to project root)
    pub extension_files: Vec<PathBuf>,

    /// Other text source files (currently `.tsx`, paths relative to project root)
    pub source_files: Vec<PathBuf>,
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
                // Files inside _extensions/ are included wholesale (Lua filters,
                // JSON manifests, YAML metadata, etc. are all needed at render time).
                // Binary files within _extensions/ go into binary_files; everything
                // else is treated as text and goes into extension_files.
                let in_extensions = path.components().any(|c| c.as_os_str() == "_extensions");

                if in_extensions {
                    if let Ok(relative) = path.strip_prefix(project_root) {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if is_binary_extension(ext) {
                            debug!(?relative, "Discovered extension binary file");
                            files.binary_files.push(relative.to_path_buf());
                        } else {
                            debug!(?relative, "Discovered extension text file");
                            files.extension_files.push(relative.to_path_buf());
                        }
                    }
                    continue;
                }

                // Check for config files first (by name)
                if let Some(file_name) = path.file_name()
                    && (file_name == "_quarto.yml"
                        || file_name == "_quarto.yaml"
                        || file_name == "_metadata.yml"
                        || file_name == "_metadata.yaml")
                {
                    if let Ok(relative) = path.strip_prefix(project_root) {
                        debug!(?relative, "Discovered config file");
                        files.config_files.push(relative.to_path_buf());
                    }
                    continue;
                }

                // Get file extension for further checks
                let ext = path.extension().and_then(|e| e.to_str());

                // Check for .qmd files
                if ext == Some("qmd") {
                    if let Ok(relative) = path.strip_prefix(project_root) {
                        debug!(?relative, "Discovered .qmd file");
                        files.qmd_files.push(relative.to_path_buf());
                    }
                    continue;
                }

                // Check for .tsx source files
                if ext == Some("tsx") {
                    if let Ok(relative) = path.strip_prefix(project_root) {
                        debug!(?relative, "Discovered .tsx source file");
                        files.source_files.push(relative.to_path_buf());
                    }
                    continue;
                }

                // Check for binary resource files (images, PDFs, etc.)
                if let Some(ext_str) = ext {
                    if is_binary_extension(ext_str) {
                        if let Ok(relative) = path.strip_prefix(project_root) {
                            debug!(?relative, "Discovered binary file");
                            files.binary_files.push(relative.to_path_buf());
                        }
                    }
                }
            }
        }

        // Sort for deterministic ordering
        files.qmd_files.sort();
        files.config_files.sort();
        files.binary_files.sort();
        files.extension_files.sort();
        files.source_files.sort();

        files
    }

    /// Build a `ProjectFiles` whose only entry is the supplied
    /// relative path, treated as a `.qmd` file. Used by `q2 preview`
    /// when invoked on a single file with no `_quarto.yml` ancestor
    /// (bd-tnm3k): the parent directory becomes `project_root`, but
    /// discovery must not walk it — that would pull in arbitrary
    /// sibling files (think `~/Downloads/`).
    ///
    /// The caller is responsible for supplying a *non-empty* relative
    /// path. We do not validate, but downstream
    /// `reconcile_files_with_index` produces a `project_root + '/'`
    /// path that trips ENOTDIR when `read_to_string` is called on it
    /// if the relative path is empty.
    pub fn single_file(relative: PathBuf) -> Self {
        Self {
            qmd_files: vec![relative],
            ..Self::default()
        }
    }

    /// Returns the total number of discovered files.
    pub fn total_count(&self) -> usize {
        self.qmd_files.len()
            + self.config_files.len()
            + self.binary_files.len()
            + self.extension_files.len()
            + self.source_files.len()
    }

    /// Returns the count of text files (config + qmd + extension text + source files).
    pub fn text_file_count(&self) -> usize {
        self.qmd_files.len()
            + self.config_files.len()
            + self.extension_files.len()
            + self.source_files.len()
    }

    /// Returns an iterator over all discovered file paths.
    pub fn all_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.config_files
            .iter()
            .chain(self.qmd_files.iter())
            .chain(self.extension_files.iter())
            .chain(self.source_files.iter())
            .chain(self.binary_files.iter())
    }

    /// Returns an iterator over text files only (config + qmd + extension text + source files).
    pub fn text_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.config_files
            .iter()
            .chain(self.qmd_files.iter())
            .chain(self.extension_files.iter())
            .chain(self.source_files.iter())
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
        assert!(
            files
                .config_files
                .contains(&PathBuf::from("subproject/_quarto.yaml"))
        );
        assert_eq!(files.qmd_files.len(), 1);
        assert_eq!(files.total_count(), 3);
    }

    #[test]
    fn test_discover_metadata_files() {
        let temp = TempDir::new().unwrap();

        // Create project structure with _metadata.yml in a subdirectory
        fs::write(temp.path().join("_quarto.yml"), "project:\n  type: website").unwrap();
        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();
        fs::create_dir(temp.path().join("chapters")).unwrap();
        fs::write(
            temp.path().join("chapters/_metadata.yml"),
            "author: Directory Author",
        )
        .unwrap();
        fs::write(temp.path().join("chapters/chapter1.qmd"), "# Chapter 1").unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert_eq!(files.config_files.len(), 2);
        assert!(files.config_files.contains(&PathBuf::from("_quarto.yml")));
        assert!(
            files
                .config_files
                .contains(&PathBuf::from("chapters/_metadata.yml"))
        );
        assert_eq!(files.qmd_files.len(), 2);
    }

    #[test]
    fn test_discover_metadata_yaml_extension() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("_quarto.yml"), "project:\n  type: website").unwrap();
        fs::create_dir(temp.path().join("docs")).unwrap();
        fs::write(temp.path().join("docs/_metadata.yaml"), "toc: true").unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert!(
            files
                .config_files
                .contains(&PathBuf::from("docs/_metadata.yaml"))
        );
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

    #[test]
    fn test_discover_binary_files() {
        let temp = TempDir::new().unwrap();

        // Create some binary files
        fs::write(temp.path().join("logo.png"), &[0x89, 0x50, 0x4E, 0x47]).unwrap();
        fs::write(temp.path().join("photo.jpg"), &[0xFF, 0xD8, 0xFF]).unwrap();
        fs::write(temp.path().join("document.pdf"), b"PDF content").unwrap();

        // Create a subdirectory with more binary files
        fs::create_dir(temp.path().join("images")).unwrap();
        fs::write(temp.path().join("images/diagram.svg"), "<svg></svg>").unwrap();
        fs::write(temp.path().join("images/icon.webp"), b"webp data").unwrap();

        // Also create a qmd file to ensure we're not breaking existing discovery
        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert_eq!(files.binary_files.len(), 5);
        assert!(files.binary_files.contains(&PathBuf::from("logo.png")));
        assert!(files.binary_files.contains(&PathBuf::from("photo.jpg")));
        assert!(files.binary_files.contains(&PathBuf::from("document.pdf")));
        assert!(
            files
                .binary_files
                .contains(&PathBuf::from("images/diagram.svg"))
        );
        assert!(
            files
                .binary_files
                .contains(&PathBuf::from("images/icon.webp"))
        );

        // qmd file should still be discovered
        assert_eq!(files.qmd_files.len(), 1);
        assert!(files.qmd_files.contains(&PathBuf::from("index.qmd")));

        // Total count includes all files
        assert_eq!(files.total_count(), 6);
        assert_eq!(files.text_file_count(), 1);
    }

    #[test]
    fn test_text_files_iterator() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("_quarto.yml"), "project:\n  type: website").unwrap();
        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();
        fs::write(temp.path().join("logo.png"), &[0x89, 0x50, 0x4E, 0x47]).unwrap();

        let files = ProjectFiles::discover(temp.path());

        // all_files includes binary
        let all: Vec<_> = files.all_files().collect();
        assert_eq!(all.len(), 3);

        // text_files excludes binary
        let text: Vec<_> = files.text_files().collect();
        assert_eq!(text.len(), 2);
        assert!(text.contains(&&PathBuf::from("_quarto.yml")));
        assert!(text.contains(&&PathBuf::from("index.qmd")));
    }

    #[test]
    fn test_discover_tsx_files() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();
        fs::write(
            temp.path().join("App.tsx"),
            "export const App = () => null;",
        )
        .unwrap();
        fs::create_dir(temp.path().join("components")).unwrap();
        fs::write(
            temp.path().join("components/Widget.tsx"),
            "export const Widget = () => null;",
        )
        .unwrap();

        // .ts (not .tsx) should NOT be picked up
        fs::write(temp.path().join("util.ts"), "export const x = 1;").unwrap();

        let files = ProjectFiles::discover(temp.path());

        assert_eq!(files.source_files.len(), 2);
        assert!(files.source_files.contains(&PathBuf::from("App.tsx")));
        assert!(
            files
                .source_files
                .contains(&PathBuf::from("components/Widget.tsx"))
        );

        // .tsx flows through text_files() so it gets synced as text
        let text: Vec<_> = files.text_files().collect();
        assert!(text.contains(&&PathBuf::from("App.tsx")));
        assert!(text.contains(&&PathBuf::from("components/Widget.tsx")));

        // .ts is silently dropped (only .tsx is included)
        assert!(!files.source_files.contains(&PathBuf::from("util.ts")));
    }

    /// bd-tnm3k: `q2 preview` on a single `.qmd` file with no
    /// `_quarto.yml` ancestor needs a `ProjectFiles` that contains
    /// exactly that one file with a *non-empty* relative path. The
    /// pre-fix walk-based discovery produced an empty relative path
    /// (because `path.strip_prefix(self)` returns `""`), which made
    /// `project_root.join(rel)` append a trailing slash and trip
    /// ENOTDIR downstream. The explicit single-file constructor
    /// short-circuits the walk and guarantees a sane relative path.
    #[test]
    fn single_file_constructor_yields_one_qmd_with_nonempty_path() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path();
        // Sibling file that must NOT be included.
        fs::write(project_root.join("sibling.qmd"), "# sibling").unwrap();
        let rel = PathBuf::from("doc.qmd");
        fs::write(project_root.join(&rel), "# doc").unwrap();

        let files = ProjectFiles::single_file(rel.clone());

        assert_eq!(files.qmd_files, vec![rel.clone()]);
        assert!(files.config_files.is_empty());
        assert!(files.binary_files.is_empty());
        assert!(files.extension_files.is_empty());
        assert!(files.source_files.is_empty());
        // Critically: the relative path is non-empty.
        assert!(!files.qmd_files[0].as_os_str().is_empty());
    }

    #[test]
    fn test_discover_extensions_directory() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();

        // Create a typical _extensions/ layout
        let ext_dir = temp.path().join("_extensions").join("lipsum");
        fs::create_dir_all(&ext_dir).unwrap();
        fs::write(ext_dir.join("_extension.yml"), "title: Lipsum").unwrap();
        fs::write(ext_dir.join("lipsum.lua"), "function Str(el) end").unwrap();
        fs::write(ext_dir.join("data.json"), "{}").unwrap();
        fs::write(ext_dir.join("logo.png"), &[0x89, 0x50, 0x4E, 0x47]).unwrap();

        let files = ProjectFiles::discover(temp.path());

        // Text files in _extensions/ go into extension_files
        assert_eq!(files.extension_files.len(), 3);
        assert!(
            files
                .extension_files
                .contains(&PathBuf::from("_extensions/lipsum/_extension.yml"))
        );
        assert!(
            files
                .extension_files
                .contains(&PathBuf::from("_extensions/lipsum/lipsum.lua"))
        );
        assert!(
            files
                .extension_files
                .contains(&PathBuf::from("_extensions/lipsum/data.json"))
        );

        // Binary files in _extensions/ still go into binary_files
        assert!(
            files
                .binary_files
                .contains(&PathBuf::from("_extensions/lipsum/logo.png"))
        );

        // Extension files appear in text_files() and all_files()
        let text: Vec<_> = files.text_files().collect();
        assert!(text.contains(&&PathBuf::from("_extensions/lipsum/lipsum.lua")));

        assert_eq!(files.total_count(), 5); // 1 qmd + 3 extension text + 1 extension binary
    }
}
