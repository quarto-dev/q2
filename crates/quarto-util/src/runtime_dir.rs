//! Cross-platform per-user runtime directory for Quarto

use std::path::PathBuf;

/// Pure branch logic (no IO): prefer the XDG runtime dir, else the cache dir,
/// namespaced under `quarto`. Returns `None` only if both inputs are `None`.
fn runtime_dir_from(xdg: Option<PathBuf>, cache: Option<PathBuf>) -> Option<PathBuf> {
    xdg.or(cache).map(|d| d.join("quarto"))
}

/// Cross-platform per-user runtime dir for quarto, created if missing.
///
/// Linux: `$XDG_RUNTIME_DIR/quarto`. macOS: `~/Library/Caches/quarto`.
/// Windows: `%LOCALAPPDATA%/quarto`.
///
/// Creates the directory if it does not already exist.
pub fn quarto_runtime_dir() -> std::io::Result<PathBuf> {
    let dir = runtime_dir_from(dirs::runtime_dir(), dirs::cache_dir()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no runtime or cache directory available",
        )
    })?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dir_from_prefers_xdg_over_cache() {
        // XDG wins when present — the result must be under the XDG path,
        // NOT the cache path.
        let xdg = PathBuf::from("/run/user/1000");
        let cache = PathBuf::from("/home/user/.cache");
        let result = runtime_dir_from(Some(xdg.clone()), Some(cache.clone()));
        let result = result.expect("should return Some when xdg is provided");
        assert!(
            result.starts_with(&xdg),
            "expected result under XDG path {xdg:?}, got {result:?}"
        );
        assert!(
            !result.starts_with(&cache),
            "result must NOT be under cache path {cache:?}, got {result:?}"
        );
    }

    #[test]
    fn runtime_dir_from_falls_back_to_cache() {
        let cache = PathBuf::from("/home/user/.cache");
        let result = runtime_dir_from(None, Some(cache.clone()));
        assert_eq!(result, Some(cache.join("quarto")));
    }

    #[test]
    fn runtime_dir_from_namespaces_under_quarto() {
        // Both branches must produce a path whose final component is "quarto".
        let xdg_result = runtime_dir_from(Some(PathBuf::from("/run/user/1000")), None).unwrap();
        assert_eq!(
            xdg_result.file_name().and_then(|n| n.to_str()),
            Some("quarto"),
            "last component of XDG result must be 'quarto', got {xdg_result:?}"
        );

        let cache_result =
            runtime_dir_from(None, Some(PathBuf::from("/home/user/.cache"))).unwrap();
        assert_eq!(
            cache_result.file_name().and_then(|n| n.to_str()),
            Some("quarto"),
            "last component of cache result must be 'quarto', got {cache_result:?}"
        );
    }

    #[test]
    fn runtime_dir_from_both_none_returns_none() {
        assert_eq!(runtime_dir_from(None, None), None);
    }

    #[test]
    fn quarto_runtime_dir_returns_existing_directory() {
        let dir = quarto_runtime_dir().expect("quarto_runtime_dir() should succeed");
        assert!(
            dir.exists(),
            "quarto_runtime_dir() should create the directory, but {dir:?} does not exist"
        );
        assert!(
            dir.is_dir(),
            "quarto_runtime_dir() should be a directory, but {dir:?} is not"
        );
        assert_eq!(
            dir.file_name().and_then(|n| n.to_str()),
            Some("quarto"),
            "last component must be 'quarto', got {dir:?}"
        );
    }
}
