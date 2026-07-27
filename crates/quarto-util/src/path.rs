//! Cross-platform path utilities

use std::path::Path;

/// Returns `true` if `path` has a root component.
///
/// Use this, not [`std::path::Path::is_absolute`], for "should this path be
/// used as-is, or resolved against a working directory?" decisions. On
/// `wasm32-unknown-unknown` — which std treats as neither `unix` nor `wasi`
/// and which carries no path prefix — `is_absolute()` returns `false` even for
/// rooted paths like `/foo` (it requires a unix/wasi target or a Windows-style
/// prefix), whereas `has_root()` is correct on both native and WASM targets.
/// Same rationale as `quarto-core`'s `artifact.rs` / `output_sink.rs` (bd-cfl67).
pub fn is_rooted(path: &Path) -> bool {
    path.has_root()
}

/// Convert a path to a string using forward slashes only.
///
/// Windows paths like `C:\Users\chris\file.txt` become `C:/Users/chris/file.txt`.
/// On Unix, this is a no-op since paths already use forward slashes.
/// Forward slashes are accepted by Windows APIs, making this safe for file operations.
pub fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Returns `true` if `path` is a URL rather than a filesystem path, and
/// should therefore be emitted verbatim instead of resolved, rebased, or
/// copied.
///
/// Recognizes an RFC-3986-style scheme (`https:`, `data:`, `mailto:`) and
/// the protocol-relative form (`//cdn.example.com/logo.png`).
///
/// **A scheme must be at least two characters.** Quarto 1's equivalent is
/// `/^\w+:/` (`external-sources/quarto-cli/src/core/url.ts:13`), which also
/// matches a Windows drive letter — `C:\logos\brand.png` would be classified
/// as external and then emitted into HTML as-is. The two-character minimum
/// costs nothing (no real scheme is one character) and removes that trap.
///
/// A relative path containing a colon in its first segment — say
/// `my:file.png`, legal on Unix — is classified as a URL. That ambiguity is
/// inherent to the syntax; URLs are overwhelmingly the more likely intent.
pub fn is_external_url(path: &str) -> bool {
    if path.starts_with("//") {
        return true;
    }
    let Some(colon) = path.find(':') else {
        return false;
    };
    if colon < 2 {
        return false;
    }
    let scheme = &path[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
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

    #[test]
    fn external_url_recognizes_schemes() {
        assert!(is_external_url("https://example.com/logo.png"));
        assert!(is_external_url("http://example.com/logo.png"));
        assert!(is_external_url("HTTPS://EXAMPLE.COM/logo.png"));
        assert!(is_external_url("data:image/png;base64,iVBOR"));
        assert!(is_external_url("mailto:someone@example.com"));
        // Protocol-relative.
        assert!(is_external_url("//cdn.example.com/logo.png"));
    }

    #[test]
    fn external_url_rejects_filesystem_paths() {
        assert!(!is_external_url("logo.png"));
        assert!(!is_external_url("assets/logo.png"));
        assert!(!is_external_url("../assets/logo.png"));
        assert!(!is_external_url("/rooted/logo.png"));
        assert!(!is_external_url(""));
    }

    /// The reason this is not Q1's `/^\w+:/`: a Windows drive letter is a
    /// one-character "scheme". Misclassifying `C:\logos\brand.png` as a URL
    /// would emit a local filesystem path into HTML unrebased.
    #[test]
    fn external_url_does_not_match_windows_drive_letters() {
        assert!(!is_external_url(r"C:\logos\brand.png"));
        assert!(!is_external_url("C:/logos/brand.png"));
        assert!(!is_external_url("d:/logo.png"));
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

    #[test]
    fn test_is_rooted_distinguishes_rooted_from_relative() {
        assert!(is_rooted(Path::new("/abs/file.txt")));
        assert!(!is_rooted(Path::new("relative/file.txt")));
    }

    #[cfg(windows)]
    #[test]
    fn test_is_rooted_recognizes_windows_drive_paths() {
        // Guards against regressing to a `starts_with('/')`-style check, which
        // would wrongly report a drive-rooted path as not rooted.
        assert!(is_rooted(Path::new("C:/abs/file.txt")));
    }
}
