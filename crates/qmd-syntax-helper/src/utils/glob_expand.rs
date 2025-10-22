use anyhow::{Context, Result};
use std::path::PathBuf;

/// Expand glob patterns into a list of file paths
///
/// If a pattern doesn't contain glob characters (*, ?, [, ]),
/// treat it as a literal path.
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for pattern in patterns {
        // Check if pattern contains glob characters
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // It's a glob pattern - expand it
            let paths = glob::glob(pattern)
                .with_context(|| format!("Invalid glob pattern: {}", pattern))?;

            let mut match_count = 0;
            for path in paths {
                let path =
                    path.with_context(|| format!("Failed to read glob match for: {}", pattern))?;
                files.push(path);
                match_count += 1;
            }

            // Warn if glob matched nothing
            if match_count == 0 {
                eprintln!("Warning: No files matched pattern: {}", pattern);
            }
        } else {
            // It's a literal path - verify it exists
            let path = PathBuf::from(pattern);
            if !path.exists() {
                anyhow::bail!("File not found: {}", pattern);
            }
            files.push(path);
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_literal_path() {
        // Create a temporary test file
        let test_file = "test-glob-expand.qmd";
        File::create(test_file).unwrap().write_all(b"test").unwrap();

        let patterns = vec![test_file.to_string()];
        let result = expand_globs(&patterns).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], PathBuf::from(test_file));

        // Clean up
        std::fs::remove_file(test_file).unwrap();
    }

    #[test]
    fn test_multiple_literals() {
        // Create temporary test files
        let test_file_a = "test-a-glob-expand.qmd";
        let test_file_b = "test-b-glob-expand.qmd";
        File::create(test_file_a).unwrap().write_all(b"test").unwrap();
        File::create(test_file_b).unwrap().write_all(b"test").unwrap();

        let patterns = vec![test_file_a.to_string(), test_file_b.to_string()];
        let result = expand_globs(&patterns).unwrap();
        assert_eq!(result.len(), 2);

        // Clean up
        std::fs::remove_file(test_file_a).unwrap();
        std::fs::remove_file(test_file_b).unwrap();
    }

    #[test]
    fn test_nonexistent_file_errors() {
        let patterns = vec!["file-that-does-not-exist.qmd".to_string()];
        let result = expand_globs(&patterns);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File not found"));
    }
}
