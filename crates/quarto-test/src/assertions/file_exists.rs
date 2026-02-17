/*
 * quarto-test/src/assertions/file_exists.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * File and path existence assertions.
 */

//! `fileExists`, `pathDoesNotExist`, and `folderExists` assertion implementations.

use std::path::PathBuf;

use anyhow::bail;

use super::{Assertion, VerifyContext};

/// Where to resolve relative paths for file existence checks.
#[derive(Debug, Clone, Copy)]
enum FileExistsBase {
    /// Relative to the output directory (parent of output file).
    OutputDir,
    /// Relative to the support files directory ({stem}_files).
    SupportDir,
}

/// Assertion that verifies a file exists.
///
/// The path can be absolute, relative to the output directory (`outputPath`),
/// or relative to the support files directory (`supportPath`).
#[derive(Debug)]
pub struct FileExists {
    /// Path to check.
    path: String,
    /// Where to resolve relative paths.
    base: FileExistsBase,
}

impl FileExists {
    /// Create a FileExists assertion relative to the output directory.
    pub fn new(path: String) -> Self {
        Self {
            path,
            base: FileExistsBase::OutputDir,
        }
    }

    /// Create a FileExists assertion relative to the support files directory.
    pub fn new_support_path(path: String) -> Self {
        Self {
            path,
            base: FileExistsBase::SupportDir,
        }
    }

    /// Resolve the path relative to the appropriate base directory.
    fn resolve_path(&self, context: &VerifyContext) -> PathBuf {
        let path = PathBuf::from(&self.path);
        if path.is_absolute() {
            path
        } else {
            match self.base {
                FileExistsBase::OutputDir => {
                    // Relative to output directory
                    context
                        .output_path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join(&self.path)
                }
                FileExistsBase::SupportDir => {
                    // Relative to support files directory ({stem}_files)
                    let support_dir = context
                        .output_path
                        .with_extension("")
                        .to_string_lossy()
                        .to_string()
                        + "_files";
                    PathBuf::from(support_dir).join(&self.path)
                }
            }
        }
    }
}

impl Assertion for FileExists {
    fn name(&self) -> &str {
        "fileExists"
    }

    fn verify(&self, context: &VerifyContext) -> anyhow::Result<()> {
        // If rendering failed, we can't check for files
        if let Some(err) = &context.render_error {
            bail!(
                "Cannot check file existence: rendering failed with: {}",
                err
            );
        }

        let resolved = self.resolve_path(context);

        if !resolved.exists() {
            bail!("Expected file does not exist: {}", resolved.display());
        }

        if !resolved.is_file() {
            bail!(
                "Path exists but is not a file: {} (is a directory)",
                resolved.display()
            );
        }

        Ok(())
    }
}

/// Assertion that verifies a path does not exist.
///
/// The path can be absolute or relative to the output directory.
#[derive(Debug)]
pub struct PathDoesNotExist {
    /// Path to check (relative to output directory or absolute).
    path: String,
}

impl PathDoesNotExist {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    /// Resolve the path relative to the output directory.
    fn resolve_path(&self, context: &VerifyContext) -> PathBuf {
        let path = PathBuf::from(&self.path);
        if path.is_absolute() {
            path
        } else {
            // Relative to output directory
            context
                .output_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join(&self.path)
        }
    }
}

impl Assertion for PathDoesNotExist {
    fn name(&self) -> &str {
        "pathDoesNotExist"
    }

    fn verify(&self, context: &VerifyContext) -> anyhow::Result<()> {
        // Note: We allow checking non-existence even if rendering failed,
        // since a failed render might be expected not to produce certain files.

        let resolved = self.resolve_path(context);

        if resolved.exists() {
            let kind = if resolved.is_dir() {
                "directory"
            } else {
                "file"
            };
            bail!(
                "Path should not exist but it does: {} (is a {})",
                resolved.display(),
                kind
            );
        }

        Ok(())
    }
}

/// Assertion that verifies a folder (directory) exists.
///
/// The path can be absolute or relative to the output directory.
#[derive(Debug)]
pub struct FolderExists {
    /// Path to check (relative to output directory or absolute).
    path: String,
}

impl FolderExists {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    /// Resolve the path relative to the output directory.
    fn resolve_path(&self, context: &VerifyContext) -> PathBuf {
        let path = PathBuf::from(&self.path);
        if path.is_absolute() {
            path
        } else {
            // Relative to output directory
            context
                .output_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join(&self.path)
        }
    }
}

impl Assertion for FolderExists {
    fn name(&self) -> &str {
        "folderExists"
    }

    fn verify(&self, context: &VerifyContext) -> anyhow::Result<()> {
        // If rendering failed, we can't check for folders
        if let Some(err) = &context.render_error {
            bail!(
                "Cannot check folder existence: rendering failed with: {}",
                err
            );
        }

        let resolved = self.resolve_path(context);

        if !resolved.exists() {
            bail!("Expected folder does not exist: {}", resolved.display());
        }

        if !resolved.is_dir() {
            bail!(
                "Path exists but is not a folder: {} (is a file)",
                resolved.display()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_context(output_path: PathBuf, render_error: Option<String>) -> VerifyContext {
        VerifyContext {
            output_path: output_path.clone(),
            input_path: PathBuf::from("/tmp/test.qmd"),
            format: "html".to_string(),
            render_error,
            messages: vec![],
        }
    }

    // FileExists tests

    #[test]
    fn test_file_exists_passes_when_file_exists() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "content").unwrap();

        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = FileExists::new("test.txt".to_string());
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_file_exists_fails_when_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = FileExists::new("nonexistent.txt".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_file_exists_fails_when_path_is_directory() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).unwrap();

        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = FileExists::new("subdir".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("is not a file"));
    }

    #[test]
    fn test_file_exists_fails_when_render_error() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, Some("Parse error".to_string()));

        let assertion = FileExists::new("test.txt".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rendering failed"));
    }

    // PathDoesNotExist tests

    #[test]
    fn test_path_does_not_exist_passes_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = PathDoesNotExist::new("nonexistent.txt".to_string());
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_path_does_not_exist_fails_when_file_exists() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("exists.txt");
        fs::write(&file_path, "content").unwrap();

        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = PathDoesNotExist::new("exists.txt".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("should not exist"));
    }

    #[test]
    fn test_path_does_not_exist_fails_when_directory_exists() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).unwrap();

        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = PathDoesNotExist::new("subdir".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("is a directory"));
    }

    #[test]
    fn test_path_does_not_exist_works_with_render_error() {
        // This assertion should work even when rendering failed
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, Some("Parse error".to_string()));

        let assertion = PathDoesNotExist::new("nonexistent.txt".to_string());
        assert!(assertion.verify(&context).is_ok());
    }

    // FolderExists tests

    #[test]
    fn test_folder_exists_passes_when_directory_exists() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).unwrap();

        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = FolderExists::new("subdir".to_string());
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_folder_exists_fails_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = FolderExists::new("nonexistent".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_folder_exists_fails_when_path_is_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();

        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, None);

        let assertion = FolderExists::new("file.txt".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("is not a folder"));
    }

    #[test]
    fn test_folder_exists_fails_when_render_error() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.html");
        let context = make_context(output_path, Some("Parse error".to_string()));

        let assertion = FolderExists::new("subdir".to_string());
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rendering failed"));
    }

    // Test absolute paths

    #[test]
    fn test_file_exists_with_absolute_path() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("absolute.txt");
        fs::write(&file_path, "content").unwrap();

        let output_path = PathBuf::from("/different/output.html");
        let context = make_context(output_path, None);

        let assertion = FileExists::new(file_path.to_string_lossy().to_string());
        assert!(assertion.verify(&context).is_ok());
    }
}
