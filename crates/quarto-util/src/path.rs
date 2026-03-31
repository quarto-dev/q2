//! Cross-platform path utilities

use std::path::Path;

/// Convert a path to a string using forward slashes only.
///
/// Windows paths like `C:\Users\chris\file.txt` become `C:/Users/chris/file.txt`.
/// On Unix, this is a no-op since paths already use forward slashes.
/// Forward slashes are accepted by Windows APIs, making this safe for file operations.
pub fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_forward_slashes_preserves_unix_paths() {
        let path = PathBuf::from("relative/path/file.txt");
        assert_eq!(to_forward_slashes(&path), "relative/path/file.txt");
    }

    #[cfg(windows)]
    #[test]
    fn test_forward_slashes_converts_windows_paths() {
        // Use a real OS-provided path that naturally contains backslashes
        let temp = std::env::temp_dir().join("test_file.txt");
        let result = to_forward_slashes(&temp);
        assert!(
            !result.contains('\\'),
            "Expected no backslashes, got: {result}"
        );
        assert!(
            result.contains('/'),
            "Expected forward slashes, got: {result}"
        );
    }
}
