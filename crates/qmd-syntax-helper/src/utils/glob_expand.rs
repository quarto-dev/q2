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

            for path in paths {
                let path =
                    path.with_context(|| format!("Failed to read glob match for: {}", pattern))?;
                files.push(path);
            }
        } else {
            // It's a literal path - use as-is
            files.push(PathBuf::from(pattern));
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_path() {
        let patterns = vec!["test.qmd".to_string()];
        let result = expand_globs(&patterns).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], PathBuf::from("test.qmd"));
    }

    #[test]
    fn test_multiple_literals() {
        let patterns = vec!["a.qmd".to_string(), "b.qmd".to_string()];
        let result = expand_globs(&patterns).unwrap();
        assert_eq!(result.len(), 2);
    }
}
